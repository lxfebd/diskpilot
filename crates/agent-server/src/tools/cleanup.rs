//! 可清理建议（只读）：按 scaffold 的 scope glob 对目标目录做单遍匹配，
//! 统计每个 scope 命中的文件数/字节，输出「按软件分类的可清理项」。
//!
//! **纯工具文件**：不接路由，由 `tools/mod.rs` 薄壳对接 MCP tool。
//! **全部只读**：只 walk + glob 匹配 + 元数据统计，**绝不删除/移动/改写任何文件**
//! （清理执行属于主项目 executor 域，这里只给 AI 出建议清单）。
//!
//! ## 与主项目的关系
//!
//! 主项目 `apps/desktop/src-tauri/src/cleanup.rs` 的 `cleanup_suggestions`
//! 绑定 Tauri `AppState`（scan_tree / cleanup_cache 缓存），agent-server 是
//! 独立进程无法复用；这里直接依赖 `diskpilot-scaffold` crate 读
//! `scaffolds/*.toml`，复刻「单遍 walk + glob 匹配」的 tally 语义。
//! 性能：单遍并行 walk + 每个路径用编译好的 GlobSet 判命中，不重复遍历。
//!
//! ## scaffold 清单来源
//!
//! 按优先级：
//! 1. 环境变量 `DISKPILOT_SCAFFOLDS`（指向 scaffolds 目录；桌面主进程 agent.rs
//!    拉起 agent-server 时已把主项目打包路径带进来）；
//! 2. agent-server 的 `../../../scaffolds`（dev 仓库布局）；
//! 3. 相邻 `crates/../scaffolds` 兜底。
//!
//! ## 本文件导出函数清单
//!
//! 1. `pub fn collect_cleanup_suggestions(root: String, top_n: usize) -> Result<String, String>`
//!    读 scaffolds → 对 `root` 下目录做单遍并行 walk → 统计每个 scope
//!    命中文件数/总字节 → 按字节降序输出 Top N 个可清理项。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use diskpilot_scaffold::{compile_all, detect_compiled, expand_env, load_dir, Scope};

use super::files::PathGuard;

/// 单次遍历的文件硬上限：C 盘几十万文件也够（与 diskx 大文件扫描同量级）。
const MAX_FILES_WALK: usize = 500_000;

/// 候选 scaffolds 目录（按优先级取第一个存在的）。
fn candidate_scaffold_dirs() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Ok(env) = std::env::var("DISKPILOT_SCAFFOLDS") {
        if !env.trim().is_empty() {
            v.push(PathBuf::from(env));
        }
    }
    let cargo_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    v.push(cargo_dir.join("../../../scaffolds"));
    v.push(cargo_dir.join("../scaffolds"));
    v.push(PathBuf::from("scaffolds"));
    v
}

/// 读取 scaffolds 目录里的所有 TOML；失败返回中文错误串。
fn load_scaffolds() -> Result<Vec<diskpilot_scaffold::Scaffold>, String> {
    let mut last_err: Option<String> = None;
    for dir in candidate_scaffold_dirs() {
        if !dir.is_dir() {
            continue;
        }
        // load_dir 已做 .toml 过滤 + parse；个别坏文件跳过（对齐主项目容错）。
        // 红线闸与主项目装载闸同尺：命中红线的脚本不进建议引擎。
        match load_dir(&dir) {
            Ok(list) => {
                let list: Vec<diskpilot_scaffold::Scaffold> = list
                    .into_iter()
                    .filter(|sc| diskpilot_scaffold::scaffold_red_line_violations(sc).is_empty())
                    .collect();
                if !list.is_empty() {
                    return Ok(list);
                }
                last_err = Some(format!("{} 下没有可解析的 scaffold TOML", dir.display()));
            }
            Err(e) => {
                last_err = Some(format!("{}：{e}", dir.display()));
            }
        }
    }
    Err(match last_err {
        Some(e) => format!("找不到 scaffolds 清单（{e}）。装好 DiskPilot 或设 DISKPILOT_SCAFFOLDS 指向 scaffolds 目录。"),
        None => "找不到 scaffolds 清单目录（已按候选路径逐个找过：DISKPILOT_SCAFFOLDS / 仓库 scaffolds/）。".into(),
    })
}

