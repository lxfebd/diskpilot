//! diskpilot-scaffold-lint: statically validates scaffold TOML files.
//!
//! Checks in order:
//!  1. TOML parses into `Scaffold` (schema check).
//!  2. Every scope has a non-empty glob.
//!  3. **Scope glob 红线（CLAUDE.md Hard rule #1）**: no scope glob may match
//!     conversation/account DBs, login config, favorites, or crypto material.
//!     Any hit is a hard error — tighten the glob, don't widen the check.
//!  4. **目录粒度强制 Recycle**: `recycle_granularity = "directory"` 的 scope
//!     mode 恒被执行层忽略（一律走 Recycle，见 desktop/executor.rs），TOML 里
//!     写 quarantine/delete 是静默不生效的陷阱——lint 必须 FAIL 拦住。
//!
//! 红线校验走 `diskpilot_scaffold::red_line_violations` —— 与 `scaffold-gen`、
//! 运行时 scaffold 安装/装载共用同一入口、同一份样本，杜绝「CI 一把尺、
//! 运行时另一把尺」的口径漂移。
//!
//! Usage:
//!   diskpilot-scaffold-lint <file.toml> [...]     校验既有 scaffold（原行为）
//!   diskpilot-scaffold-lint list                  列出 scaffolds/ 概况
//!   diskpilot-scaffold-lint new <id> [--name N] [--risk low|medium|high]
//!                                                  从模板生成新 scaffold 并自检

use diskpilot_scaffold::{canonicalize_for_red_line, red_line_violations, Scaffold};
use std::path::{Path, PathBuf};

fn build_set(pattern: &str) -> Result<globset::GlobSet, String> {
    let g = globset::GlobBuilder::new(pattern)
        .literal_separator(false)
        .case_insensitive(true)
        .build()
        .map_err(|e| format!("bad glob `{pattern}`: {e}"))?;
    let mut b = globset::GlobSetBuilder::new();
    b.add(g);
    b.build()
        .map_err(|e| format!("bad globset `{pattern}`: {e}"))
}

/// 校验一份 scaffold 文本；返回 (is_ok, error_count)。
/// 规则与 `lint_one_path` 完全一致，供 `new` 子命令写盘前自检。
fn lint_text(display: &str, text: &str) -> (bool, usize) {
    let mut errors = 0usize;
    match toml::from_str::<Scaffold>(text) {
        Ok(s) => {
            if s.scopes.is_empty() {
                eprintln!("WARN: {display} has no scopes");
            }
            let mut violations: Vec<String> = Vec::new();
            for scope in &s.scopes {
                if scope.glob.trim().is_empty() {
                    errors += 1;
                    eprintln!("FAIL: {display}: scope `{}` has empty glob", scope.id);
                    continue;
                }
                // 规范化到红线锚定空间（写死的 C:/Users/test，不再靠 set_var
                // 伪造 env）：这样 lint 与运行时安装/装载走的是同一把尺。
                let canon = canonicalize_for_red_line(&scope.glob);
                // 坏 glob → 硬错误（glob 语法错误，会让 scope 失效）。
                if let Err(e) = build_set(&canon) {
                    errors += 1;
                    eprintln!("FAIL: {display}: scope `{}`: {e}", scope.id);
                    continue;
                }
                // 红线校验与 scaffold-gen / 运行时安装共用同一入口。
                let hits = red_line_violations(&scope.glob);
                for sample in &hits {
                    violations.push(format!("  `{}` hits red line: {sample}", scope.glob));
                }
                // 目录粒度 scope 的 mode 会被执行层忽略（强制 Recycle，见
                // desktop/executor.rs `Directory granularity is locked to Recycle`）：
                // TOML 写 quarantine/delete 是静默不生效的陷阱，lint 直接拦下。
                // （规则实现抽在 diskpilot_scaffold 便于单测，两处共用一把尺。）
                for (sid, mode) in diskpilot_scaffold::directory_scope_recycle_violations(&s) {
                    violations.push(format!(
                        "  scope `{sid}`: recycle_granularity=directory 时 mode={mode:?} 会被忽略（目录粒度恒走 Recycle），请改为 mode=\"recycle\""
                    ));
                }
            }
            for v in &violations {
                errors += 1;
                eprintln!("FAIL: {display}:{v}");
            }
            if violations.is_empty() {
                println!(
                    "ok: {display} ({}, {} scopes, no red-line hits)",
                    s.id,
                    s.scopes.len()
                );
            }
        }
        Err(e) => {
            errors += 1;
            eprintln!("FAIL: {display}: {e}");
        }
    }
    (errors == 0, errors)
}

