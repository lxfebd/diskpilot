//! Valve KeyValues (.acf / .vdf) parser + appmanifest/libraryfolders extraction.

use anyhow::{anyhow, Context, Result};
use std::path::PathBuf;
// ---------------------------------------------------------------------------
// KeyValues parser (shared by .acf and .vdf)
// ---------------------------------------------------------------------------

/// A node in a Valve KeyValues tree. Leaves are strings; inner nodes are
/// ordered lists of `(key, value)` pairs (Valve KV preserves order and allows
/// duplicate keys, though we don't currently exercise the latter).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KvValue {
    Str(String),
    Block(Vec<(String, KvValue)>),
}

impl KvValue {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            KvValue::Str(s) => Some(s),
            KvValue::Block(_) => None,
        }
    }

    pub fn as_block(&self) -> Option<&[(String, KvValue)]> {
        match self {
            KvValue::Block(b) => Some(b),
            KvValue::Str(_) => None,
        }
    }
}

/// Find the first `(key, value)` pair whose key matches case-insensitively.
pub(crate) fn find_ci<'a>(entries: &'a [(String, KvValue)], key: &str) -> Option<&'a KvValue> {
    entries
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(key))
        .map(|(_, v)| v)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Token {
    Quoted(String),
    Open,
    Close,
}

pub(crate) fn tokenize(text: &str) -> Result<Vec<Token>> {
    let mut tokens = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(&c) = chars.peek() {
        match c {
            ' ' | '\t' | '\r' | '\n' => {
                chars.next();
            }
            '/' => {
                chars.next();
                match chars.peek() {
                    Some(&'/') => {
                        // Line comment — skip to end of line.
                        while let Some(&nc) = chars.peek() {
                            chars.next();
                            if nc == '\n' {
                                break;
                            }
                        }
                    }
                    _ => return Err(anyhow!("stray '/' at top level (line comments only)")),
                }
            }
            '{' => {
                chars.next();
                tokens.push(Token::Open);
            }
            '}' => {
                chars.next();
                tokens.push(Token::Close);
            }
            '"' => {
                chars.next();
                let mut s = String::new();
                loop {
                    match chars.next() {
                        None => return Err(anyhow!("unterminated quoted string")),
                        Some('"') => break,
                        Some('\\') => match chars.next() {
                            Some('n') => s.push('\n'),
                            Some('t') => s.push('\t'),
                            Some('\\') => s.push('\\'),
                            Some('"') => s.push('"'),
                            // Unknown escape: keep both chars verbatim — Valve
                            // is loose about what's escaped, especially in
                            // installdir paths like "Counter-Strike\Game" (no,
                            // they actually use forward slash, but be safe).
                            Some(other) => {
                                s.push('\\');
                                s.push(other);
                            }
                            None => return Err(anyhow!("trailing backslash in string")),
                        },
                        Some(c) => s.push(c),
                    }
                }
                tokens.push(Token::Quoted(s));
            }
            _ => {
                // Unquoted identifier — read until whitespace or delimiter.
                let mut s = String::new();
                while let Some(&c) = chars.peek() {
                    if c.is_whitespace() || c == '{' || c == '}' || c == '"' || c == '/' {
                        break;
                    }
                    s.push(c);
                    chars.next();
                }
                if !s.is_empty() {
                    tokens.push(Token::Quoted(s));
                }
            }
        }
    }
    Ok(tokens)
}

pub(crate) fn parse_entries(
    tokens: &mut std::vec::IntoIter<Token>,
    expect_close: bool,
) -> Result<Vec<(String, KvValue)>> {
    let mut entries = Vec::new();
    loop {
        let key = match tokens.next() {
            None => {
                if expect_close {
                    return Err(anyhow!("unexpected EOF, expected closing '}}'"));
                }
                return Ok(entries);
            }
            Some(Token::Close) => {
                if !expect_close {
                    return Err(anyhow!("unexpected '}}' at top level"));
                }
                return Ok(entries);
            }
            Some(Token::Open) => return Err(anyhow!("unexpected '{{' where key was expected")),
            Some(Token::Quoted(s)) => s,
        };
        let value = match tokens.next() {
            None => return Err(anyhow!("expected value after key {:?}", key)),
            Some(Token::Quoted(s)) => KvValue::Str(s),
            Some(Token::Open) => KvValue::Block(parse_entries(tokens, true)?),
            Some(Token::Close) => return Err(anyhow!("unexpected '}}' after key {:?}", key)),
        };
        entries.push((key, value));
    }
}

