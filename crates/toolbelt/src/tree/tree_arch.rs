//! 启动文件与架构变体解析（从 tree.rs 拆分，2026-10-12）。
//! is_launchable / 架构后缀剥离 / 主启动文件判定 / 架构变体聚合。

use std::path::{Path, PathBuf};

use super::{preferred_arch_priority, ArchVariantOut, ARCH_SUFFIXES, LAUNCHABLE_EXTS};

/// 可双击启动判定（扩展名命中 LAUNCHABLE_EXTS，同 tubatools IsLaunchable）。
pub(crate) fn is_launchable(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| {
            LAUNCHABLE_EXTS
                .iter()
                .any(|x| e.eq_ignore_ascii_case(&x[1..]))
        })
        .unwrap_or(false)
}

/// 去掉文件/目录名尾部的架构后缀（同 C# StripArchSuffix）。
/// 注意：不能按字节切片——中文工具名是多字节 UTF-8，切片可能落在字符中间
/// panic。转小写后直接用 `ends_with` 比较（ASCII 后缀大小写不敏感）。
fn strip_arch_suffix(name: &str) -> String {
    let lower = name.to_ascii_lowercase();
    for suffix in ARCH_SUFFIXES {
        let sfx = suffix.to_ascii_lowercase();
        if lower.len() > sfx.len() && lower.ends_with(&sfx) {
            return name[..name.len() - suffix.len()].to_string();
        }
    }
    name.to_string()
}

/// 去架构后缀后再去掉残留的 `_`（如 `cpuz_x64` → `cpuz`，`WizTree64` → `WizTree`），
/// 用于与目录名/彼此比较：C# 里 `StripArchSuffix("cpuz_x64")` 得 `cpuz_`，与目录
/// `CPUZ` 比不相等，导致主文件判定/变体收集落空——这里把尾 `_` 也剥掉再比。
fn strip_arch_suffix_trim(name: &str) -> String {
    strip_arch_suffix(name).trim_end_matches('_').to_string()
}

/// 从文件名末尾识别架构（同 C# DetectArch + FormatArchDisplay）。
pub(crate) fn detect_arch(file_stem: &str) -> Option<&'static str> {
    let arm = ["ARM64", "_ARM64", "arm64", "_arm64"];
    let x64 = ["x64", "_x64", "64", "_64", "w64", "_Win64"];
    let x86 = ["x86", "_x86", "32", "_32", "w32", "_Win32"];
    for p in arm {
        if file_stem.ends_with(p) {
            return Some("ARM64");
        }
    }
    for p in x64 {
        if file_stem.ends_with(p) {
            return Some("x64");
        }
    }
    for p in x86 {
        if file_stem.ends_with(p) {
            return Some("x86");
        }
    }
    None
}

/// 已知多架构工具的文件名模式（跨目录关联，同 C# KnownMultiArchTools）。
const KNOWN_MULTI_ARCH: &[(&str, &[&str])] = &[
    (
        "cpuz",
        &[
            "cpuz_x64.exe",
            "cpuz_x32.exe",
            "cpuz_arm64.exe",
            "cpuz64.exe",
            "cpuz32.exe",
        ],
    ),
    (
        "hwinfo",
        &[
            "HWiNFO64.exe",
            "HWiNFO32.exe",
            "HWiNFO_ARM64.exe",
            "HWiNFO.exe",
        ],
    ),
    (
        "hwmonitor",
        &[
            "HWMonitor_x64.exe",
            "HWMonitor_x32.exe",
            "hwmonitor_arm64.exe",
            "HWMonitor.exe",
        ],
    ),
    (
        "dism++",
        &[
            "Dism++x64.exe",
            "Dism++x86.exe",
            "Dism++ARM64.exe",
            "Dism++.exe",
        ],
    ),
    (
        "coretemp",
        &["Core Temp x64.exe", "Core Temp x86.exe", "Core Temp.exe"],
    ),
    (
        "crystaldiskinfo",
        &[
            "DiskInfo64S.exe",
            "DiskInfo32S.exe",
            "DiskInfo.exe",
            "DiskInfo64.exe",
            "DiskInfo32.exe",
        ],
    ),
    (
        "bootice",
        &["BOOTICEx64.exe", "BOOTICEx86.exe", "BOOTICE.exe"],
    ),
    (
        "bluescreenview",
        &[
            "BlueScreenViewx64.exe",
            "BlueScreenViewx86.exe",
            "BlueScreenView.exe",
        ],
    ),
    ("aida64", &["aida64.exe", "aida64.exe.manifest"]),
    (
        "linx",
        &[
            "linpack_xeon64.exe",
            "linpack_xeon32.exe",
            "linpack_xeon.exe",
        ],
    ),
];

