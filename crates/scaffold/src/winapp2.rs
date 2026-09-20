//! Winapp2.ini 规则引擎（缝口 C）：把社区维护的 Winapp2 清理规则解析成
//! 本工程的 [`Scaffold`] 结构。
//!
//! 对齐 tubatools 的 `Winapp2Parser` + `CleanerEntry` / `FileKeyEntry` /
//! `RegKeyEntry` / `ExcludeKeyEntry`（MIT，源自 builtbybel/FluentCleaner）。
//!
//! ## 能力边界
//!
//! 本工程的执行层（executor）只做**文件/目录**操作，不碰注册表。因此：
//! - `FileKeyN`（文件清理）→ 完整转成 `Scope`（glob / recycle / cache 分组）
//! - `RegKeyN`（注册表清理）→ 只留在 [`Winapp2Entry`] 上，不生成可执行 Scope，
//!   转换结果会带 `registry_only` 标记，UI 可据此提示「仅注册表，当前版本不执行」
//! - `ExcludeKeyN`（排除）→ 解析保留，暂不参与 Scope glob 构造（scaffold 的
//!   Scope 模型没有排除面，属于已知限制）
//!
//! `Detect`（注册表探测）无法转成文件 glob，转换时只采纳 `DetectFile`；
//! 只有注册表探测的条目会退化为 `match.name_contains`（按名字匹配目录名）。

use crate::{Match, Mode, RecycleGranularity, Risk, Scaffold, Scope};
use std::collections::HashMap;

/// 一个 Winapp2 条目（对齐 C# CleanerEntry）。
#[derive(Debug, Clone)]
pub struct Winapp2Entry {
    /// 展示名（已剥掉行尾的 ` *` 社区标记）。
    pub name: String,
    /// 可选的自由格式分区名。
    pub section: Option<String>,
    /// 分类码（如 3025=Windows、3029=浏览器），面板分组用。
    pub lang_sec_ref: Option<i64>,
    /// 注册表探测路径（OR 语义），无法转成文件 glob。
    pub detect: Vec<String>,
    /// 文件/目录探测路径（OR 语义），可转成 `detect` glob。
    pub detect_files: Vec<String>,
    /// 预定义探测代号（DET_CHROME 等），未内建映射时忽略。
    pub special_detect: Option<String>,
    /// 默认是否勾选。
    pub default: bool,
    /// 警告文案（如「会删除已保存的密码」）。
    pub warning: Option<String>,
    /// 文件清理规则。
    pub file_keys: Vec<FileKey>,
    /// 注册表清理规则（当前版本不执行，仅保留信息）。
    pub reg_keys: Vec<RegKey>,
    /// 排除规则（当前版本仅保留信息）。
    pub exclude_keys: Vec<ExcludeKey>,
}

/// 一条 `FileKeyN=`（对齐 C# FileKeyEntry）。
/// 格式：`路径|模式[|RECURSE|REMOVESELF]`
#[derive(Debug, Clone)]
pub struct FileKey {
    /// 目录路径，可含 `%EnvVar%` 与通配符。
    pub path: String,
    /// 分号分隔的文件模式；缺省 `*.*`。
    pub pattern: String,
    pub flag: FileKeyFlag,
}

/// 目录扫描方式（对齐 C# FileKeyFlag）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKeyFlag {
    /// 只扫顶层文件。
    None,
    /// 递归扫全部子目录。
    Recurse,
    /// 同 Recurse，且清完空目录后顺手删掉空目录。
    RemoveSelf,
}

/// 一条 `RegKeyN=`（对齐 C# RegKeyEntry）。
/// 格式：`HIVE\SubKey[|ValueName]`；无 ValueName 时删除整个键树。
#[derive(Debug, Clone)]
pub struct RegKey {
    pub key_path: String,
    pub value_name: Option<String>,
}