/// Parse a Valve KeyValues document (used by both `.acf` and `.vdf`) into a
/// flat list of top-level `(key, value)` entries. Most documents have exactly
/// one top-level key (`AppState` or `libraryfolders`), but the format itself
/// allows multiple.
pub fn parse(text: &str) -> Result<Vec<(String, KvValue)>> {
    let tokens = tokenize(text)?;
    parse_entries(&mut tokens.into_iter(), false)
}

// ---------------------------------------------------------------------------
// appmanifest_*.acf
// ---------------------------------------------------------------------------

/// The fields we care about from a single `appmanifest_<appid>.acf`. Only
/// public Steam metadata — never anything from the game's own files.
#[derive(Debug, Clone, serde::Serialize)]
pub struct RawAppManifest {
    pub appid: u32,
    pub name: String,
    pub install_dir: String,
    pub size_on_disk: u64,
    /// Unix seconds. `None` when ACF reports `0` (Steam's "never played /
    /// reinstalled and lost the record" sentinel).
    pub last_played: Option<u64>,
    pub last_updated: Option<u64>,
    pub state_flags: u32,
    pub bytes_to_download: u64,
    pub bytes_downloaded: u64,
}

impl RawAppManifest {
    /// Steam writes `StateFlags = 4` for "fully installed". Anything else is
    /// "needs update / partially downloaded / removing / etc." — we treat
    /// non-4 as a signal the install is incomplete.
    pub fn is_fully_installed(&self) -> bool {
        self.state_flags == 4
    }
}

pub fn parse_appmanifest(text: &str) -> Result<RawAppManifest> {
    let top = parse(text).context("invalid KeyValues syntax in appmanifest")?;
    let app_state = find_ci(&top, "AppState")
        .and_then(KvValue::as_block)
        .ok_or_else(|| anyhow!("missing top-level 'AppState' block"))?;

    let get = |key: &str| -> Option<&str> { find_ci(app_state, key).and_then(KvValue::as_str) };
    let parse_u32 = |key: &str| -> u32 { get(key).and_then(|s| s.parse().ok()).unwrap_or(0) };
    let parse_u64 = |key: &str| -> u64 { get(key).and_then(|s| s.parse().ok()).unwrap_or(0) };

    let appid = parse_u32("appid");
    if appid == 0 {
        return Err(anyhow!("appmanifest missing or invalid appid"));
    }
    let name = get("name").unwrap_or("").to_string();
    let install_dir = get("installdir").unwrap_or("").to_string();
    let size_on_disk = parse_u64("SizeOnDisk");
    let last_played_raw = parse_u64("LastPlayed");
    let last_updated_raw = parse_u64("LastUpdated");
    let state_flags = parse_u32("StateFlags");
    let bytes_to_download = parse_u64("BytesToDownload");
    let bytes_downloaded = parse_u64("BytesDownloaded");

    Ok(RawAppManifest {
        appid,
        name,
        install_dir,
        size_on_disk,
        last_played: (last_played_raw != 0).then_some(last_played_raw),
        last_updated: (last_updated_raw != 0).then_some(last_updated_raw),
        state_flags,
        bytes_to_download,
        bytes_downloaded,
    })
}

// ---------------------------------------------------------------------------
// libraryfolders.vdf
// ---------------------------------------------------------------------------

/// Parse `<steam_root>/config/libraryfolders.vdf` and return every library
/// root path it lists. Steam stores them under indexed keys (`"0"`, `"1"`,
/// ...) inside the top-level `libraryfolders` block; each entry has a
/// `path` field.
pub fn parse_libraryfolders(text: &str) -> Result<Vec<PathBuf>> {
    let top = parse(text).context("invalid KeyValues syntax in libraryfolders.vdf")?;
    let lf = find_ci(&top, "libraryfolders")
        .and_then(KvValue::as_block)
        .ok_or_else(|| anyhow!("missing top-level 'libraryfolders' block"))?;

    let mut roots = Vec::new();
    for (_, v) in lf {
        let entry = match v.as_block() {
            Some(b) => b,
            None => continue,
        };
        if let Some(p) = find_ci(entry, "path").and_then(KvValue::as_str) {
            roots.push(PathBuf::from(p));
        }
    }
    Ok(roots)
}
