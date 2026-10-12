//! 红线（red-line）校验域（从 lib.rs 拆分，2026-10-12）。
//! 拿 glob 反查固定敏感路径样本（CLAUDE.md Hard rule #1）的口径统一在这里：
//! red_line_anchor 锚定环境 / canonicalize_for_red_line 规范化 / red_line_samples
//! 样本清单 / glob_hits_red_line + red_line_violations 反查 / 目录粒度陷阱审计。
//! norm / expand_env / make_globset 也被 detect_for / compile_all 共用，一并迁入。

use crate::{Mode, RecycleGranularity, Scaffold};

pub(crate) fn norm(s: &str) -> String {
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
    // 注意边界：所有样本都锚定在 `C:/Users/test/...`（及 pagefile.sys 等盘根系统文件）
    // 之下，因此 `C:/Users/**` 这类覆盖整个 home 的宽 glob 会命中样本并被拦，
    // 而盘根 `C:/` / `C:`（不带 `**`，字面字串）反而不命中任何样本——globset
    // 的 `C:` 不会匹配 `C:/Users/...` 这种全路径字面。真正该拦的盘级宽扫描
    // 形态是 `C:/**`（见 red_line_check_is_machine_env_independent 外的边界测试）。
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

pub(crate) fn make_globset(patterns: &[&str]) -> anyhow::Result<globset::GlobSet> {
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
        // Chromium 表单自动填充库（winapp2 社区规则 `*Web Data` 只认文件名后缀，
        // 补样本把该绕过封死：Form Data 同样是存地址/卡号的 SQLite）
        "C:/Users/test/AppData/Local/Google/Chrome/User Data/Default/Web Data",
        "C:/Users/test/AppData/Local/Google/Chrome/User Data/Default/Form Data",
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

/// 目录粒度 scope 的 mode 会被执行层静默忽略（一律走 Recycle，见
/// `desktop/executor.rs` "Directory granularity is locked to Recycle"）——
/// TOML 里写 quarantine/delete 是不生效的陷阱。返回需要 lint FAIL 的 scope。
pub fn directory_scope_recycle_violations(sc: &Scaffold) -> Vec<(String, Mode)> {
    sc.scopes
        .iter()
        .filter(|s| {
            s.recycle_granularity == RecycleGranularity::Directory && s.mode != Mode::Recycle
        })
        .map(|s| (s.id.clone(), s.mode))
        .collect()
}
