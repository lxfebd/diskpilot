//! diskpilot-scaffold-gen：交互式生成清理脚本（scaffold TOML）。
//!
//! 对应 scaffold 14-phase 工作流的 Phase 8-10 机械化部分（见 development.md）：
//! 交互采集 → 生成 `scaffolds/<id>.toml` → 跑本地红线校验（Phase 9）→
//! 生成 safety test 骨架（Phase 10）。它不替代 14-phase 的调研/实测步骤，
//! 只是把"复制模板 + 手填 + 手工 lint"压缩成一次问答。
//!
//! 用法：
//!   cargo run -p diskpilot-scaffold-gen
//!
//! 生成的文件：
//!   - scaffolds/<id>.toml            （新清理脚本）
//!   - crates/scaffold/tests/<id>_safety.rs （safety 测试骨架，含正向/红线断言占位）
//!
//! 红线校验复用 `diskpilot_scaffold::red_line_violations`（与 scaffold-lint、
//! 运行时安装同一入口、同一份清单）。任何命中都是硬错误——收紧 glob，不要放宽清单。

use anyhow::{anyhow, Context};
use diskpilot_scaffold::red_line_violations;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

fn main() -> anyhow::Result<()> {
    let ws = workspace_root();
    let mut q = Prompt::new();
    let id = q.ask(
        "scaffold id（kebab-case，全仓库唯一，如 wechat-pc）",
        &|s| {
            let ok = !s.is_empty()
                && s.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
            ok && s.ends_with(|c: char| c.is_ascii_alphanumeric())
        },
    )?;
    let id = id.trim().to_string();
    let name = q.ask("显示名（UI 卡片标题，中文 OK）", &|s| {
        !s.trim().is_empty()
    })?;
    let risk = q.ask_enum("风险级（low / medium / high）", &["low", "medium", "high"])?;
    let roots = collect_roots(&mut q)?;
    let buckets = collect_buckets(&mut q)?;
    let policy = q.ask_enum(
        "缓存保留策略（none = 全量清 / days = 保留 N 天）",
        &["none", "days"],
    )?;

    let toml = render_toml(&id, &name, &risk, &roots, &buckets, &policy);
    let toml_path = ws.join("scaffolds").join(format!("{id}.toml"));
    write_new(&toml_path, &toml)?;

    // Phase 9：本地红线校验（与 scaffold-lint、运行时安装共用同一入口）。
    let mut errors = 0usize;
    for scope in extract_globs(&toml) {
        let hits = red_line_violations(&scope);
        if !hits.is_empty() {
            errors += 1;
            eprintln!("FAIL: scope `{scope}` 命中红线: {hits:?}");
        }
    }
    if errors > 0 {
        return Err(anyhow!(
            "生成的 scaffold 命中 {errors} 处红线，已停止。请修改 glob 后重试（不要放宽红线清单）。"
        ));
    }
    println!("lint: {} 通过（红线零命中）", toml_path.display());

    // Phase 10：safety test 骨架。
    let test = render_safety_test(&id);
    let test_path = ws
        .join("crates/scaffold/tests")
        .join(format!("{id}_safety.rs"));
    write_new(&test_path, &test)?;
    println!("test: {} 已生成（请补正向/红线路径后跑 cargo test -p diskpilot-scaffold --test {}_safety）", test_path.display(), id);

    println!("\n完成。建议按 development.md 的 14-phase 流程补完 requirements 文档与实测勘测。");
    Ok(())
}

// ── workspace 定位 ────────────────────────────────────────────────────────

fn workspace_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop(); // out of crates/scaffold
    p.pop(); // out of crates
    p
}

// ── 交互采集 ──────────────────────────────────────────────────────────────

struct Prompt {
    stdin: std::io::Stdin,
}

impl Prompt {
    fn new() -> Self {
        Self {
            stdin: std::io::stdin(),
        }
    }

    fn ask(&mut self, prompt: &str, valid: &dyn Fn(&str) -> bool) -> anyhow::Result<String> {
        loop {
            print!("{prompt}: ");
            std::io::stdout().flush().ok();
            let mut line = String::new();
            let n = self.stdin.lock().read_line(&mut line)?;
            if n == 0 {
                return Err(anyhow!("输入结束（EOF），已取消"));
            }
            let v = line.trim().to_string();
            if valid(&v) {
                return Ok(v);
            }
            println!("无效输入，请重试。");
        }
    }

    fn ask_enum(&mut self, prompt: &str, choices: &[&str]) -> anyhow::Result<String> {
        println!("{prompt} [{}]", choices.join("/"));
        let choices_owned: Vec<String> = choices.iter().map(|c| c.to_string()).collect();
        self.ask("选择", &|s| choices_owned.iter().any(|c| c == s.trim()))
    }
}