/// 判断某路径（文件）是否被某个已展开的 scope glob 命中。
/// 复刻主项目 cleanup.rs `build_scope_builds` 语义：glob 先 `expand_env`
/// 展开 `%APPDATA%` 等环境变量、反斜杠转正斜杠，再对**完整绝对路径**
/// （正斜杠归一化）做匹配；literal_separator(false) + case_insensitive(true)
/// 保持一致。不命中返回 None。
fn scope_matches(build: &globset::GlobMatcher, path: &str) -> bool {
    build.is_match(path)
}

/// 预编译一个 scope 的 glob 为 GlobMatcher（展开环境变量 + 正斜杠归一化）。
/// 编译失败（glob 语法错误）返回 None，调用方跳过该 scope（对齐主项目容错）。
fn compile_scope_matcher(glob: &str) -> Option<globset::GlobMatcher> {
    let pat = expand_env(glob).replace('\\', "/");
    globset::GlobBuilder::new(&pat)
        .literal_separator(false)
        .case_insensitive(true)
        .build()
        .ok()
        .map(|g| g.compile_matcher())
}

/// 可清理建议（只读）。
///
/// `root`：要扫的目标目录（通常盘根如 `C:\`）。`top_n`：返回按可清理字节
/// 降序的 Top N 个 scope（默认 20、钳 1..=50）。
///
/// 输出：按软件分类的清单 + 每项文件数/总字节 + 「下一步该做什么」提示
/// （不自动执行；执行由用户确认后走主项目清理流程）。
#[cfg(windows)]
pub fn collect_cleanup_suggestions(root: String, top_n: usize) -> Result<String, String> {
    let p = Path::new(&root);
    // 清理建议的守卫与普通文件工具不同：**允许盘根**（如 `C:\`）——本工具
    // 只读统计 scaffold 缓存的字节/文件数，唯一正确用法就是传盘根全盘匹配；
    // 但系统目录 / 用户主目录根仍拒绝（避免在 Windows/Users 里瞎扫）。
    if !p.exists() {
        return Err(format!("路径不存在：{root}"));
    }
    // 盘根判定走守卫规范化（展开 ./.. 变体），避免 C:/./ 之类绕过被当普通目录
    let canon = super::files::guard_canonical(p);
    let trimmed = canon.to_string_lossy();
    let trimmed = trimmed.trim_end_matches('\\').trim_end_matches('/');
    let is_root = trimmed.len() == 2 && trimmed.ends_with(':');
    if !is_root {
        PathGuard
            .check(p)
            .map_err(|e| format!("路径被安全守卫拒绝：{e}"))?;
    }

    let scaffolds = load_scaffolds()?;
    let compiled = compile_all(&scaffolds);
    // scaffold_id -> scopes（便于按 detect 命中后取 scope 列表）+ 预编译 matcher。
    let mut scopes_by_scaffold: BTreeMap<String, Vec<Scope>> = BTreeMap::new();
    let mut matchers_by_scaffold: BTreeMap<String, Vec<Option<globset::GlobMatcher>>> =
        BTreeMap::new();
    for s in &scaffolds {
        scopes_by_scaffold.insert(s.id.clone(), s.scopes.clone());
        matchers_by_scaffold.insert(
            s.id.clone(),
            s.scopes
                .iter()
                .map(|sc| compile_scope_matcher(&sc.glob))
                .collect(),
        );
    }

    // 单遍并行 walk root：每个目录先用 compiled detect 判断是否命中某 scaffold，
    // 命中后再对该目录内文件做 scope glob 匹配、累加字节/文件数。
    let mut tally: BTreeMap<(String, String), (u64, u64)> = BTreeMap::new(); // (scaffold, scope) -> (bytes, files)
    let mut files_seen = 0usize;
    let mut truncated = false;

    let walker = jwalk::WalkDir::new(p)
        .skip_hidden(false)
        .follow_links(false)
        .max_depth(48);

    for entry in walker {
        let Ok(entry) = entry else { continue };
        if files_seen >= MAX_FILES_WALK {
            truncated = true;
            break;
        }
        if entry.path_is_symlink() {
            continue;
        }
        let ft = entry.file_type();
        let path = entry.path();
        // 只认目录（scope 以目录/目录树为清理单位）。
        if !ft.is_dir() {
            continue;
        }
        // 内存子目录名（避免逐目录 exists 磁盘调用——主项目 scan tag 同款优化）。
        let children: Vec<String> = std::fs::read_dir(&path)
            .map(|rd| {
                rd.flatten()
                    .filter_map(|e| e.file_name().to_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        let Some(sc_id) = detect_compiled(&compiled, &path, Some(&children)) else {
            continue;
        };
        let Some(scopes) = scopes_by_scaffold.get(&sc_id) else {
            continue;
        };
        let Some(matchers) = matchers_by_scaffold.get(&sc_id) else {
            continue;
        };
        // 命中 scaffold：递归该目录下的文件，做 scope glob 匹配累加。
        // max_depth(8) 防打满（多数 scope 目录树在 8 层内）。
        for e2 in jwalk::WalkDir::new(&path)
            .skip_hidden(false)
            .follow_links(false)
            .max_depth(8)
        {
            let Ok(e2) = e2 else { continue };
            if files_seen >= MAX_FILES_WALK {
                truncated = true;
                break;
            }
            if e2.path_is_symlink() || !e2.file_type().is_file() {
                continue;
            }
            files_seen += 1;
            let Some(len) = e2.metadata().map(|m| m.len()).ok().filter(|&x| x > 0) else {
                continue;
            };
            // 对齐主项目：scope glob 匹配**完整绝对路径**（正斜杠归一化），
            // 不再剥掉 detect 命中目录（否则 `**/xwechat_files/...` 永远配不上）。
            let fpath = e2.path();
            let abs = fpath.to_string_lossy().replace('\\', "/");
            for (scope, m) in scopes.iter().zip(matchers.iter()) {
                let Some(m) = m else { continue };
                if scope_matches(m, &abs) {
                    let e = tally.entry((sc_id.clone(), scope.id.clone())).or_default();
                    e.0 = e.0.saturating_add(len);
                    e.1 += 1;
                }
            }
        }
    }

    if tally.is_empty() {
        return Ok(format!(
            "{} 下没有匹配到已知清理目标（共扫描 {files_seen} 个文件）。\n\
             已知软件清单见项目 scaffolds/ 目录；没列出的软件无法给出清理建议。",
            root
        ));
    }

    let mut items: Vec<(String, String, u64, u64)> = tally
        .into_iter()
        .map(|((sc, scp), (bytes, files))| (sc, scp, bytes, files))
        .collect();
    items.sort_by_key(|x| std::cmp::Reverse(x.2));
    let n = if top_n == 0 { 20 } else { top_n.clamp(1, 50) };
    let truncated_items = items.len() > n;
    items.truncate(n);

    let total_bytes: u64 = items.iter().map(|x| x.2).sum();
    let total_files: u64 = items.iter().map(|x| x.3).sum();

    let mut s = format!(
        "{} 可清理建议（{} 项命中，共 {} 文件 / {}）：\n",
        root,
        items.len(),
        total_files,
        fmt_bytes(total_bytes)
    );
    for (sc, scp, bytes, files) in &items {
        let sc_name = scaffolds
            .iter()
            .find(|x| &x.id == sc)
            .map(|x| x.name.as_str())
            .unwrap_or(sc);
        let scp_label = scopes_by_scaffold
            .get(sc)
            .and_then(|v| v.iter().find(|x| &x.id == scp).map(|x| x.label.as_str()))
            .unwrap_or(scp);
        s.push_str(&format!(
            "- [{sc_name}] {scp_label}：{files} 个文件，{0}\n",
            fmt_bytes(*bytes)
        ));
    }
    s.push_str(&format!(
        "\n说明：以上为只读统计，**未删除任何文件**。清理执行需在 DiskPilot 主界面\
         选中清单并逐项确认后走系统回收站（可还原）；AI 不会自动执行清理。\
         命中文件数上限 {MAX_FILES_WALK}，超出标 truncated。"
    ));
    if truncated || truncated_items {
        s.push_str("\n⚠ 已达上限，结果不完整（文件遍历或 Top N 截断）");
    }
    Ok(s)
}

#[cfg(not(windows))]
pub fn collect_cleanup_suggestions(_root: String, _top_n: usize) -> Result<String, String> {
    Ok("当前平台不是 Windows，清理建议不可用".into())
}

/// 字节数人类可读（同 `mod.rs::fmt_bytes` 语义；rustc 1.98 用 `{:.*}`）。
fn fmt_bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = n as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 {
        format!("{n} B")
    } else {
        format!("{:.*} {}", 1, v, UNITS[u])
    }
}
