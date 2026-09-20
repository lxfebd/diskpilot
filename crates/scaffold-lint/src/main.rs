//! diskpilot-scaffold-lint: statically validates scaffold TOML files.
//!
//! Checks in order:
//!  1. TOML parses into `Scaffold` (schema check).
//!  2. Every scope has a non-empty glob.
//!  3. **Scope glob 红线（CLAUDE.md Hard rule #1）**: no scope glob may match
//!     conversation/account DBs, login config, favorites, or crypto material.
//!     Any hit is a hard error — tighten the glob, don't widen the check.
//!
//! 红线校验走 `diskpilot_scaffold::red_line_violations` —— 与 `scaffold-gen`、
//! 运行时 scaffold 安装/装载共用同一入口、同一份样本，杜绝「CI 一把尺、
//! 运行时另一把尺」的口径漂移。
//!
//! Usage: diskpilot-scaffold-lint <file.toml> [...]

use diskpilot_scaffold::{canonicalize_for_red_line, red_line_violations, Scaffold};
use std::path::Path;

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

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("usage: diskpilot-scaffold-lint <file.toml> [...]");
        std::process::exit(2);
    }

    let mut errors = 0usize;
    for arg in &args {
        let path = Path::new(arg);
        let text = std::fs::read_to_string(path)?;
        match toml::from_str::<Scaffold>(&text) {
            Ok(s) => {
                if s.scopes.is_empty() {
                    eprintln!("WARN: {} has no scopes", path.display());
                }
                let mut violations: Vec<String> = Vec::new();
                for scope in &s.scopes {
                    if scope.glob.trim().is_empty() {
                        errors += 1;
                        eprintln!(
                            "FAIL: {}: scope `{}` has empty glob",
                            path.display(),
                            scope.id
                        );
                        continue;
                    }
                    // 规范化到红线锚定空间（写死的 C:/Users/test，不再靠 set_var
                    // 伪造 env）：这样 lint 与运行时安装/装载走的是同一把尺。
                    let canon = canonicalize_for_red_line(&scope.glob);
                    // 坏 glob → 硬错误（glob 语法错误，会让 scope 失效）。
                    if let Err(e) = build_set(&canon) {
                        errors += 1;
                        eprintln!("FAIL: {}: scope `{}`: {e}", path.display(), scope.id);
                        continue;
                    }
                    // 红线校验与 scaffold-gen / 运行时安装共用同一入口。
                    let hits = red_line_violations(&scope.glob);
                    for sample in &hits {
                        violations.push(format!("  `{}` hits red line: {sample}", scope.glob));
                    }
                }
                for v in &violations {
                    errors += 1;
                    eprintln!("FAIL: {}:{}", path.display(), v);
                }
                if violations.is_empty() {
                    println!(
                        "ok: {} ({}, {} scopes, no red-line hits)",
                        path.display(),
                        s.id,
                        s.scopes.len()
                    );
                }
            }
            Err(e) => {
                errors += 1;
                eprintln!("FAIL: {}: {}", path.display(), e);
            }
        }
    }
    if errors > 0 {
        std::process::exit(1);
    }
    Ok(())
}
