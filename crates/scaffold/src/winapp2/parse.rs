//! Winapp2.ini 解析域（从 winapp2.rs 拆分，2026-10-12）。
//! parse_winapp2 按 INI 段扫描：Detect*/FileKeyN/RegKeyN/ExcludeKeyN 分流到
//! 结构体字段；is_valid 对齐 C# IsValid（有探测 + 有可清理内容才收）；
//! classify_key/parse_file_key/parse_reg_key/parse_exclude_key 三种键格式解析。
//! 类型定义（Winapp2Entry/FileKey/...）在父模块 winapp2.rs。

use super::{ExcludeKey, ExcludeKind, FileKey, FileKeyFlag, RegKey, Winapp2Entry};
/// 解析 Winapp2.ini 全文，返回有效条目（有探测 + 有可清理内容，同 C# IsValid）。
///
/// 健壮性要求：单条坏行/坏块绝不能让整个解析失败——坏 key 忽略，坏块跳过。
pub fn parse_winapp2(content: &str) -> Vec<Winapp2Entry> {
    let mut entries: Vec<Winapp2Entry> = Vec::new();
    let mut current: Option<Winapp2Entry> = None;

    for raw_line in content.split(['\r', '\n']) {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            if let Some(e) = current.take() {
                if is_valid(&e) {
                    entries.push(e);
                }
            }
            let name = line[1..line.len() - 1].trim();
            // 跳过文件头块（[Winapp2] / [version]）——同 C# OrdinalIgnoreCase
            let lower = name.to_ascii_lowercase();
            if lower.starts_with("winapp2") || lower.starts_with("version") {
                continue;
            }
            current = Some(Winapp2Entry {
                name: name.trim_end_matches('*').trim().to_string(),
                section: None,
                lang_sec_ref: None,
                detect: Vec::new(),
                detect_files: Vec::new(),
                special_detect: None,
                default: true,
                warning: None,
                file_keys: Vec::new(),
                reg_keys: Vec::new(),
                exclude_keys: Vec::new(),
            });
            continue;
        }
        let Some(eq) = line.find('=') else { continue };
        let key = line[..eq].trim();
        let value = line[eq + 1..].trim();
        if value.is_empty() {
            continue;
        }
        let Some(entry) = current.as_mut() else {
            continue;
        };

        match classify_key(key) {
            KeyKind::LangSecRef => {
                if let Ok(n) = value.parse::<i64>() {
                    entry.lang_sec_ref = Some(n);
                }
            }
            KeyKind::Section => entry.section = Some(value.to_string()),
            KeyKind::SpecialDetect => entry.special_detect = Some(value.to_string()),
            KeyKind::Warning => entry.warning = Some(value.to_string()),
            KeyKind::Default => entry.default = value.eq_ignore_ascii_case("true"),
            KeyKind::Detect => entry.detect.push(value.to_string()),
            KeyKind::DetectFile => entry.detect_files.push(value.to_string()),
            KeyKind::FileKey => entry.file_keys.push(parse_file_key(value)),
            KeyKind::RegKey => entry.reg_keys.push(parse_reg_key(value)),
            KeyKind::ExcludeKey => entry.exclude_keys.push(parse_exclude_key(value)),
            KeyKind::Unknown => {}
        }
    }
    if let Some(e) = current.take() {
        if is_valid(&e) {
            entries.push(e);
        }
    }
    entries
}

/// 条目只有「能被探测」且「有东西可清」才有用（同 C# IsValid）。
fn is_valid(e: &Winapp2Entry) -> bool {
    (!e.detect.is_empty() || !e.detect_files.is_empty() || e.special_detect.is_some())
        && (!e.file_keys.is_empty() || !e.reg_keys.is_empty())
}

enum KeyKind {
    LangSecRef,
    Section,
    SpecialDetect,
    Warning,
    Default,
    Detect,
    DetectFile,
    FileKey,
    RegKey,
    ExcludeKey,
    Unknown,
}

fn classify_key(key: &str) -> KeyKind {
    let k = key.to_ascii_lowercase();
    match k.as_str() {
        "langsecref" => return KeyKind::LangSecRef,
        "section" => return KeyKind::Section,
        "specialdetect" => return KeyKind::SpecialDetect,
        "warning" => return KeyKind::Warning,
        "default" => return KeyKind::Default,
        _ => {}
    }
    if let Some(rest) = k.strip_prefix("detectfile") {
        return if rest.is_empty() || rest.chars().all(|c| c.is_ascii_digit()) {
            KeyKind::DetectFile
        } else {
            KeyKind::Unknown
        };
    }
    if let Some(rest) = k.strip_prefix("detect") {
        return if rest.is_empty() || rest.chars().all(|c| c.is_ascii_digit()) {
            KeyKind::Detect
        } else {
            KeyKind::Unknown
        };
    }
    if let Some(rest) = k.strip_prefix("filekey") {
        return if rest.chars().all(|c| c.is_ascii_digit()) && !rest.is_empty() {
            KeyKind::FileKey
        } else {
            KeyKind::Unknown
        };
    }
    if let Some(rest) = k.strip_prefix("regkey") {
        return if rest.chars().all(|c| c.is_ascii_digit()) && !rest.is_empty() {
            KeyKind::RegKey
        } else {
            KeyKind::Unknown
        };
    }
    if let Some(rest) = k.strip_prefix("excludekey") {
        return if rest.chars().all(|c| c.is_ascii_digit()) && !rest.is_empty() {
            KeyKind::ExcludeKey
        } else {
            KeyKind::Unknown
        };
    }
    KeyKind::Unknown
}

/// `path|pattern[|flag]`——第二个字段可能是模式也可能是裸 flag（同 C#）。
pub(crate) fn parse_file_key(value: &str) -> FileKey {
    let parts: Vec<&str> = value.split('|').map(str::trim).collect();
    let mut fk = FileKey {
        path: parts.first().copied().unwrap_or("").to_string(),
        pattern: "*.*".to_string(),
        flag: FileKeyFlag::None,
    };
    if parts.len() == 2 {
        if parts[1].eq_ignore_ascii_case("recurse") {
            fk.flag = FileKeyFlag::Recurse;
        } else if parts[1].eq_ignore_ascii_case("removeself") {
            fk.flag = FileKeyFlag::RemoveSelf;
        } else {
            fk.pattern = parts[1].to_string();
        }
    } else if parts.len() > 2 {
        fk.pattern = parts[1].to_string();
        fk.flag = match parts[2].to_ascii_uppercase().as_str() {
            "RECURSE" => FileKeyFlag::Recurse,
            "REMOVESELF" => FileKeyFlag::RemoveSelf,
            _ => FileKeyFlag::None,
        };
    }
    fk
}

pub(crate) fn parse_reg_key(value: &str) -> RegKey {
    match value.split_once('|') {
        Some((k, v)) => RegKey {
            key_path: k.trim().to_string(),
            value_name: Some(v.trim().to_string()),
        },
        None => RegKey {
            key_path: value.trim().to_string(),
            value_name: None,
        },
    }
}

pub(crate) fn parse_exclude_key(value: &str) -> ExcludeKey {
    let parts: Vec<&str> = value.split('|').map(str::trim).collect();
    let kind = match parts.first().map(|s| s.to_ascii_uppercase()).as_deref() {
        Some("PATH") => ExcludeKind::Path,
        Some("REG") => ExcludeKind::Reg,
        _ => ExcludeKind::File,
    };
    ExcludeKey {
        kind,
        path: parts.get(1).copied().unwrap_or("").to_string(),
        pattern: parts.get(2).map(|s| s.to_string()),
    }
}