/// 校验单个文件路径；返回进程退出码（0 或 1）。
fn lint_one_path(path: &Path) -> i32 {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("FAIL: {}: {}", path.display(), e);
            return 1;
        }
    };
    let (ok, _) = lint_text(&path.display().to_string(), &text);
    if ok {
        0
    } else {
        1
    }
}

// ── list 子命令 ──────────────────────────────────────────────────────────

fn list_scaffolds(scaffolds_dir: &Path) -> i32 {
    let mut files: Vec<PathBuf> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(scaffolds_dir) {
        for entry in rd.flatten() {
            let p = entry.path();
            if p.extension().map(|e| e == "toml").unwrap_or(false)
                && !p.components().any(|c| c.as_os_str() == "_templates")
            {
                files.push(p);
            }
        }
    }
    files.sort();
    if files.is_empty() {
        println!("（无 scaffold）");
        return 0;
    }
    let h_id = "id";
    let h_name = "name";
    let h_scopes = "scopes";
    let h_risk = "risk";
    println!("{h_id:<28} {h_name:<34} {h_scopes:>6}  {h_risk}");
    println!("{:-<84}", "");
    for f in &files {
        let text = match std::fs::read_to_string(f) {
            Ok(t) => t,
            Err(_) => continue,
        };
        if let Ok(s) = toml::from_str::<Scaffold>(&text) {
            println!(
                "{:<28} {:<34} {:>6}  {}",
                s.id,
                s.name,
                s.scopes.len(),
                match s.risk {
                    diskpilot_scaffold::Risk::Low => "low",
                    diskpilot_scaffold::Risk::Medium => "medium",
                    diskpilot_scaffold::Risk::High => "high",
                }
            );
        }
    }
    println!("\n{} 份", files.len());
    0
}

// ── new 子命令 ──────────────────────────────────────────────────────────

/// 生成新 scaffold 的 TOML 文本：基于官方模板，替换 id/name/risk。
/// scope 留给作者按 14-phase 工作流实地勘测后填充——模板注释即指引。
fn render_new_scaffold(id: &str, name: &str, risk: &str) -> String {
    let template = include_str!("../../../scaffolds/_templates/scaffold.toml");
    template
        .replace("TODO-kebab-case-id", id)
        .replace("TODO Display Name", name)
        // 只替换第一处 risk（模板头部那行）；scope 里不出现 risk 字段，安全。
        .replacen(
            "risk        = \"low\"",
            &format!("risk        = \"{risk}\""),
            1,
        )
}

