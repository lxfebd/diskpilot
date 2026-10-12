//! winapp2.rs 单元测试（从 winapp2.rs 拆分，2026-10-12）。
//! Winapp2.ini 解析 / FileKey+RegKey+ExcludeKey / 转 Scaffold / slugify。

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
    // DetectFile → detect glob（只规范化不展开：红线 canonicalize 需要
    // 原始 %VAR% 形态才能还原到锚定空间，展开成真实路径会绕过红线）
    assert_eq!(chrome.detect.len(), 1);
    assert!(chrome.detect[0].ends_with("Google/Chrome*"));
    assert!(
        chrome.detect[0].contains("%LocalAppData%"),
        "变量必须保留未展开形态：{}",
        chrome.detect[0]
    );
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
