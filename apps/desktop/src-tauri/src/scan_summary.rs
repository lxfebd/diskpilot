//! 扫描摘要历史（R480）：每次真实扫描（commit_scan）把一份**聚合摘要**追加到
//! `app_data_dir/scan-summary.jsonl`，让「上次扫描看到了什么」可在进程外追溯——
//! 而不是只有内存里的 scan_tree（进程一退全丢，P0-3 的结构性缺口之一）。
//!
//! 每条摘要是扫描根的**聚合统计**，不存路径明细（明细量级是整棵树，不适合
//! 追加式历史）：文件/目录数、总字节、一级子目录 Top20、大文件 Top50、
//! 按体量分档的直方图。明细仍以 scan_tree 内存树 + 按需子树展开为准。
//!
//! 族规与其它历史模块一致（R479 obs 内核，R88 诚实性）：
//! - 追加式 jsonl，记录带 `"v": 1`；
//! - **每 root 最近 MAX_PER_ROOT 条**，超长丢最旧；
//! - 缺文件 = 真空态空 vec；文件在但读失败/全部行损坏 = Err（查询侧）；
//!   写路径静默失败（bool），不影响扫描主流程；
//! - 单条撕裂行跳过并计数（obs::ReadOut.corrupt_lines），不整体 Err。

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use diskpilot_scanner::{Node, ScanStats};

/// 一级子目录的体量占比（Top N 里的一条）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct DirShare {
    pub path: String,
    pub bytes: u64,
    pub files: u64,
}

/// 大文件 Top N 里的一条。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct FileShare {
    pub path: String,
    pub bytes: u64,
}

/// 体量分档直方图的一档。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct SizeBucket {
    pub label: String,
    pub count: u64,
    pub bytes: u64,
}

/// 一次扫描的聚合摘要。`root` 是归一化扫描根（与 scan_stamp 同口径）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct ScanSummary {
    pub v: u32,
    pub root: String,
    pub at: u64,
    pub duration_ms: u64,
    pub mode: String,
    pub file_count: u64,
    pub dir_count: u64,
    pub total_bytes: u64,
    pub top_dirs: Vec<DirShare>,
    pub top_files: Vec<FileShare>,
    pub buckets: Vec<SizeBucket>,
}

/// 每 root 最多保留的摘要条数。
const MAX_PER_ROOT: usize = 30;
/// 一级子目录 Top N 上限。
const TOP_DIRS: usize = 20;
/// 大文件 Top N 上限。
const TOP_FILES: usize = 50;

/// 体量分档：label + 上界（闭区间 [下界, 上界)）。
const BUCKETS: [(&str, u64); 5] = [
    ("<1MB", 1 << 20),
    ("1-100MB", 100 << 20),
    ("100MB-1GB", 1 << 30),
    ("1-10GB", 10u64 << 30),
    (">=10GB", u64::MAX),
];