/// 数据根路径：`%APPDATA%` / `%LOCALAPPDATA%` / `%USERPROFILE%` / `$HOME` /
/// 自定义绝对路径（正斜杠）。
fn collect_roots(q: &mut Prompt) -> anyhow::Result<Vec<String>> {
    println!("\n数据根路径（detect 用）。输入 0 结束，每行一个：");
    println!("  常用：%LOCALAPPDATA%/App  %APPDATA%/App  %USERPROFILE%/AppData/...  $HOME/.config/App  **/AppData（通配兼容改盘符）");
    let mut out = Vec::new();
    loop {
        let v = q.ask(
            &format!("根路径 {}（0=完成）", out.len() + 1),
            &|s| s == "0" || !s.trim().is_empty(),
        )?;
        if v == "0" {
            break;
        }
        out.push(v.trim().replace('\\', "/"));
    }
    if out.is_empty() {
        return Err(anyhow!("至少需要一个根路径"));
    }
    Ok(out)
}

/// 缓存子目录（L1 桶）：相对根路径的子目录名，生成一个 scope。
fn collect_buckets(q: &mut Prompt) -> anyhow::Result<Vec<String>> {
    println!(
        "\n缓存/日志子目录（每个生成一个 scope，如 Cache / logs / crash-dumps）。输入 0 结束："
    );
    println!("  相对根路径的子目录名（可含通配，如 {{log,cache}}/** 或 account-*/Cache）");
    let mut out = Vec::new();
    loop {
        let v = q.ask(
            &format!("子目录 {}（0=完成）", out.len() + 1),
            &|s| s == "0" || !s.trim().is_empty(),
        )?;
        if v == "0" {
            break;
        }
        out.push(v.trim().replace('\\', "/"));
    }
    if out.is_empty() {
        return Err(anyhow!("至少需要一个缓存子目录"));
    }
    Ok(out)
}

// ── TOML 渲染 ─────────────────────────────────────────────────────────────

fn render_toml(
    id: &str,
    name: &str,
    risk: &str,
    roots: &[String],
    buckets: &[String],
    policy: &str,
) -> String {
    let mut s = String::new();
    s.push_str(&format!(
        "id          = \"{id}\"\n\
         name        = \"{name}\"\n\
         homepage    = \"\"\n\
         risk        = \"{risk}\"\n\
         disclaimer  = \"\"\"\\\n\
         由 diskpilot-scaffold-gen 生成，请按 development.md 的 14-phase 流程补完：\n\
         TODO: 显式列出\"绝不删 X / Y / Z\"。\\\n\
         明确\"删除后某些功能可能受影响\"。\\\n\
         明确\"走回收站可还原\"。\\\n\
         不要使用\"安全\"二字。\\\n\
         \"\"\"\n"
    ));
    s.push('\n');
    s.push_str("detect = [\n");
    for r in roots {
        s.push_str(&format!("  \"{r}\",\n"));
    }
    s.push_str("]\n\n[match]\nname_contains = []\nmust_have_child = []\n");
    for (i, b) in buckets.iter().enumerate() {
        let scope_id = format!("{id}-scope-{}", i + 1);
        let (prompt_line, glob_base) = if policy == "days" {
            (
                "prompt = { kind = \"days\", default = 30, label = \"Delete older than (days)\" }",
                b.trim_end_matches("/**").to_string(),
            )
        } else {
            (
                "prompt = { kind = \"none\" }",
                b.trim_end_matches("/**").to_string(),
            )
        };
        // glob 锚定到 detect 根路径下：{root1,root2}/<bucket>/**，绝不裸奔
        // 成全盘匹配（`Cache/**` 会误扫任意目录下的 Cache）。
        let roots_brace = format!("{{{}}}", roots.join(","));
        let glob = format!("{roots_brace}/{glob_base}/**");
        s.push_str(&format!(
            "\n[[scope]]\n\
             id     = \"{scope_id}\"\n\
             label  = \"{b} 缓存\"\n\
             glob   = \"{glob}\"\n\
             mode   = \"recycle\"\n\
             category = \"cache\"\n\
             {prompt_line}\n\
             recycle_granularity = \"directory\"\n"
        ));
        let _ = i;
    }
    s
}

/// 从 TOML 文本提取所有 `[[scope]]` 的 glob（用于本地红线校验）。
fn extract_globs(toml: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_scope = false;
    for line in toml.lines() {
        let t = line.trim();
        if t.starts_with("[[scope]]") {
            in_scope = true;
            continue;
        }
        if in_scope && t.starts_with('[') && !t.starts_with("[[scope]]") {
            in_scope = false;
        }
        if in_scope && t.starts_with("glob") {
            if let Some(v) = t.split('=').nth(1) {
                out.push(v.trim().trim_matches('"').to_string());
            }
        }
    }
    out
}

