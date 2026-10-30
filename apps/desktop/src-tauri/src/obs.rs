//! obs —— 观测持久化内核（R479）。
//!
//! 把散落在各历史模块里的 jsonl / state 文件读写族规收编到这里：
//! - 缺文件 = 真空态（Ok 空），绝不伪装成故障；文件在但打不开/整文件损坏 = Err（R88 诚实性）。
//! - 损坏行策略统一为「跳过 + 计数 + 上报」，只有**全部**行都损坏才 Err —— 一行撕裂
//!   （崩溃时写了一半）不该杀掉全部历史。
//! - 写一律走「临时文件 + rename」原子覆写；append 走 O_APPEND 单行写。
//! - 写失败一律静默返回 bool，不阻塞主流程（观测层不得拖垮主功能）。
//!
//! 新记录建议带 `"v": 1` 字段，方便以后格式演化时按版本分流。

use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::Serialize;

/// 一次 jsonl 读取的结果：有效记录 + 被跳过的损坏行数（供 observations 聚合层上报）。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ReadOut<T> {
    pub items: Vec<T>,
    pub corrupt_lines: usize,
}

impl<T> ReadOut<T> {
    pub(crate) fn empty() -> Self {
        ReadOut {
            items: Vec::new(),
            corrupt_lines: 0,
        }
    }
}

fn tmp_path(p: &Path) -> PathBuf {
    let mut s = p.as_os_str().to_owned();
    s.push(".tmp");
    PathBuf::from(s)
}

fn ensure_parent(p: &Path) -> bool {
    match p.parent() {
        Some(parent) if !parent.as_os_str().is_empty() && !parent.exists() => {
            std::fs::create_dir_all(parent).is_ok()
        }
        _ => true,
    }
}

/// 读 jsonl 全文。缺文件 = Ok(ReadOut::empty())；文件级 IO 失败 = Err；
/// 损坏行跳过并计入 corrupt_lines；corrupt > 0 且没有任何有效记录 = Err（R88：故障不得伪装成真空态）。
pub(crate) fn read_jsonl<T: DeserializeOwned>(p: &Path) -> Result<ReadOut<T>, String> {
    if !p.exists() {
        return Ok(ReadOut::empty());
    }
    let f = std::fs::File::open(p).map_err(|e| format!("打开 {} 失败: {e}", p.display()))?;
    let reader = BufReader::new(f);
    let mut items = Vec::new();
    let mut corrupt = 0usize;
    for line in reader.lines() {
        let Ok(line) = line else {
            corrupt += 1;
            continue;
        };
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        match serde_json::from_str::<T>(t) {
            Ok(v) => items.push(v),
            Err(_) => corrupt += 1,
        }
    }
    if corrupt > 0 && items.is_empty() {
        return Err(format!("{} 存在但全部行损坏（{corrupt} 行）", p.display()));
    }
    Ok(ReadOut {
        items,
        corrupt_lines: corrupt,
    })
}

/// 读末尾至多 n 条有效记录（历史文件都有上限，读全文件再裁尾部可接受）。
pub(crate) fn read_jsonl_tail<T: DeserializeOwned>(
    p: &Path,
    n: usize,
) -> Result<ReadOut<T>, String> {
    let mut r = read_jsonl(p)?;
    if r.items.len() > n {
        r.items.drain(..r.items.len() - n);
    }
    Ok(r)
}

/// 读最后一条有效记录。缺文件 = Ok(None)。
pub(crate) fn read_jsonl_latest<T: DeserializeOwned>(p: &Path) -> Result<Option<T>, String> {
    Ok(read_jsonl(p)?.items.into_iter().last())
}

/// 整文件覆写（临时文件 + rename 原子替换）。写失败静默返回 false。
pub(crate) fn write_jsonl<T: Serialize>(p: &Path, items: &[T]) -> bool {
    if !ensure_parent(p) {
        return false;
    }
    let tmp = tmp_path(p);
    let Ok(mut out) = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&tmp)
    else {
        return false;
    };
    for it in items {
        let Ok(line) = serde_json::to_string(it) else {
            continue;
        };
        if writeln!(out, "{line}").is_err() {
            return false;
        }
    }
    if out.flush().is_err() {
        return false;
    }
    drop(out);
    std::fs::rename(&tmp, p).is_ok()
}

/// 追加一行。写失败静默返回 false。
pub(crate) fn append_jsonl<T: Serialize>(p: &Path, item: &T) -> bool {
    if !ensure_parent(p) {
        return false;
    }
    let Ok(line) = serde_json::to_string(item) else {
        return false;
    };
    let Ok(mut f) = OpenOptions::new().create(true).append(true).open(p) else {
        return false;
    };
    writeln!(f, "{line}").is_ok()
}