fn summary_path(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_data_dir()
        .ok()
        .map(|d| d.join("scan-summary.jsonl"))
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 从完整树单趟聚合出摘要。迭代栈避免深树递归栈溢出；大文件 TopN 用
/// 小顶堆保持 O(1) 插入。O(nodes)，对百万节点树也只是毫秒级，可在
/// commit_scan 里同步调用（扫描本身已耗时秒级以上）。
fn build_summary(root: &str, stats: &ScanStats, tree: &Node) -> ScanSummary {
    let mut file_count = 0u64;
    let mut dir_count = 0u64;
    let mut total_bytes = 0u64;
    let mut buckets: Vec<SizeBucket> = BUCKETS
        .iter()
        .map(|(label, _)| SizeBucket {
            label: label.to_string(),
            count: 0,
            bytes: 0,
        })
        .collect();
    // 小顶堆按 bytes 排，堆顶是当前 TopN 里最小的，超长弹出。
    let mut top_files: BinaryHeap<Reverse<(u64, String)>> = BinaryHeap::new();

    let mut stack: Vec<&Node> = vec![tree];
    while let Some(n) = stack.pop() {
        if n.is_dir {
            dir_count += 1;
            stack.extend(n.children.iter());
        } else {
            file_count += 1;
            total_bytes = total_bytes.saturating_add(n.size);
            let bi = BUCKETS
                .iter()
                .position(|(_, upper)| n.size < *upper)
                .unwrap_or(BUCKETS.len() - 1);
            buckets[bi].count += 1;
            buckets[bi].bytes = buckets[bi].bytes.saturating_add(n.size);
            top_files.push(Reverse((n.size, n.path.clone())));
            if top_files.len() > TOP_FILES {
                top_files.pop();
            }
        }
    }

    let mut top_dirs: Vec<DirShare> = tree
        .children
        .iter()
        .filter(|c| c.is_dir)
        .map(|c| DirShare {
            path: c.path.clone(),
            bytes: c.size,
            files: c.file_count,
        })
        .collect();
    top_dirs.sort_by_key(|d| Reverse(d.bytes));
    top_dirs.truncate(TOP_DIRS);

    let mut top_files: Vec<FileShare> = top_files
        .into_iter()
        .map(|Reverse((bytes, path))| FileShare { path, bytes })
        .collect();
    top_files.sort_by_key(|f| Reverse(f.bytes));

    ScanSummary {
        v: 1,
        root: root.to_string(),
        at: now_secs(),
        duration_ms: stats.total_ms,
        mode: stats.mode.clone(),
        file_count,
        dir_count,
        total_bytes,
        top_dirs,
        top_files,
        buckets,
    }
}

/// 每 root 只留最新 MAX_PER_ROOT 条（按 at 新旧，靠后的更新）。
fn prune(items: &mut Vec<ScanSummary>) {
    let mut seen: HashMap<&str, usize> = HashMap::new();
    let mut keep = vec![false; items.len()];
    for i in (0..items.len()).rev() {
        let c = seen.entry(items[i].root.as_str()).or_insert(0);
        if *c < MAX_PER_ROOT {
            keep[i] = true;
            *c += 1;
        }
    }
    let mut j = 0;
    items.retain(|_| {
        let k = keep[j];
        j += 1;
        k
    });
}

fn append_pruned(p: &Path, s: &ScanSummary) -> bool {
    // 写路径静默：读失败/全损坏都当真空态重建（查询侧才报 Err）。
    let mut items: Vec<ScanSummary> = crate::obs::read_jsonl(p)
        .map(|r| r.items)
        .unwrap_or_default();
    items.push(s.clone());
    prune(&mut items);
    crate::obs::write_jsonl(p, &items)
}

/// 记录一次扫描摘要。静默失败（bool），不影响扫描主流程返回。
pub(crate) fn record_scan_summary(
    app: &AppHandle,
    root: &str,
    stats: &ScanStats,
    tree: &Node,
) -> bool {
    let Some(p) = summary_path(app) else {
        return false;
    };
    let s = build_summary(root, stats, tree);
    append_pruned(&p, &s)
}

/// 查询侧读取全部摘要。缺文件 = Ok 空；读失败/全部行损坏 = Err（R88）。
pub(crate) fn read_summaries(app: &AppHandle) -> Result<Vec<ScanSummary>, String> {
    match summary_path(app) {
        Some(p) => read_summaries_at(&p),
        None => Ok(Vec::new()),
    }
}

/// 测试/聚合层用：直接读某个路径。
pub(crate) fn read_summaries_at(p: &Path) -> Result<Vec<ScanSummary>, String> {
    Ok(crate::obs::read_jsonl(p)?.items)
}

/// 扫描摘要历史（给前端「扫描历史」面板 / R482 聚合层用）。缺文件返回空列表。
#[tauri::command]
pub(crate) fn scan_summary_history(app: AppHandle) -> Result<Vec<ScanSummary>, String> {
    read_summaries(&app)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str, size: u64) -> Node {
        Node {
            name: path.rsplit('/').next().unwrap_or(path).to_string(),
            path: path.to_string(),
            is_dir: false,
            size,
            file_count: 1,
            children: Vec::new(),
            scaffold_id: None,
            top_extensions: Vec::new(),
            children_truncated: None,
            mtime: None,
        }
    }

    fn dir(path: &str, size: u64, file_count: u64, children: Vec<Node>) -> Node {
        Node {
            name: path.rsplit('/').next().unwrap_or(path).to_string(),
            path: path.to_string(),
            is_dir: true,
            size,
            file_count,
            children,
            scaffold_id: None,
            top_extensions: Vec::new(),
            children_truncated: None,
            mtime: None,
        }
    }

    fn stats(mode: &str, total_ms: u64) -> ScanStats {
        ScanStats {
            mode: mode.to_string(),
            total_ms,
            ..Default::default()
        }
    }

    #[test]
    fn aggregates_counts_buckets_and_tops() {
        let tree = dir(
            "c:/",
            0,
            4,
            vec![
                dir("c:/big", 5 << 30, 1, vec![file("c:/big/huge.bin", 5 << 30)]),
                dir(
                    "c:/mid",
                    500 << 20,
                    2,
                    vec![
                        file("c:/mid/a.bin", 400 << 20),
                        file("c:/mid/b.bin", 100 << 20),
                    ],
                ),
                file("c:/small.txt", 512),
            ],
        );
        let s = build_summary("c:/", &stats("mft", 1234), &tree);
        assert_eq!(s.v, 1);
        assert_eq!(s.mode, "mft");
        assert_eq!(s.duration_ms, 1234);
        // 3 个目录（根 + big + mid）+ 4 个文件（huge/a/b/small）。
        assert_eq!(s.dir_count, 3);
        assert_eq!(s.file_count, 4);
        assert_eq!(s.total_bytes, (5u64 << 30) + (500u64 << 20) + 512);
        // top_dirs 按 bytes 降序，只算一级子目录。
        assert_eq!(s.top_dirs.len(), 2);
        assert_eq!(s.top_dirs[0].path, "c:/big");
        assert_eq!(s.top_dirs[0].files, 1);
        // 大文件第一名是 huge.bin。
        assert_eq!(s.top_files[0].path, "c:/big/huge.bin");
        assert_eq!(s.top_files[0].bytes, 5u64 << 30);
        // 档位：512 → <1MB；400MB、100MB → 100MB-1GB? 400MB 是 100MB-1GB 档，
        // 100MB 正好落 1-100MB 上界(< 上界进位)——100<<20 不小于 100<<20，进 100MB-1GB。
        let b = |label: &str| s.buckets.iter().find(|b| b.label == label).unwrap();
        assert_eq!(b("<1MB").count, 1);
        assert_eq!(b("100MB-1GB").count, 2);
        assert_eq!(b(">=10GB").count, 0);
        assert_eq!(b("1-10GB").count, 1); // 5GB
        assert_eq!(b("1-10GB").bytes, 5u64 << 30);
    }

    #[test]
    fn top_files_capped_and_sorted_desc() {
        let kids: Vec<Node> = (0..120u64)
            .map(|i| file(&format!("c:/f{i:03}.bin"), 1000 + i))
            .collect();
        let tree = dir("c:/", 0, 120, kids);
        let s = build_summary("c:/", &stats("walkdir", 1), &tree);
        assert_eq!(s.top_files.len(), TOP_FILES);
        assert_eq!(s.top_files[0].bytes, 1000 + 119);
        assert!(s.top_files.windows(2).all(|w| w[0].bytes >= w[1].bytes));
    }

    #[test]
    fn prune_keeps_newest_max_per_root() {
        let mut items: Vec<ScanSummary> = (0..35u64)
            .map(|at| ScanSummary {
                v: 1,
                root: "c:/".to_string(),
                at,
                duration_ms: 0,
                mode: "mft".to_string(),
                file_count: 0,
                dir_count: 0,
                total_bytes: 0,
                top_dirs: Vec::new(),
                top_files: Vec::new(),
                buckets: Vec::new(),
            })
            .collect();
        items.push(ScanSummary {
            root: "d:/".to_string(),
            at: 100,
            ..items[0].clone()
        });
        prune(&mut items);
        let c: Vec<_> = items.iter().filter(|i| i.root == "c:/").collect();
        assert_eq!(c.len(), MAX_PER_ROOT);
        // 保留 at 最大的 30 条（5..=34），顺序保持追加序（升序）。
        assert_eq!(c[0].at, 5);
        assert_eq!(c.last().unwrap().at, 34);
        // 别的 root 不受影响。
        assert_eq!(items.iter().filter(|i| i.root == "d:/").count(), 1);
    }

    #[test]
    fn roundtrip_through_obs_and_torn_line() {
        let tdir = std::env::temp_dir().join(format!("dp-scansummary-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tdir);
        std::fs::create_dir_all(&tdir).unwrap();
        let p = tdir.join("scan-summary.jsonl");

        let tree = dir("c:/", 0, 1, vec![file("c:/a.txt", 42)]);
        let s = build_summary("c:/", &stats("mft", 7), &tree);
        assert!(append_pruned(&p, &s));
        assert!(append_pruned(&p, &s));

        // 撕裂一行：不整体 Err，有效行还在。
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new().append(true).open(&p).unwrap();
        writeln!(f, "torn line {{").unwrap();
        drop(f);
        let out = crate::obs::read_jsonl::<ScanSummary>(&p).unwrap();
        assert_eq!(out.items.len(), 2);
        assert_eq!(out.corrupt_lines, 1);
        let v = read_summaries_at(&p).unwrap();
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].file_count, 1);
        assert_eq!(v[0].total_bytes, 42);

        // 缺文件 = 真空态。
        let none = read_summaries_at(&tdir.join("nope.jsonl")).unwrap();
        assert!(none.is_empty());

        let _ = std::fs::remove_dir_all(&tdir);
    }
}
