//! 扫描报告导出（R516）：把内存里的扫描树 + 统计渲染成 markdown / csv / json。
//! 纯内存渲染，零磁盘 IO，给用户可分享的磁盘占用报告。

use crate::{Node, ScanStats};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportFormat {
    Markdown,
    Csv,
    Json,
}

impl ReportFormat {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "md" | "markdown" => Some(Self::Markdown),
            "csv" => Some(Self::Csv),
            "json" => Some(Self::Json),
            _ => None,
        }
    }
    pub fn extension(&self) -> &'static str {
        match self {
            Self::Markdown => "md",
            Self::Csv => "csv",
            Self::Json => "json",
        }
    }
}

fn fmt_bytes(n: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    const TB: u64 = GB * 1024;
    if n >= TB {
        format!("{:.2} TB", n as f64 / TB as f64)
    } else if n >= GB {
        format!("{:.2} GB", n as f64 / GB as f64)
    } else if n >= MB {
        format!("{:.1} MB", n as f64 / MB as f64)
    } else if n >= KB {
        format!("{:.1} KB", n as f64 / KB as f64)
    } else {
        format!("{n} B")
    }
}

/// 扁平化目录表：(相对路径, 字节, 文件数)，按字节降序。
pub fn flatten_dirs(root: &Node, prefix: &str, out: &mut Vec<(String, u64, u64)>) {
    let path = if prefix.is_empty() {
        root.name.clone()
    } else {
        format!("{prefix}/{}", root.name)
    };
    out.push((path.clone(), root.size, root.file_count));
    for c in &root.children {
        flatten_dirs(c, &path, out);
    }
}

fn top_extensions(root: &Node, top_n: usize) -> Vec<(String, u64, u64)> {
    let mut m: std::collections::HashMap<String, (u64, u64)> = std::collections::HashMap::new();
    fn walk(n: &Node, m: &mut std::collections::HashMap<String, (u64, u64)>) {
        for es in &n.top_extensions {
            let acc = m.entry(es.ext.clone()).or_insert((0, 0));
            acc.0 += es.bytes;
            acc.1 += es.count;
        }
        for c in &n.children {
            walk(c, m);
        }
    }
    walk(root, &mut m);
    let mut v: Vec<_> = m.into_iter().map(|(k, (b, c))| (k, b, c)).collect();
    v.sort_by_key(|e| std::cmp::Reverse(e.1));
    v.truncate(top_n);
    v
}

/// 渲染报告。`stats` 为 None 时统计段标注 n/a（如树来自缓存而统计未留存）。
pub fn render(root: &Node, stats: Option<&ScanStats>, fmt: ReportFormat, top_n: usize) -> String {
    match fmt {
        ReportFormat::Markdown => render_markdown(root, stats, top_n),
        ReportFormat::Csv => render_csv(root, top_n),
        ReportFormat::Json => render_json(root, stats),
    }
}

fn stats_block(stats: Option<&ScanStats>) -> String {
    match stats {
        Some(s) => format!(
            "mode={} | files={} | bytes={} | walk={}ms tree={}ms total={}ms",
            s.mode,
            s.files_seen,
            fmt_bytes(s.bytes_seen),
            s.walk_ms,
            s.build_tree_ms,
            s.total_ms
        ),
        None => "n/a".to_string(),
    }
}

fn render_markdown(root: &Node, stats: Option<&ScanStats>, top_n: usize) -> String {
    let mut out = String::new();
    out.push_str("# DiskPilot 扫描报告\n\n");
    out.push_str(&format!("- 扫描根: `{}`\n", root.name));
    out.push_str(&format!(
        "- 总大小: **{}**（{} 个文件）\n",
        fmt_bytes(root.size),
        root.file_count
    ));
    out.push_str(&format!("- 扫描统计: {}\n", stats_block(stats)));
    out.push_str(&format!("- 生成时间: {}\n\n", chrono_now()));

    out.push_str(&format!("## Top {} 目录（按占用字节）\n\n", top_n));
    out.push_str("| 目录 | 大小 | 文件数 |\n|---|---|---|\n");
    let mut dirs = Vec::new();
    flatten_dirs(root, "", &mut dirs);
    dirs.sort_by_key(|d| std::cmp::Reverse(d.1));
    for (p, sz, fc) in dirs.iter().take(top_n) {
        out.push_str(&format!("| `{p}` | {} | {} |\n", fmt_bytes(*sz), fc));
    }

    out.push_str(&format!("\n## Top {} 文件类型（按占用字节）\n\n", top_n));
    out.push_str("| 扩展名 | 大小 | 文件数 |\n|---|---|---|\n");
    for (ext, b, c) in top_extensions(root, top_n) {
        out.push_str(&format!("| {ext} | {} | {} |\n", fmt_bytes(b), c));
    }
    out
}