/// 收集一个工具目录内的全部可执行文件（递归；同 C# FindAllArchVariants 第一步）。
pub(crate) fn all_launchables(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for ent in rd.flatten() {
            let p = ent.path();
            if p.is_dir() {
                stack.push(p);
            } else if is_launchable(&p) {
                out.push(p);
            }
        }
    }
    out
}

/// 主启动文件判定（同 C# FindPrimaryLaunchable，去掉 launchTarget 元数据分支，
/// 保留：精确名 → 去架构后缀 → 任意兜底）。`all` 已由调用方收集（catalog_tree 去重 IO）。
pub(crate) fn find_primary_launchable_with(
    tool_dir: &Path,
    dir_name: &str,
    all: &[PathBuf],
) -> Option<PathBuf> {
    if all.is_empty() {
        return None;
    }
    if all.len() == 1 {
        return all.first().cloned();
    }

    let direct: Vec<PathBuf> = std::fs::read_dir(tool_dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| is_launchable(p))
                .collect()
        })
        .unwrap_or_default();

    let stem_eq = |p: &Path, n: &str| {
        p.file_stem()
            .and_then(|s| s.to_str())
            .map(|s| s.eq_ignore_ascii_case(n))
            .unwrap_or(false)
    };
    let stem_stripped_eq = |p: &Path, n: &str| {
        p.file_stem()
            .and_then(|s| s.to_str())
            .map(|s| strip_arch_suffix_trim(s).eq_ignore_ascii_case(&strip_arch_suffix_trim(n)))
            .unwrap_or(false)
    };

    // 顶层精确名
    if let Some(m) = direct.iter().find(|p| stem_eq(p, dir_name)) {
        return Some(m.clone());
    }
    // 顶层去架构后缀
    let arch_cand: Vec<PathBuf> = direct
        .iter()
        .filter(|p| stem_stripped_eq(p, dir_name))
        .cloned()
        .collect();
    if !arch_cand.is_empty() {
        return Some(pick_preferred_arch(&arch_cand));
    }
    // 递归精确名
    if let Some(m) = all.iter().find(|p| stem_eq(p, dir_name)) {
        return Some(m.clone());
    }
    // 递归去架构后缀
    let arch_cand: Vec<PathBuf> = all
        .iter()
        .filter(|p| stem_stripped_eq(p, dir_name))
        .cloned()
        .collect();
    if !arch_cand.is_empty() {
        return Some(pick_preferred_arch(&arch_cand));
    }
    // 兜底
    if !direct.is_empty() {
        return Some(direct[0].clone());
    }
    all.first().cloned()
}

/// 从候选里按宿主架构优先级挑一个（同 C# PickPreferredArch）。
fn pick_preferred_arch(candidates: &[PathBuf]) -> PathBuf {
    for arch in preferred_arch_priority() {
        let patterns: &[&str] = match *arch {
            "ARM64" => &["ARM64", "_ARM64", "arm64", "_arm64"],
            "x64" => &["x64", "_x64", "64", "_64", "w64", "_Win64"],
            _ => &["x86", "_x86", "32", "_32", "w32", "_Win32"],
        };
        if let Some(m) = candidates.iter().find(|p| {
            p.file_stem()
                .and_then(|s| s.to_str())
                .map(|s| patterns.iter().any(|pat| s.ends_with(pat)))
                .unwrap_or(false)
        }) {
            return m.clone();
        }
    }
    candidates[0].clone()
}

