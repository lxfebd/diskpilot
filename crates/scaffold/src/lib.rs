//! Loads scaffold TOML manifests and matches them against folders.

pub mod winapp2;

use serde::{Deserialize, Serialize};
use std::path::Path;

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
        let entry = entry?;
        if entry
            .path()
            .extension()
            .map(|e| e == "toml")
            .unwrap_or(false)
        {
            let text = std::fs::read_to_string(entry.path())?;
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

fn norm(s: &str) -> String {
    s.replace('\\', "/")
}

/// Expand environment variables in a path/glob string, supporting both
/// `$VAR` / `${VAR}` (Unix) and `%VAR%` (Windows) syntax. Used by the scanner
/// for `detect` patterns and by callers that want to evaluate scope `glob`s.
pub fn expand_env(s: &str) -> String {
    let unix = shellexpand::env(s)
        .map(|c| c.into_owned())
        .unwrap_or_else(|_| s.to_string());
    expand_winpct(&unix)
}

fn expand_winpct(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        let mut name = String::new();
        let mut closed = false;
        while let Some(&nc) = chars.peek() {
            chars.next();
            if nc == '%' {
                closed = true;
                break;
            }
            name.push(nc);
        }
        if closed {
            match std::env::var(&name) {
                Ok(v) => out.push_str(&v),
                Err(_) => {
                    out.push('%');
                    out.push_str(&name);
                    out.push('%');
                }
            }
        } else {
            out.push('%');
            out.push_str(&name);
        }
    }
    out
}

/// 红线校验专用的**锚定环境**（与 `red_line_samples()` 的 `C:/Users/test` 同构）。
///
/// 红线是「拿 glob 反查固定样本」的语法检验，必须用写死的锚，绝不能读真实机器
/// 的 env：开发者机上 `%USERPROFILE%` 展开成 `C:/Users/<真名>`，对样本
/// `C:/Users/test/**` 零命中，宽到覆盖整个 home 的 scope 反而躲过红线。
fn red_line_anchor(var: &str) -> Option<&'static str> {
    let v = var.to_ascii_uppercase();
    Some(match v.as_str() {
        "USERPROFILE" | "HOME" => "C:/Users/test",
        "APPDATA" => "C:/Users/test/AppData/Roaming",
        "LOCALAPPDATA" => "C:/Users/test/AppData/Local",
        "TEMP" | "TMP" => "C:/Users/test/AppData/Local/Temp",
        "PROGRAMDATA" => "C:/ProgramData",
        "SYSTEMROOT" | "WINDIR" => "C:/Windows",
        "PROGRAMFILES" => "C:/Program Files",
        "PROGRAMFILES(X86)" | "PROGRAMFILES(WOW6432NODE)" => "C:/Program Files (x86)",
        "ONEDRIVE" | "ONEDRIVECONSUMER" => "C:/Users/test/OneDrive",
        _ => return None,
    })
}

