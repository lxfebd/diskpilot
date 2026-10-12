//! Loads scaffold TOML manifests and matches them against folders.

pub mod winapp2;

use serde::{Deserialize, Serialize};
use std::path::Path;

use red_line::{make_globset, norm};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Scaffold {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub homepage: Option<String>,
    pub risk: Risk,
    pub disclaimer: String,
    pub detect: Vec<String>,
    #[serde(rename = "match", default)]
    pub matcher: Match,
    // TOML uses `[[scope]]` blocks (singular) — keep that for authoring ergonomics.
    // JSON to the frontend uses `scopes` (plural) so it matches the TS Scaffold type.
    #[serde(
        rename(deserialize = "scope", serialize = "scopes"),
        alias = "scopes",
        default
    )]
    pub scopes: Vec<Scope>,
    /// 可选标签：驱动前端分组/筛选（如 "privacy" 归入隐私清理页）。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Match {
    #[serde(default)]
    pub name_contains: Vec<String>,
    #[serde(default)]
    pub must_have_child: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Scope {
    pub id: String,
    pub label: String,
    pub glob: String,
    pub mode: Mode,
    #[serde(default)]
    pub prompt: Option<Prompt>,
    /// "cache" | "media" | "backup". `None` is treated by the UI as "cache".
    /// Drives Studio's grouping: media → top, cache → merged into one button,
    /// backup → bottom.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    /// Optional product-version tag (e.g. "3.x" / "4.x" for WeChat). When set,
    /// the UI hides this scope unless the variant is detected in the matched
    /// paths — keeps obsolete-version buckets out of sight without deleting
    /// them from the scaffold.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
    /// "file" (default) → glob matches files, recycle file-by-file (one
    /// Recycle Bin entry per file). Right for media buckets where each file
    /// is meaningful (chat images, log files).
    /// "directory" → glob matches **directories**, recycle each as a single
    /// unit (one Recycle Bin entry per dir). Required for any scope that
    /// targets thousands of small files in self-contained subdirs (conda
    /// pkgs/<pkg>, conda envs/<name>, node_modules/, cargo target/) — the
    /// per-file path would create thousands of Recycle Bin entries and take
    /// minutes; per-directory creates one entry per logical unit.
    #[serde(default)]
    pub recycle_granularity: RecycleGranularity,
    /// 清理页进入时的推荐预选（P2b）。只是 UI 建议，用户仍可逐项取消——
    /// 不改变「先出清单 → 确认 → 执行」铁律；执行范围永远以用户勾选为准。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommended_selected: Option<bool>,
    /// 清理本 scope 前建议退出的进程名（如微信占用 msg 缓存）。cleanup_preflight
    /// 只读检查是否在跑，绝不在这里杀进程——杀进程是独立的写路径。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required_stopped_processes: Option<Vec<String>>,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RecycleGranularity {
    #[default]
    File,
    Directory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Recycle,
    Quarantine,
    Delete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Risk {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Prompt {
    None,
    Days {
        default: u32,
        #[serde(default)]
        label: Option<String>,
    },
    Bytes {
        default: u64,
        #[serde(default)]
        label: Option<String>,
    },
    Choice {
        default: String,
        options: Vec<String>,
        #[serde(default)]
        label: Option<String>,
    },
    Confirm {
        #[serde(default)]
        label: Option<String>,
    },
}

pub fn parse_toml(s: &str) -> anyhow::Result<Scaffold> {
    Ok(toml::from_str::<Scaffold>(s)?)
}

pub fn load_dir(dir: &Path) -> anyhow::Result<Vec<Scaffold>> {
    let mut out = Vec::new();
    if !dir.exists() {
        return Ok(out);
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = match entry {
            Ok(e) => e,
            // 单个目录项读不出来（瞬时 IO 错误/被删除）只跳过它，不中止整批——
            // 与下面 parse 失败跳过同语义（2026-10-11 修复：原 `entry?` 会让
            // 一个坏条目清空整张清理规则表）。
            Err(e) => {
                tracing::warn!("scaffold read_dir entry error in {:?}: {}", dir, e);
                continue;
            }
        };
        if entry
            .path()
            .extension()
            .map(|e| e == "toml")
            .unwrap_or(false)
        {
            // 单文件读失败（被 AV 锁住 / 非 UTF-8 编码）同样只跳过该文件：
            // 原 `read_to_string()?` 会让一个坏文件把整目录规则清空，清理页
            // 直接失去全部脚手架（2026-10-11 修复）。
            let text = match std::fs::read_to_string(entry.path()) {
                Ok(t) => t,
                Err(e) => {
                    tracing::warn!("scaffold read error in {:?}: {}", entry.path(), e);
                    continue;
                }
            };
            match toml::from_str::<Scaffold>(&text) {
                Ok(s) => out.push(s),
                Err(e) => tracing::warn!("scaffold parse error in {:?}: {}", entry.path(), e),
            }
        }
    }
    Ok(out)
}

pub fn detect_for(scaffolds: &[Scaffold], path: &Path) -> Option<String> {
    let path_norm = norm(&path.to_string_lossy()).to_lowercase();
    let basename = path
        .file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default();

    for s in scaffolds {
        for d in &s.detect {
            let pat = norm(&expand_env(d)).to_lowercase();
            if let Ok(set) = make_globset(&[pat.as_str()]) {
                if set.is_match(&path_norm) {
                    return Some(s.id.clone());
                }
            }
        }
        if !s.matcher.name_contains.is_empty()
            && s.matcher
                .name_contains
                .iter()
                .any(|n| basename.contains(&n.to_lowercase()))
            && s.matcher
                .must_have_child
                .iter()
                .all(|c| path.join(c).exists())
        {
            return Some(s.id.clone());
        }
    }
    None
}

/// Pre-compiled form of a `Scaffold` for hot-path matching. Holds the union of
/// all `detect` globs as a single `GlobSet`, plus lower-cased copies of the
/// fragment lists. Callers that need to detect against many paths (e.g. tag
/// every directory in a scan tree) should call `compile_all` once and then
/// `detect_compiled` per path — vs `detect_for`, which rebuilds globsets on
/// every call.
pub struct CompiledScaffold {
    pub id: String,
    detect_globs: globset::GlobSet,
    name_fragments_lc: Vec<String>,
    must_have_child: Vec<String>,
}

/// Compile a list of scaffolds to the matching-friendly form. Each `detect`
/// pattern is compiled individually and added to the union GlobSet only if
/// well-formed — matching `detect_for`'s "skip the bad one, keep the rest"
/// behavior. A single broken pattern must not disable the whole scaffold.
pub fn compile_all(scaffolds: &[Scaffold]) -> Vec<CompiledScaffold> {
    scaffolds
        .iter()
        .map(|s| {
            let mut builder = globset::GlobSetBuilder::new();
            let mut accepted: Vec<String> = Vec::with_capacity(s.detect.len());
            for d in &s.detect {
                let pat = norm(&expand_env(d)).to_lowercase();
                match globset::GlobBuilder::new(&pat)
                    .literal_separator(false)
                    .case_insensitive(true)
                    .build()
                {
                    Ok(g) => {
                        builder.add(g);
                        accepted.push(pat);
                    }
                    Err(e) => {
                        tracing::warn!(
                            "scaffold {}: skipping bad detect pattern {:?}: {}",
                            s.id,
                            d,
                            e
                        );
                    }
                }
            }
            let detect_globs = builder.build().unwrap_or_else(|_| {
                globset::GlobSetBuilder::new()
                    .build()
                    .expect("empty globset")
            });
            tracing::debug!(
                "scaffold {}: compiled detect={:?} name_contains_lc={:?} must_have_child={:?}",
                s.id,
                accepted,
                s.matcher
                    .name_contains
                    .iter()
                    .map(|n| n.to_lowercase())
                    .collect::<Vec<_>>(),
                s.matcher.must_have_child,
            );
            CompiledScaffold {
                id: s.id.clone(),
                detect_globs,
                name_fragments_lc: s
                    .matcher
                    .name_contains
                    .iter()
                    .map(|n| n.to_lowercase())
                    .collect(),
                must_have_child: s.matcher.must_have_child.clone(),
            }
        })
        .collect()
}

/// Same matching semantics as `detect_for`, but uses pre-compiled scaffolds.
/// Returns the first matching scaffold's id, or `None`.
///
/// `existing_children` 提供当前目录的**内存**子目录名时，`must_have_child`
/// 直接在内存里查（大小写不敏感），避免对每个命中目录都做 `path.join(child)
/// .exists()` 磁盘调用——扫描树 tag 阶段对每盘数十万目录逐个 exists 会拖慢
/// 2 倍以上（实测 C 盘 29.7 万目录 tag_ms 229s）。`None` 时保持原磁盘语义，
/// 供非扫描树场景（真实文件系统判断）与测试使用。
pub fn detect_compiled(
    compiled: &[CompiledScaffold],
    path: &Path,
    existing_children: Option<&[String]>,
) -> Option<String> {
    let path_norm_lc = norm(&path.to_string_lossy()).to_lowercase();
    let basename_lc = path
        .file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default();

    for c in compiled {
        if c.detect_globs.is_match(&path_norm_lc) {
            return Some(c.id.clone());
        }
        if !c.name_fragments_lc.is_empty()
            && c.name_fragments_lc.iter().any(|n| basename_lc.contains(n))
            && c.must_have_child.iter().all(|child| {
                existing_children
                    .map(|names| names.iter().any(|n| n.eq_ignore_ascii_case(child)))
                    .unwrap_or_else(|| path.join(child).exists())
            })
        {
            return Some(c.id.clone());
        }
    }
    None
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;

mod red_line;
pub use red_line::{
    canonicalize_for_red_line, directory_scope_recycle_violations, expand_env, glob_hits_red_line,
    red_line_samples, red_line_violations, scaffold_red_line_violations,
};