// ── safety test 骨架 ──────────────────────────────────────────────────────

fn render_safety_test(id: &str) -> String {
    format!(
        r#"//! 由 diskpilot-scaffold-gen 生成的 safety 测试骨架。
//! 按 development.md 的 14-phase 流程 Phase 10 补完：
//!   1. positives：每个 scope id 至少一条命中路径
//!   2. red_lines：该应用特有"看似可清实则不可清"目录
//! 跑：cargo test -p diskpilot-scaffold --test {id}_safety
//! 红线断言失败 = glob 写宽了——收紧 glob，不要放宽测试。

use std::path::PathBuf;

const SCAFFOLD_FILE: &str = "scaffolds/{id}.toml";

fn workspace_root() -> PathBuf {{
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p.pop();
    p
}}

fn load_scaffold() -> diskpilot_scaffold::Scaffold {{
    let path = workspace_root().join(SCAFFOLD_FILE);
    let text = std::fs::read_to_string(&path).expect("read scaffold toml");
    toml::from_str(&text).expect("parse scaffold toml")
}}

fn build_set(pattern: &str) -> globset::GlobSet {{
    let g = globset::GlobBuilder::new(pattern)
        .literal_separator(false)
        .case_insensitive(true)
        .build()
        .unwrap_or_else(|e| panic!("bad glob `{{pattern}}`: {{e}}"));
    let mut b = globset::GlobSetBuilder::new();
    b.add(g);
    b.build().unwrap()
}}

fn expand(s: &str) -> String {{
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {{
        if bytes[i] == b'%' {{
            if let Some(end) = bytes[i + 1..].iter().position(|&b| b == b'%') {{
                let var = std::str::from_utf8(&bytes[i + 1..i + 1 + end]).unwrap_or("");
                if let Ok(v) = std::env::var(var) {{
                    out.push_str(&v.replace('\\', "/"));
                    i += end + 2;
                    continue;
                }}
            }}
        }}
        out.push(bytes[i] as char);
        i += 1;
    }}
    out
}}

fn matching_scopes<'a>(scopes: &'a [(String, globset::GlobSet)], path: &str) -> Vec<&'a str> {{
    scopes
        .iter()
        .filter_map(|(id, gs)| if gs.is_match(path) {{ Some(id.as_str()) }} else {{ None }})
        .collect()
}}

#[test]
fn {id}_globs_are_safe() {{
    std::env::set_var("USERPROFILE", "C:/Users/test");
    std::env::set_var("APPDATA", "C:/Users/test/AppData/Roaming");
    std::env::set_var("LOCALAPPDATA", "C:/Users/test/AppData/Local");
    std::env::set_var("HOME", "/home/test");

    let scaffold = load_scaffold();
    let scopes: Vec<(String, globset::GlobSet)> = scaffold
        .scopes
        .iter()
        .map(|s| (s.id.clone(), build_set(&expand(&s.glob))))
        .collect();

    // 正向断言：每个 scope id 至少一条命中路径
    let positives: &[(&str, &str)] = &[
        // TODO: ("scope-id", "C:/Users/test/.../<bucket>/file.dat"),
    ];

    for (expected_id, p) in positives {{
        let hits = matching_scopes(&scopes, p);
        assert!(hits.contains(expected_id), "expected scope `{{expected_id}}` to match `{{p}}`, got {{hits:?}}");
    }}

    // 红线断言：聊天/账号 DB、config/login/Accounts、Favorite、加密 key 等
    let red_lines: &[&str] = &[
        // TODO: 该 app 特有红线路径
        // "C:/Users/test/.../config/account.cfg",
    ];

    let mut violations = Vec::new();
    for p in red_lines {{
        let hits = matching_scopes(&scopes, p);
        if !hits.is_empty() {{
            violations.push(format!("`{{p}}` -> {{hits:?}}"));
        }}
    }}
    assert!(violations.is_empty(), "{id}.toml glob hit red lines:\n  {{}}", violations.join("\n  "));
}}
"#
    )
}

/// 写新文件；已存在则报错（不覆盖用户可能改过的文件）。
fn write_new(path: &Path, content: &str) -> anyhow::Result<()> {
    if path.exists() {
        return Err(anyhow!(
            "{} 已存在，不覆盖（如需重写请先删除）",
            path.display()
        ));
    }
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p).with_context(|| format!("创建目录 {}", p.display()))?;
    }
    std::fs::write(path, content).with_context(|| format!("写入 {}", path.display()))?;
    Ok(())
}