/// 把 glob 规范化到红线锚定空间：`\`→`/`、`~` 前缀与 `%VAR%` / `$VAR` /
/// `${VAR}` 一律换成 [`red_line_anchor`] 的固定值。未知变量保持原样。
pub fn canonicalize_for_red_line(glob: &str) -> String {
    let s = norm(glob);
    let b: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0usize;
    if s == "~" || s == "~/" {
        return red_line_anchor("HOME").unwrap().to_string();
    }
    if s.starts_with("~/") {
        out.push_str(red_line_anchor("HOME").unwrap());
        out.push('/');
        i = 2;
    }
    while i < b.len() {
        let c = b[i];
        // %VAR%
        if c == '%' {
            if let Some(off) = b[i + 1..].iter().position(|&x| x == '%') {
                let name: String = b[i + 1..i + 1 + off].iter().collect();
                if let Some(anchor) = red_line_anchor(&name) {
                    out.push_str(anchor);
                    i += off + 2;
                    continue;
                }
            }
        }
        // $VAR / ${VAR}
        if c == '$' {
            let braced = b.get(i + 1) == Some(&'{');
            let start = i + if braced { 2 } else { 1 };
            let mut j = start;
            while j < b.len()
                && (b[j].is_ascii_alphanumeric() || b[j] == '_' || (braced && b[j] != '}'))
            {
                j += 1;
            }
            let closed = !braced || b.get(j) == Some(&'}');
            if j > start && closed {
                let name: String = b[start..j].iter().collect();
                if let Some(anchor) = red_line_anchor(&name) {
                    out.push_str(anchor);
                    i = j + if braced { 1 } else { 0 };
                    continue;
                }
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

fn make_globset(patterns: &[&str]) -> anyhow::Result<globset::GlobSet> {
    let mut b = globset::GlobSetBuilder::new();
    for p in patterns {
        let g = globset::GlobBuilder::new(p)
            .literal_separator(false)
            .case_insensitive(true)
            .build()?;
        b.add(g);
    }
    Ok(b.build()?)
}

/// 红线路径样本（CLAUDE.md Hard rule #1）。`scaffold-lint` 与 `scaffold-gen`
/// 共用同一份清单，避免两份红线漂移。所有 scope glob 都必须零命中。
pub fn red_line_samples() -> &'static [&'static str] {
    &[
        // *.db / *.db-wal / *.db-shm — conversation & account databases
        "C:/Users/test/AppData/Roaming/Company/App/data.db",
        "C:/Users/test/AppData/Roaming/Company/App/data.db-wal",
        "C:/Users/test/AppData/Roaming/Company/App/data.db-shm",
        // **/db_storage/** — WeChat 4.x DB cluster
        "C:/Users/test/AppData/Roaming/Tencent/WeChat/xwechat_files/wxid_abc/db_storage/MMKV/mmkv.db",
        // **/Msg/** & **/MultiMsg/** — WeChat 3.x chat data
        "C:/Users/test/Documents/WeChat Files/wxid_abc/Msg/FileStorage/file.dat",
        "C:/Users/test/Documents/WeChat Files/wxid_abc/MultiMsg/msg.dat",
        // **/Accounts/** — account state
        "C:/Users/test/AppData/Roaming/Tencent/QQ/Accounts/12345/config.dat",
        // QQ NT 架构（9.x）本地状态 / 缓存外内容
        "C:/Users/test/AppData/Roaming/Tencent/QQ/nt_data/QQ/9.x/QtWebView/cookies.dat",
        "C:/Users/test/AppData/Roaming/Tencent/QQ/nt_data/QQ/9.x/global/config.dat",
        // **/All Users/** — machine-wide state
        "C:/Users/test/AppData/Roaming/Company/All Users/config.dat",
        // **/login/** — auth state
        "C:/Users/test/AppData/Roaming/Company/login/auth.dat",
        // **/config/** — account & app state
        "C:/Users/test/AppData/Roaming/Company/config/settings.ini",
        // **/Favorite*/** & **/Fav/** — user favorites
        "C:/Users/test/AppData/Roaming/Company/Favorite/1/file.dat",
        "C:/Users/test/AppData/Roaming/Company/Fav/item.dat",
        // **/key/** & **/crypto/** — encryption material
        "C:/Users/test/AppData/Roaming/Company/key/secret.key",
        "C:/Users/test/AppData/Roaming/Company/crypto/secret.bin",
        // 浏览器登录数据 / 会话 / 书签（文件名不是 .db 也可能存密码）
        "C:/Users/test/AppData/Local/Google/Chrome/User Data/Default/Login Data",
        "C:/Users/test/AppData/Local/Google/Chrome/User Data/Default/Cookies",
        "C:/Users/test/AppData/Local/Google/Chrome/User Data/Default/Bookmarks",
        "C:/Users/test/AppData/Roaming/Mozilla/Firefox/Profiles/abc.default/cookies.sqlite",
        "C:/Users/test/AppData/Roaming/Mozilla/Firefox/Profiles/abc.default/places.sqlite",
        "C:/Users/test/AppData/Roaming/Mozilla/Firefox/Profiles/abc.default/logins.json",
        // SSH / 云凭据 / 私有密钥文件
        "C:/Users/test/.ssh/id_rsa",
        "C:/Users/test/.ssh/id_ed25519",
        "C:/Users/test/.aws/credentials",
        "C:/Users/test/.config/gcloud/credentials.db",
        // Windows 系统关键文件 / 回收站 / 系统保护
        "C:/pagefile.sys",
        "C:/hiberfil.sys",
        "C:/$Recycle.Bin/S-1-5-21-1/file.dat",
        "C:/System Volume Information/catalog.dat",
        // 虚拟磁盘镜像（Docker/WSL 数据盘）
        "C:/Users/test/AppData/Local/Docker/wsl/docker-desktop-data/data/ext4.vhdx",
        "C:/Users/test/AppData/Local/Packages/ubuntu/vhdx",
    ]
}

/// 检查一个**已规范化**的 scope glob 是否命中任何红线路径。
///
/// 调用方几乎总是该用 [`red_line_violations`]（它替你做了
/// [`canonicalize_for_red_line`]）。本函数只做一次反查，入参里若还留着
/// `%USERPROFILE%` 这类未展开变量，将命中不到样本。
pub fn glob_hits_red_line(glob: &str) -> Vec<String> {
    let g = match make_globset(&[glob]) {
        Ok(v) => v,
        Err(_) => return vec![],
    };
    red_line_samples()
        .iter()
        .filter(|&&sample| g.is_match(sample))
        .map(|s| s.to_string())
        .collect()
}

/// 红线校验的**唯一入口**：先规范化到锚定空间，再反查样本。CI lint、运行时
/// 安装、装载复检三条路径都走这里，保证「校验口径 == 执行口径」（执行期是
/// `expand_env(glob).replace('\\', "/")`，见 desktop `executor.rs`）。
pub fn red_line_violations(glob: &str) -> Vec<String> {
    glob_hits_red_line(&canonicalize_for_red_line(glob))
}

/// 整份脚本的红线审计：返回 `(scope id, 命中样本)`。非空即必须拒绝安装/装载。
pub fn scaffold_red_line_violations(sc: &Scaffold) -> Vec<(String, Vec<String>)> {
    sc.scopes
        .iter()
        .filter_map(|s| {
            let hits = red_line_violations(&s.glob);
            (!hits.is_empty()).then(|| (s.id.clone(), hits))
        })
        .collect()
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
mod tests {
    use super::*;

    #[test]
    fn red_line_samples_are_all_hit_by_catchall() {
        // `**/*` 命中一切；它必须把每条红线样本都召回来，保证清单本身可验证。
        let hits = glob_hits_red_line("**/*");
        assert_eq!(
            hits.len(),
            red_line_samples().len(),
            "catchall must recall every red-line sample, got {hits:?}"
        );
    }

    #[test]
    fn red_line_hits_on_specific_families() {
        // 单条红线族必须被各自命中。
        assert!(!glob_hits_red_line("**/Msg/**").is_empty(), "Msg");
        assert!(
            !glob_hits_red_line("**/db_storage/**").is_empty(),
            "db_storage"
        );
        assert!(!glob_hits_red_line("**/config/**").is_empty(), "config");
        assert!(
            !glob_hits_red_line("**/Favorite*/**").is_empty(),
            "Favorite"
        );
        assert!(!glob_hits_red_line("**/key/**").is_empty(), "key");
    }

    #[test]
    fn red_line_clean_globs_have_no_hits() {
        // 正常缓存类 glob 不允许误伤红线样本。
        for glob in [
            "**/Cache/**",
            "**/logs/**",
            "**/temp/**",
            "**/Code/Cache/**",
            "**/node_modules/.cache/**",
        ] {
            assert!(
                glob_hits_red_line(glob).is_empty(),
                "`{glob}` must not hit red lines"
            );
        }
    }

    #[test]
    fn red_line_check_is_machine_env_independent() {
        // 关键回归：红线用写死的锚，不读本机 env。开发者机上 USERPROFILE 是
        // C:/Users/<真名>，若按真实展开，`%USERPROFILE%/**` 这种覆盖整个 home
        // 的 delete 级 scope 会零命中从而溜过校验。
        for wide in [
            "%USERPROFILE%/**",
            "$HOME/**",
            "${HOME}/**",
            "~/**",
            "%USERPROFILE%\\.ssh\\**",
            "%APPDATA%/**",
            "%LOCALAPPDATA%/Packages/**",
        ] {
            assert!(
                !red_line_violations(wide).is_empty(),
                "宽 scope `{wide}` 必须命中红线"
            );
        }
    }

    #[test]
    fn canonicalize_for_red_line_forms() {
        assert_eq!(canonicalize_for_red_line("%USERPROFILE%"), "C:/Users/test");
        assert_eq!(
            canonicalize_for_red_line("%LOCALAPPDATA%\\Google\\Chrome\\**"),
            "C:/Users/test/AppData/Local/Google/Chrome/**"
        );
        assert_eq!(
            canonicalize_for_red_line("~/Downloads/**"),
            "C:/Users/test/Downloads/**"
        );
        assert_eq!(canonicalize_for_red_line("$HOME/**"), "C:/Users/test/**");
        assert_eq!(canonicalize_for_red_line("${HOME}/**"), "C:/Users/test/**");
        // 未知变量原样保留（不猜值，避免误判成命中）。
        assert_eq!(
            canonicalize_for_red_line("%SOME_APP_HOME%/Cache/**"),
            "%SOME_APP_HOME%/Cache/**"
        );
    }

    #[test]
    fn red_line_clean_env_anchored_cache_globs() {
        // 真实 scaffold 的缓存类 glob 规范化后不得误伤红线。
        for glob in [
            "%TEMP%/**",
            r#"{"%LOCALAPPDATA%/Google/Chrome/User Data,${HOME}/.config/google-chrome}/**/{Cache,Code Cache,GPUCache,GrShaderCache,DawnGraphiteCache,DawnWebGPUCache,ShaderCache}"#,
            "%LOCALAPPDATA%/pip/Cache/**",
            "%APPDATA%/Code/CachedData/**",
        ] {
            assert!(
                red_line_violations(glob).is_empty(),
                "缓存 glob `{glob}` 不该命中红线，got {:?}",
                red_line_violations(glob)
            );
        }
    }

    #[test]
    fn scaffold_red_line_violations_reports_offending_scope_only() {
        let toml = r#"
id = "demo"
name = "Demo"
risk = "low"
disclaimer = "test"
detect = ["**/Demo"]
[[scope]]
id = "safe-cache"
label = "Safe"
glob = "%LOCALAPPDATA%/Demo/Cache/**"
mode = "recycle"
[[scope]]
id = "wide-home"
label = "Wide"
glob = "%USERPROFILE%/**"
mode = "delete"
"#;
        let sc: Scaffold = toml::from_str(toml).unwrap();
        let v = scaffold_red_line_violations(&sc);
        assert_eq!(v.len(), 1, "只应报出越界的那一条 scope，got {v:?}");
        assert_eq!(v[0].0, "wide-home");
        assert!(!v[0].1.is_empty());
    }

    #[test]
    fn parses_minimal_scaffold() {
        let toml = r#"
id = "demo"
name = "Demo"
risk = "low"
disclaimer = "test"
detect = ["**/Demo"]
[[scope]]
id = "s1"
label = "S1"
glob = "**/*"
mode = "recycle"
"#;
        let s: Scaffold = toml::from_str(toml).unwrap();
        assert_eq!(s.id, "demo");
        assert_eq!(s.scopes.len(), 1);
    }

    #[test]
    fn detect_compiled_matches_simple_recursive_glob() {
        // Reproduce the wechat-pc detect surface as minimally as possible.
        let toml = r#"
id = "wechat-pc"
name = "WeChat (PC)"
risk = "low"
disclaimer = "test"
detect = [
  "**/xwechat_files",
  "**/WeChat Files",
]
[match]
name_contains = ["xwechat_files", "WeChat Files", "xwechat", "WeChat"]
"#;
        let s: Scaffold = toml::from_str(toml).unwrap();
        let compiled = compile_all(&[s]);
        let path = Path::new("C:/Users/lvjin/Documents/xwechat_files");
        assert_eq!(
            detect_compiled(&compiled, path, None),
            Some("wechat-pc".to_string()),
            "detect_compiled missed `**/xwechat_files` against {:?}",
            path,
        );
    }

    #[test]
    fn detect_compiled_must_isolate_bad_patterns_like_detect_for() {
        // If any single detect pattern fails to compile, compile_all should
        // not nuke the whole scaffold's detect surface — that would diverge
        // from detect_for, which silently skips each bad pattern individually.
        // This guards against regressions where one quirky pattern (e.g. a
        // shell-expansion result containing `[` or `{` in a real user env)
        // poisons every other pattern in the same scaffold.
        let toml = r#"
id = "wechat-pc"
name = "WeChat (PC)"
risk = "low"
disclaimer = "test"
detect = [
  "[invalid-glob",
  "**/xwechat_files",
]
[match]
name_contains = []
"#;
        let s: Scaffold = toml::from_str(toml).unwrap();
        let scaffolds = vec![s];
        let compiled = compile_all(&scaffolds);
        let path = Path::new("C:/Users/lvjin/Documents/xwechat_files");
        assert_eq!(
            detect_compiled(&compiled, path, None),
            detect_for(&scaffolds, path),
            "compile_all dropped good pattern when sibling pattern is bad",
        );
        assert_eq!(
            detect_compiled(&compiled, path, None).as_deref(),
            Some("wechat-pc"),
        );
    }

    #[test]
    fn detect_compiled_matches_actual_wechat_toml() {
        // Load the real scaffolds/wechat-pc.toml (same path as production
        // load_dir) and confirm detect_compiled tags a typical 4.x data dir.
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scaffolds/wechat-pc.toml");
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read {:?} failed: {}", path, e));
        let s: Scaffold = toml::from_str(&text).expect("wechat-pc.toml parse");
        eprintln!(
            "wechat-pc loaded: detect={:?}, name_contains={:?}, must_have_child={:?}",
            s.detect, s.matcher.name_contains, s.matcher.must_have_child,
        );
        let compiled = compile_all(std::slice::from_ref(&s));

        for p in [
            "C:/Users/lvjin/Documents/xwechat_files",
            "C:\\Users\\lvjin\\Documents\\xwechat_files",
            "/some/path/xwechat_files",
        ] {
            let path = Path::new(p);
            let got = detect_compiled(&compiled, path, None);
            let oracle = detect_for(std::slice::from_ref(&s), path);
            eprintln!("path={:?} compiled={:?} oracle={:?}", p, got, oracle);
            assert_eq!(got, oracle, "divergence at {:?}", p);
            assert_eq!(got.as_deref(), Some("wechat-pc"), "missed at {:?}", p);
        }
    }

    #[test]
    fn detect_compiled_equivalent_to_detect_for_on_wechat_pc() {
        // Full wechat-pc detect/match block, including %VAR% expansion
        // patterns, to verify compile_all isn't silently dropping any pattern.
        let toml = r#"
id = "wechat-pc"
name = "WeChat (PC)"
risk = "low"
disclaimer = "test"
detect = [
  "%USERPROFILE%/Documents/xwechat_files",
  "%USERPROFILE%/Documents/WeChat Files",
  "%APPDATA%/Tencent/xwechat",
  "%APPDATA%/Tencent/WeChat",
  "**/xwechat_files",
  "**/WeChat Files",
]
[match]
name_contains = ["xwechat_files", "WeChat Files", "xwechat", "WeChat"]
"#;
        let s: Scaffold = toml::from_str(toml).unwrap();
        let scaffolds = vec![s];
        let compiled = compile_all(&scaffolds);

        for path_str in [
            "C:/Users/lvjin/Documents/xwechat_files",
            "C:\\Users\\lvjin\\Documents\\xwechat_files",
            "/home/foo/Documents/xwechat_files",
            "C:/Users/lvjin/Documents/WeChat Files",
        ] {
            let path = Path::new(path_str);
            assert_eq!(
                detect_compiled(&compiled, path, None),
                detect_for(&scaffolds, path),
                "compile_all vs detect_for diverged on {:?}",
                path_str,
            );
        }
    }
}