/// 单文档 state 文件整写（pretty json，临时文件 + rename）。写失败静默返回 false。
pub(crate) fn write_state<T: Serialize>(p: &Path, v: &T) -> bool {
    if !ensure_parent(p) {
        return false;
    }
    let Ok(text) = serde_json::to_string_pretty(v) else {
        return false;
    };
    let tmp = tmp_path(p);
    let Ok(mut out) = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&tmp)
    else {
        return false;
    };
    if out.write_all(text.as_bytes()).is_err() {
        return false;
    }
    drop(out);
    std::fs::rename(&tmp, p).is_ok()
}

/// 读单文档 state。缺文件 = Ok(None)；文件在但读失败/内容损坏 = Err（R88）。
pub(crate) fn read_state<T: DeserializeOwned>(p: &Path) -> Result<Option<T>, String> {
    if !p.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(p).map_err(|e| format!("读取 {} 失败: {e}", p.display()))?;
    let v =
        serde_json::from_str(&text).map_err(|e| format!("{} 存在损坏内容: {e}", p.display()))?;
    Ok(Some(v))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct Rec {
        v: u64,
    }

    fn tmp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("dp-obs-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn missing_file_is_vacuum_not_error() {
        let d = tmp_dir("missing");
        let p = d.join("nope.jsonl");
        let r = read_jsonl::<Rec>(&p).unwrap();
        assert!(r.items.is_empty());
        assert_eq!(r.corrupt_lines, 0);
        assert_eq!(read_jsonl_latest::<Rec>(&p).unwrap(), None);
        assert_eq!(read_state::<Rec>(&d.join("nope.json")).unwrap(), None);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn write_then_read_roundtrip_and_no_tmp_left() {
        let d = tmp_dir("roundtrip");
        let p = d.join("a.jsonl");
        let items = vec![Rec { v: 1 }, Rec { v: 2 }];
        assert!(write_jsonl(&p, &items));
        let r = read_jsonl::<Rec>(&p).unwrap();
        assert_eq!(r.items, items);
        assert_eq!(r.corrupt_lines, 0);
        assert!(!tmp_path(&p).exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn append_accumulates() {
        let d = tmp_dir("append");
        let p = d.join("b.jsonl");
        assert!(append_jsonl(&p, &Rec { v: 1 }));
        assert!(append_jsonl(&p, &Rec { v: 2 }));
        let r = read_jsonl::<Rec>(&p).unwrap();
        assert_eq!(r.items.len(), 2);
        assert_eq!(r.items[1].v, 2);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn torn_line_is_skipped_and_counted_not_fatal() {
        let d = tmp_dir("torn");
        let p = d.join("c.jsonl");
        std::fs::write(
            &p,
            format!(
                "{}\nnot json !!!\n{}\n",
                serde_json::to_string(&Rec { v: 1 }).unwrap(),
                serde_json::to_string(&Rec { v: 2 }).unwrap()
            ),
        )
        .unwrap();
        let r = read_jsonl::<Rec>(&p).unwrap();
        assert_eq!(r.items.len(), 2);
        assert_eq!(r.corrupt_lines, 1);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn all_corrupt_is_error_not_vacuum() {
        // R88：文件在但一行有效记录都没有 = 故障，必须 Err，不得伪装成真空态。
        let d = tmp_dir("allbad");
        let p = d.join("d.jsonl");
        std::fs::write(&p, "not json\nalso not json\n").unwrap();
        assert!(read_jsonl::<Rec>(&p).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn tail_keeps_last_n() {
        let d = tmp_dir("tail");
        let p = d.join("e.jsonl");
        let items: Vec<Rec> = (0..10).map(|v| Rec { v }).collect();
        assert!(write_jsonl(&p, &items));
        let r = read_jsonl_tail::<Rec>(&p, 3).unwrap();
        assert_eq!(r.items.len(), 3);
        assert_eq!(r.items[0].v, 7);
        assert_eq!(read_jsonl_latest::<Rec>(&p).unwrap().unwrap().v, 9);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn state_roundtrip_and_corrupt_is_err() {
        let d = tmp_dir("state");
        let p = d.join("s.json");
        assert!(write_state(&p, &Rec { v: 42 }));
        assert_eq!(read_state::<Rec>(&p).unwrap(), Some(Rec { v: 42 }));
        assert!(!tmp_path(&p).exists());
        std::fs::write(&p, "{ broken").unwrap();
        assert!(read_state::<Rec>(&p).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn write_creates_missing_parent_dirs() {
        let d = tmp_dir("parent");
        let p = d.join("nested/deep/f.jsonl");
        assert!(append_jsonl(&p, &Rec { v: 1 }));
        assert_eq!(read_jsonl::<Rec>(&p).unwrap().items.len(), 1);
        let _ = std::fs::remove_dir_all(&d);
    }
}