/// 同目录 + 跨目录（KNOWN_MULTI_ARCH）的架构变体收集（对齐 C# FindAllArchVariants）。
/// `all` 已由调用方收集（catalog_tree 去重 IO）。
pub(crate) fn find_arch_variants_with(
    tools_root: &Path,
    tool_dir: &Path,
    category_root: &Path,
    primary: &Path,
    dir_name: &str,
    all: &[PathBuf],
) -> Vec<ArchVariantOut> {
    let mut variants: Vec<ArchVariantOut> = Vec::new();
    let primary_ext = primary.extension().and_then(|e| e.to_str());

    // 1. 同目录内
    let dir_stripped = strip_arch_suffix_trim(dir_name);
    for f in all {
        if f == primary {
            continue;
        }
        if let Some(ext) = primary_ext {
            if !f
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.eq_ignore_ascii_case(ext))
                .unwrap_or(false)
            {
                continue;
            }
        }
        let Some(stem) = f.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let Some(arch) = detect_arch(stem) else {
            continue;
        };
        let stripped = strip_arch_suffix_trim(stem);
        if !stripped.eq_ignore_ascii_case(&dir_stripped) && !stripped.eq_ignore_ascii_case(dir_name)
        {
            continue;
        }
        let file_rel = normalize_rel(tools_root, f);
        if !variants.iter().any(|v| v.file_rel == file_rel) {
            variants.push(ArchVariantOut {
                name: clean_name(&stripped),
                file_rel,
                arch: arch.to_string(),
            });
        }
    }

    // 2. 跨目录（已知多架构工具映射）
    let primary_file = primary.file_name().and_then(|s| s.to_str()).unwrap_or("");
    for (_, patterns) in KNOWN_MULTI_ARCH {
        if !patterns
            .iter()
            .any(|p| p.eq_ignore_ascii_case(primary_file))
        {
            continue;
        }
        let Ok(rd) = std::fs::read_dir(category_root) else {
            break;
        };
        for ent in rd.flatten() {
            let other = ent.path();
            if other == *tool_dir || !other.is_dir() {
                continue;
            }
            let Some(other_name) = other.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            if !strip_arch_suffix_trim(other_name).eq_ignore_ascii_case(&dir_stripped) {
                continue;
            }
            for pattern in *patterns {
                let candidate = other.join(pattern);
                if candidate.is_file() && is_launchable(&candidate) {
                    let Some(stem) = candidate.file_stem().and_then(|s| s.to_str()) else {
                        continue;
                    };
                    let Some(arch) = detect_arch(stem) else {
                        continue;
                    };
                    let file_rel = normalize_rel(tools_root, &candidate);
                    if !variants.iter().any(|v| v.file_rel == file_rel) {
                        variants.push(ArchVariantOut {
                            name: clean_name(&strip_arch_suffix(stem)),
                            file_rel,
                            arch: arch.to_string(),
                        });
                    }
                }
            }
        }
        break;
    }
    variants
}

/// `_x64`/`_x86`/`_arm64` → 空格分隔的展示名（同 C# CleanupName 的前四步）。
fn clean_name(stem: &str) -> String {
    let s = stem
        .replace("_x64", " x64")
        .replace("_x86", " x86")
        .replace("_ARM64", " ARM64")
        .replace("_arm64", " ARM64")
        .replace('_', " ");
    s.trim().to_string()
}

/// 相对 Tools 根的正斜杠路径。
pub(crate) fn normalize_rel(tools_root: &Path, p: &Path) -> String {
    p.strip_prefix(tools_root)
        .unwrap_or(p)
        .to_string_lossy()
        .replace('\\', "/")
}

/// 合并仅架构后缀不同的目录（同 C# MergeArchDirectories：保第一个，跳过后续同名 stripped）。
pub(crate) fn merge_arch_dirs(dir_names: &[String]) -> Vec<String> {
    let mut consumed = vec![false; dir_names.len()];
    let mut out = Vec::new();
    for (i, n) in dir_names.iter().enumerate() {
        if consumed[i] {
            continue;
        }
        let stripped_i = strip_arch_suffix_trim(n);
        out.push(n.clone());
        for (j, n2) in dir_names.iter().enumerate().skip(i + 1) {
            if consumed[j] {
                continue;
            }
            if strip_arch_suffix_trim(n2).eq_ignore_ascii_case(&stripped_i) {
                consumed[j] = true;
            }
        }
    }
    out
}