fn cmd_new(scaffolds_dir: &Path, args: &[String]) -> i32 {
    if args.is_empty() {
        eprintln!("usage: diskpilot-scaffold-lint new <id> [--name NAME] [--risk low|medium|high]");
        return 2;
    }
    let id = args[0].clone();
    // id 合法性：kebab-case 小写字母/数字/连字符，不能以 - 开头
    let ok_id = !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && id.chars().next().map(|c| c != '-').unwrap_or(false);
    if !ok_id {
        eprintln!("FAIL: id `{id}` 必须是 kebab-case（小写字母/数字/连字符，不能以 - 开头）");
        return 2;
    }

    let mut name = id.clone();
    let mut risk = "low".to_string();
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--name" => {
                if i + 1 < args.len() {
                    name = args[i + 1].clone();
                    i += 2;
                } else {
                    eprintln!("FAIL: --name 缺少值");
                    return 2;
                }
            }
            "--risk" => {
                if i + 1 < args.len() {
                    risk = args[i + 1].clone();
                    i += 2;
                } else {
                    eprintln!("FAIL: --risk 缺少值");
                    return 2;
                }
            }
            other => {
                eprintln!("FAIL: 未知参数 `{other}`");
                return 2;
            }
        }
    }
    if !matches!(risk.as_str(), "low" | "medium" | "high") {
        eprintln!("FAIL: risk 必须是 low / medium / high");
        return 2;
    }

    let dest = scaffolds_dir.join(format!("{id}.toml"));
    if dest.exists() {
        eprintln!("FAIL: 已存在 {:?}，拒绝覆盖（先删旧文件或用别的 id）", dest);
        return 1;
    }
    let text = render_new_scaffold(&id, &name, &risk);
    // 写盘前先自检，避免生成坏文件
    let (ok, _) = lint_text(&id, &text);
    if !ok {
        eprintln!("FAIL: 生成内容未通过自检，未写入");
        return 1;
    }
    if let Err(e) = std::fs::write(&dest, text) {
        eprintln!("FAIL: 写入 {:?} 失败: {e}", dest);
        return 1;
    }
    println!("ok: 已生成 {:?}（{name}, risk={risk}）并自检通过", dest);
    println!();
    println!("下一步（按 14-phase 工作流，见 .claude/commands/add-scaffold.md）:");
    println!("  1. 实地勘测真实目录，替换 detect / [match] / [[scope]] 占位内容");
    println!("  2. 跑  cargo run -p diskpilot-scaffold-lint -- {id}.toml");
    println!("  3. 复制 safety test 模板并填入断言:  crates/scaffold/tests/{id}_safety.rs");
    println!("  4. 跑  cargo test -p diskpilot-scaffold --test {id}_safety");
    0
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!(
            "usage: diskpilot-scaffold-lint <file.toml> [...] | list | new <id> [--name NAME] [--risk low|medium|high]"
        );
        std::process::exit(2);
    }

    let scaffolds_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../scaffolds");
    match args[0].as_str() {
        "list" => std::process::exit(list_scaffolds(&scaffolds_dir)),
        "new" => std::process::exit(cmd_new(&scaffolds_dir, &args[1..])),
        _ => {
            // 兼容旧用法：逐个 lint 文件
            let mut code = 0i32;
            for arg in &args {
                code |= lint_one_path(Path::new(arg));
            }
            std::process::exit(code);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 最小合法 scaffold：一条 cache scope，不触红线。
    const VALID: &str = r#"
id = "test-scaffold"
name = "Test Scaffold"
risk = "low"
disclaimer = "test only"
detect = ["%LOCALAPPDATA%/TestApp"]

[match]
name_contains = ["TestApp"]

[[scope]]
id = "cache"
label = "缓存"
glob = "**/TestApp/Cache/**"
mode = "recycle"
"#;

    #[test]
    fn valid_scaffold_passes() {
        let (ok, errors) = lint_text("valid", VALID);
        assert!(ok, "valid scaffold must pass, errors={errors}");
        assert_eq!(errors, 0);
    }

    #[test]
    fn malformed_toml_is_hard_error() {
        let (ok, errors) = lint_text("bad", "id = not-valid-toml [[[");
        assert!(!ok);
        assert_eq!(errors, 1);
    }

    #[test]
    fn empty_glob_scope_is_hard_error() {
        let text = VALID.replace(r#"glob = "**/TestApp/Cache/**""#, r#"glob = "  ""#);
        let (ok, errors) = lint_text("empty-glob", &text);
        assert!(
            !ok,
            "empty glob must fail (scope would silently match nothing)"
        );
        assert!(errors >= 1);
    }

    #[test]
    fn red_line_glob_is_hard_error() {
        // 与运行时安装共用 red_line_violations 同一把尺：命中 Firefox 登录库样本即 FAIL。
        let text = VALID.replace(
            r#"glob = "**/TestApp/Cache/**""#,
            r#"glob = "**/Firefox/Profiles/**/logins.json""#,
        );
        let (ok, errors) = lint_text("redline", &text);
        assert!(!ok, "scope glob hitting account/login DB must be refused");
        assert!(errors >= 1);
    }

    #[test]
    fn directory_granularity_with_non_recycle_mode_is_hard_error() {
        // 执行层对目录粒度恒走 Recycle（desktop/executor.rs），TOML 写 quarantine
        // 是静默不生效的陷阱——lint 必须拦下，不能放行坏配置。
        let text = VALID.replace(
            "mode = \"recycle\"",
            "mode = \"quarantine\"\nrecycle_granularity = \"directory\"",
        );
        let (ok, _) = lint_text("dir-gran", &text);
        assert!(
            !ok,
            "directory granularity + quarantine mode must fail lint"
        );
    }

    #[test]
    fn bad_glob_syntax_is_hard_error() {
        let text = VALID.replace(
            r#"glob = "**/TestApp/Cache/**""#,
            r#"glob = "**/[unclosed/**""#,
        );
        let (ok, errors) = lint_text("bad-glob", &text);
        assert!(!ok);
        assert!(errors >= 1);
    }

    #[test]
    fn render_new_scaffold_substitutes_and_self_lints() {
        // cmd_new 写盘前自检依赖这条链路：模板替换 id/name/risk 后必须仍过 lint。
        let text = render_new_scaffold("my-new-app", "My New App", "medium");
        assert!(
            text.contains(r#"id          = "my-new-app""#),
            "id substituted"
        );
        assert!(
            text.contains(r#"name        = "My New App""#),
            "name substituted"
        );
        assert!(
            text.contains(r#"risk        = "medium""#),
            "risk substituted"
        );
        assert!(
            !text.contains("TODO-kebab-case-id"),
            "template placeholder gone"
        );
        let (ok, errors) = lint_text("my-new-app", &text);
        assert!(
            ok,
            "generated scaffold must self-lint clean, errors={errors}"
        );
    }

    #[test]
    fn build_set_rejects_bad_glob() {
        assert!(build_set("**/[abc").is_err());
        assert!(build_set("**/ok/**").is_ok());
    }
}