/// 一条 `ExcludeKeyN=`（对齐 C# ExcludeKeyEntry）。
/// 格式：`FILE|PATH|REG|路径[|模式]`
#[derive(Debug, Clone)]
pub struct ExcludeKey {
    pub kind: ExcludeKind,
    pub path: String,
    pub pattern: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExcludeKind {
    File,
    Path,
    Reg,
}

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
fn parse_file_key(value: &str) -> FileKey {
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

fn parse_reg_key(value: &str) -> RegKey {
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

fn parse_exclude_key(value: &str) -> ExcludeKey {
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

// ── 转换：Winapp2Entry → Scaffold ────────────────────────────────────

/// 已知 SpecialDetect 代号 → 常见目录名片段（只做最保守的子串匹配）。
const SPECIAL_DETECT_DIRS: &[(&str, &[&str])] = &[
    ("DET_CHROME", &["Google/Chrome"]),
    ("DET_EDGE", &["Microsoft/Edge"]),
    ("DET_FIREFOX", &["Mozilla/Firefox"]),
    ("DET_THUNDERBIRD", &["Thunderbird"]),
    ("DET_OPERA", &["Opera Software"]),
    ("DET_VIVALDI", &["Vivaldi"]),
    ("DET_BRAVE", &["BraveSoftware"]),
    ("DET_ORBITAL", &["Orbital"]),
];

/// 把一个 Winapp2 条目转换成 Scaffold（只转 FileKey；RegKey 仅记录标记）。
///
/// 返回 None 表示该条目无法落地为文件清理（如只有注册表探测、或没有文件键）——
/// 这类条目对当前版本没有可执行价值，不生成空壳。
pub fn winapp2_to_scaffold(entry: &Winapp2Entry) -> Option<Scaffold> {
    if entry.file_keys.is_empty() {
        return None;
    }
    let id = slugify(&entry.name);
    // 探测面：DetectFile 转 glob（展开 %ENV%）；SpecialDetect 映射常见目录；
    // 只有注册表探测的条目靠 match.name_contains 兜底。
    let mut detect: Vec<String> = entry
        .detect_files
        .iter()
        .map(|d| expand_env_glob(d))
        .filter(|d| !d.is_empty())
        .collect();
    let mut name_contains: Vec<String> = Vec::new();
    if let Some(sd) = entry.special_detect.as_deref() {
        if let Some((_, dirs)) = SPECIAL_DETECT_DIRS
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(sd))
        {
            detect.extend(dirs.iter().map(|d| format!("**/{}", d)));
        }
    }
    // 没有文件探测时退化为目录名包含匹配（用去掉空格/特殊字符的工具名）。
    if detect.is_empty() {
        let frag = entry.name.trim().to_ascii_lowercase();
        if !frag.is_empty() {
            name_contains.push(frag);
        }
    }

    let mut scopes: Vec<Scope> = Vec::new();
    for (i, fk) in entry.file_keys.iter().enumerate() {
        let base = expand_env_glob(&fk.path);
        if base.is_empty() {
            continue;
        }
        let glob = file_key_glob(&base, fk);
        // 同路径同模式去重（同一目录多 FileKey 很常见）
        if scopes.iter().any(|s| s.glob == glob) {
            continue;
        }
        scopes.push(Scope {
            id: format!("s{}", i + 1),
            label: scope_label(fk, entry, i),
            glob,
            mode: Mode::Recycle,
            category: Some("cache".to_string()),
            variant: None,
            recycle_granularity: RecycleGranularity::File,
            prompt: None,
        });
    }
    if scopes.is_empty() {
        return None;
    }

    let mut disclaimer =
        String::from("来自 Winapp2.ini 的清理规则（CC-BY-SA-4.0），已转成回收站回收模式。");
    if !entry.reg_keys.is_empty() {
        disclaimer.push_str(" 该条目还含注册表清理项，当前版本不执行注册表操作，仅清理文件部分。");
    }
    if let Some(w) = entry.warning.as_deref() {
        disclaimer.push_str(&format!(" 注意：{w}"));
    }

    Some(Scaffold {
        id,
        name: entry.name.clone(),
        homepage: None,
        risk: Risk::Low,
        disclaimer,
        detect,
        matcher: Match {
            name_contains,
            must_have_child: Vec::new(),
        },
        scopes,
    })
}

/// 把 Winapp2 条目转成 Scaffold 列表（跳过无法落地的）。
pub fn winapp2_to_scaffolds(entries: &[Winapp2Entry]) -> Vec<Scaffold> {
    entries.iter().filter_map(winapp2_to_scaffold).collect()
}

/// id 化：小写 + 非字母数字转 `_`（连续分隔符折叠成一个），
/// 避免 TOML 里出现非法字符或超长 id。
fn slugify(name: &str) -> String {
    let mut raw = String::with_capacity(name.len());
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            raw.push(c.to_ascii_lowercase());
        } else if c.is_whitespace() || c == '-' || c == '_' || c == '+' || c == '.' {
            raw.push('_');
        }
        // 其余字符（& * 等）直接丢弃
    }
    let mut out = String::with_capacity(raw.len());
    let mut prev_underscore = false;
    for c in raw.chars() {
        if c == '_' {
            if prev_underscore {
                continue;
            }
            prev_underscore = true;
        } else {
            prev_underscore = false;
        }
        out.push(c);
    }
    let out = out.trim_matches('_').to_string();
    if out.is_empty() {
        "winapp2_entry".to_string()
    } else {
        out
    }
}

/// 展开 `%EnvVar%` 与 `$VAR`（winapp2 里的路径都用 `%X%`），
/// 并把反斜杠统一成正斜杠（与 detect_for 的 norm() 一致）。
/// 未知变量原样保留，不 panic。
fn expand_env_glob(s: &str) -> String {
    crate::expand_env(s).replace('\\', "/")
}

/// FileKey → Scaffold glob：目录基 + 模式，按 flag 决定是否前缀 `**/`。
/// 保留路径通配符（Chrome\*、User Data\* 这类 C# 语义），
/// 由 globset 的 literal_separator(false) 保证 `*` 可跨分隔符。
/// 反斜杠统一转正斜杠（globset 与 detect_for 的 norm() 一致）。
fn file_key_glob(base: &str, fk: &FileKey) -> String {
    let base = base.replace('\\', "/");
    let base = base.trim_end_matches('/');
    let pat = if fk.pattern.is_empty() || fk.pattern == "*.*" {
        "*".to_string()
    } else {
        fk.pattern
            .split(';')
            .map(|p| p.trim())
            .filter(|p| !p.is_empty())
            .collect::<Vec<_>>()
            .join(";")
    };
    let prefix = match fk.flag {
        FileKeyFlag::None => "",
        FileKeyFlag::Recurse | FileKeyFlag::RemoveSelf => "**/",
    };
    format!("{base}/{prefix}{pat}")
}

/// 从 FileKey 生成一个人类可读的 scope 标签。
fn scope_label(fk: &FileKey, entry: &Winapp2Entry, idx: usize) -> String {
    let dir = fk
        .path
        .rsplit(['/', '\\'])
        .find(|s| !s.is_empty())
        .unwrap_or(&fk.path);
    if entry.file_keys.len() == 1 {
        format!("清理 {} 文件", dir)
    } else {
        format!("清理 {} 文件（{}）", dir, idx + 1)
    }
}

/// 汇总一个 Winapp2 条目的注册表键（供 UI 展示「当前版本不执行」详情）。
pub fn reg_keys_summary(entries: &[Winapp2Entry]) -> HashMap<&str, usize> {
    entries
        .iter()
        .filter(|e| !e.reg_keys.is_empty())
        .map(|e| (e.name.as_str(), e.reg_keys.len()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
; Version: 260828
; # of entries: 4,075

[Google Chrome Autofill Data & Search Engine Preferences *]
LangSecRef=3029
DetectFile=%LocalAppData%\Google\Chrome*
Default=False
FileKey1=%LocalAppData%\Google\Chrome*\User Data\*|*Web Data
FileKey2=%LocalAppData%\Google\Chrome*\User Data\*\AutoFill*|*|REMOVESELF
FileKey3=%LocalAppData%\Google\Chrome*\User Data\AutoFill*|*|REMOVESELF

[Google Chrome Bookmark Backups *]
LangSecRef=3029
DetectFile=%LocalAppData%\Google\Chrome*
FileKey1=%LocalAppData%\Google\Chrome*\User Data\Bookmarks.bak|*|REMOVESELF

[Example App Registry Only Entry]
Detect=HKCU\Software\Foo
RegKey1=HKCU\Software\Foo|Value
"#;

    #[test]
    fn parses_entries_and_skips_header() {
        let entries = parse_winapp2(SAMPLE);
        // header 块跳过；仅注册表条目符合 IsValid（有探测 + 有可清理内容），解析期保留
        assert_eq!(entries.len(), 3, "2 个文件条目 + 1 个仅注册表条目");
        let chrome = entries
            .iter()
            .find(|e| e.name.contains("Chrome Autofill"))
            .unwrap();
        assert_eq!(chrome.lang_sec_ref, Some(3029));
        assert_eq!(chrome.detect_files.len(), 1);
        assert!(chrome.detect_files[0].contains("Google\\Chrome"));
        assert_eq!(chrome.file_keys.len(), 3);
        assert_eq!(chrome.file_keys[0].pattern, "*Web Data");
        assert_eq!(chrome.file_keys[1].flag, FileKeyFlag::RemoveSelf);
        assert!(!chrome.default);
    }

    #[test]
    fn file_key_parse_defaults_and_flags() {
        let fk = parse_file_key("C:\\Temp|*.log");
        assert_eq!(fk.path, "C:\\Temp");
        assert_eq!(fk.pattern, "*.log");
        assert_eq!(fk.flag, FileKeyFlag::None);

        let fk2 = parse_file_key("C:\\Temp|RECURSE");
        assert_eq!(fk2.pattern, "*.*");
        assert_eq!(fk2.flag, FileKeyFlag::Recurse);

        let fk3 = parse_file_key("C:\\Temp|*.tmp;*.log|REMOVESELF");
        assert_eq!(fk3.pattern, "*.tmp;*.log");
        assert_eq!(fk3.flag, FileKeyFlag::RemoveSelf);
    }

    #[test]
    fn reg_and_exclude_keys_parse() {
        let rk = parse_reg_key("HKCU\\Software\\Foo|Value");
        assert_eq!(rk.key_path, "HKCU\\Software\\Foo");
        assert_eq!(rk.value_name.as_deref(), Some("Value"));
        let rk2 = parse_reg_key("HKLM\\Software\\Bar");
        assert!(rk2.value_name.is_none());

        let ek = parse_exclude_key("FILE|%AppData%\\Firefox\\Profiles\\|places.sqlite");
        assert!(matches!(ek.kind, ExcludeKind::File));
        assert_eq!(ek.pattern.as_deref(), Some("places.sqlite"));
        let ek2 = parse_exclude_key("PATH|C:\\Cache");
        assert!(matches!(ek2.kind, ExcludeKind::Path));
    }

    #[test]
    fn converts_file_key_entries_to_scaffold() {
        let entries = parse_winapp2(SAMPLE);
        let scaffolds = winapp2_to_scaffolds(&entries);
        // 只有两个有文件键的条目可落地；仅注册表的条目被丢弃
        assert_eq!(scaffolds.len(), 2);
        let chrome = scaffolds
            .iter()
            .find(|s| s.name.contains("Chrome Autofill"))
            .unwrap();
        assert_eq!(chrome.risk, Risk::Low);
        assert_eq!(chrome.scopes.len(), 3);
        assert_eq!(chrome.scopes[0].mode, Mode::Recycle);
        assert_eq!(chrome.scopes[0].category.as_deref(), Some("cache"));
        // DetectFile → detect glob（%LocalAppData% 已展开成真实路径前缀）
        assert_eq!(chrome.detect.len(), 1);
        assert!(chrome.detect[0].ends_with("Google/Chrome*"));
        // REMOVESELF → glob 带 **/
        assert!(chrome.scopes[1].glob.contains("**/"));
        // 标签非空
        assert!(!chrome.scopes[0].label.is_empty());
    }

    #[test]
    fn slugify_sanitizes_names() {
        assert_eq!(
            slugify("Google Chrome Autofill Data & Search Engine Preferences *"),
            "google_chrome_autofill_data_search_engine_preferences"
        );
        assert_eq!(slugify("CPU-Z"), "cpu_z");
        assert_eq!(slugify("  !!!  "), "winapp2_entry");
    }

    #[test]
    fn special_detect_maps_to_dirs() {
        let mut e = Winapp2Entry {
            name: "Chrome".into(),
            section: None,
            lang_sec_ref: None,
            detect: Vec::new(),
            detect_files: Vec::new(),
            special_detect: Some("DET_CHROME".into()),
            default: true,
            warning: None,
            file_keys: vec![parse_file_key("C:\\Temp|*.log")],
            reg_keys: Vec::new(),
            exclude_keys: Vec::new(),
        };
        let s = winapp2_to_scaffold(&e).unwrap();
        assert!(s.detect.iter().any(|d| d == "**/Google/Chrome"));
        e.detect_files = Vec::new();
        e.special_detect = None;
        let s2 = winapp2_to_scaffold(&e).unwrap();
        assert_eq!(s2.matcher.name_contains, vec!["chrome"]);
    }

    #[test]
    fn reg_key_only_entries_are_dropped_or_marked() {
        let e = Winapp2Entry {
            name: "Registry Only".into(),
            section: None,
            lang_sec_ref: None,
            detect: vec!["HKCU\\Software\\Foo".into()],
            detect_files: Vec::new(),
            special_detect: None,
            default: true,
            warning: None,
            file_keys: Vec::new(),
            reg_keys: vec![RegKey {
                key_path: "HKCU\\Software\\Foo".into(),
                value_name: None,
            }],
            exclude_keys: Vec::new(),
        };
        // 无文件键 → 不落地
        assert!(winapp2_to_scaffold(&e).is_none());
        // 有文件键 + 注册表键 → 落地并标注注册表部分
        let mut e2 = e.clone();
        e2.file_keys = vec![parse_file_key("C:\\Temp|*.log")];
        let s = winapp2_to_scaffold(&e2).unwrap();
        assert!(s.disclaimer.contains("注册表"));
    }
}