fn render_csv(root: &Node, top_n: usize) -> String {
    let mut out = String::from("path,size_bytes,size_human,file_count\n");
    let mut dirs = Vec::new();
    flatten_dirs(root, "", &mut dirs);
    dirs.sort_by_key(|d| std::cmp::Reverse(d.1));
    for (p, sz, fc) in dirs.iter().take(top_n.max(1000)) {
        // CSV 转义：路径含逗号/引号时包引号并把内部引号翻倍。
        let esc = if p.contains(',') || p.contains('"') || p.contains('\n') {
            format!("\"{}\"", p.replace('"', "\"\""))
        } else {
            p.clone()
        };
        out.push_str(&format!("{esc},{sz},{},{}\n", fmt_bytes(*sz), fc));
    }
    out
}

#[derive(Serialize)]
struct JsonReport<'a> {
    root: &'a str,
    total_bytes: u64,
    file_count: u64,
    stats: Option<&'a ScanStats>,
    generated_at: String,
    tree: &'a Node,
}

fn render_json(root: &Node, stats: Option<&ScanStats>) -> String {
    let r = JsonReport {
        root: &root.name,
        total_bytes: root.size,
        file_count: root.file_count,
        stats,
        generated_at: chrono_now(),
        tree: root,
    };
    serde_json::to_string_pretty(&r).unwrap_or_else(|_| "{}".to_string())
}

fn chrono_now() -> String {
    // 不引 chrono：手算 UTC 时间戳格式化太烦，用 SystemTime 自纪元秒粗暴格式化。
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // YYYY-MM-DD HH:MM:SS UTC 的 civil 算法（Howard Hinnant 公式）。
    let days = (secs / 86400) as i64;
    let rem = secs % 86400;
    let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mo <= 2 { y + 1 } else { y };
    format!("{y:04}-{mo:02}-{d:02} {h:02}:{m:02}:{s:02} UTC")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Node;

    fn leaf(name: &str, size: u64, files: u64) -> Node {
        Node {
            name: name.to_string(),
            path: format!("/{name}"),
            is_dir: false,
            size,
            file_count: files,
            children: vec![],
            scaffold_id: None,
            top_extensions: vec![],
            children_truncated: None,
            mtime: None,
        }
    }

    #[test]
    fn markdown_has_top_dirs_sorted() {
        let mut root = leaf("C:\\", 300, 3);
        root.is_dir = true;
        let mut big = leaf("big", 200, 2);
        big.is_dir = true;
        big.path = "/C:\\/big".into();
        big.children = vec![leaf("a.bin", 200, 2)];
        root.children = vec![big, leaf("small", 100, 1)];
        let md = render(&root, None, ReportFormat::Markdown, 10);
        assert!(md.contains("# DiskPilot 扫描报告"));
        let big_pos = md.find("big").unwrap();
        let small_pos = md.find("small").unwrap();
        assert!(big_pos < small_pos, "大目录应排在前面");
    }

    #[test]
    fn csv_escapes_commas_and_quotes() {
        let mut root = leaf("r", 1, 1);
        root.children = vec![leaf("a,\"b\"", 1, 1)];
        let csv = render(&root, None, ReportFormat::Csv, 10);
        // 路径 r/a,"b" 应转义为 "r/a,""b"""（外层引号 + 内部引号翻倍）。
        assert!(
            csv.contains("\"r/a,\"\"b\"\"\""),
            "CSV 字段需正确转义: {csv}"
        );
    }

    #[test]
    fn json_roundtrip_shape() {
        let root = leaf("C:\\", 42, 7);
        let j = render(&root, None, ReportFormat::Json, 10);
        let v: serde_json::Value = serde_json::from_str(&j).unwrap();
        assert_eq!(v["total_bytes"], 42);
        assert_eq!(v["file_count"], 7);
        assert_eq!(v["tree"]["name"], "C:\\");
    }

    #[test]
    fn format_parse_and_ext() {
        assert_eq!(ReportFormat::parse("md"), Some(ReportFormat::Markdown));
        assert_eq!(ReportFormat::parse("CSV"), Some(ReportFormat::Csv));
        assert_eq!(ReportFormat::parse("json"), Some(ReportFormat::Json));
        assert_eq!(ReportFormat::parse("pdf"), None);
        assert_eq!(ReportFormat::Csv.extension(), "csv");
    }

    #[test]
    fn chrono_now_looks_like_timestamp() {
        let t = chrono_now();
        assert!(
            t.ends_with(" UTC") && t.len() == 23,
            "格式应为 YYYY-MM-DD HH:MM:SS UTC: {t}"
        );
    }
}
