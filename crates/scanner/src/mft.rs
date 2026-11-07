//! Windows-only NTFS Master File Table direct reader.
//!
//! WizTree-class scanner: opens the volume as a raw block device, parses the
//! NTFS boot sector via the `ntfs` crate, then iterates every record in the
//! `$MFT` file. Each record yields parent FRN + filename + size in one pass,
//! so a 1M-file C: drive scans in seconds rather than minutes.
//!
//! Requires admin privileges (raw volume open is privileged). On non-NTFS
//! filesystems or access-denied, the caller should fall back to `walkdir`.
//!
//! # Fast path (2026-09-11)
//!
//! The original implementation called `ntfs::file()` once per record. Each
//! call re-locates `$MFT` (parses its own record + $DATA attribute), then
//! `seek()`s the attribute value (rewinding through every data run) and reads
//! one record — 2 seeks + 3 parses per record, all serialized. That is the
//! slow path (`scan_volume_slow`).
//!
//! The fast path reads the whole `$MFT` into memory with a single
//! `read_to_end` (the attribute value reader walks data runs sequentially,
//! so this is a handful of system calls total), applies the USN fixup in
//! memory once, then parses each FILE record by offset — zero seeks per
//! record. It falls back to the slow path on any inconsistency.

use anyhow::Context;
use ntfs::Ntfs;
use std::fs::OpenOptions;
use std::io::BufReader;
use std::os::windows::fs::OpenOptionsExt;
use std::path::Path;
use std::time::Instant;

use crate::Node;

const FILE_SHARE_READ: u32 = 0x0000_0001;
const FILE_SHARE_WRITE: u32 = 0x0000_0002;
const FILE_FLAG_SEQUENTIAL_SCAN: u32 = 0x0800_0000;
const GENERIC_READ: u32 = 0x8000_0000;
pub(crate) const NTFS_BLOCK_SIZE: usize = 512;
/// 快路径每解析多少条记录回调一次进度（对齐慢路径 50_000）。
pub(crate) const PROGRESS_EVERY: u64 = 50_000;

/// 属性类型常量。
pub(crate) const ATTR_FILE_NAME: u32 = 0x30;
pub(crate) const ATTR_DATA: u32 = 0x80;

/// Windows 卷设备（`\\.\C:`）的 `ReadFile` 要求文件偏移与读长度都按
/// 逻辑扇区（512 B）对齐，非对齐小读返回 `ERROR_INVALID_PARAMETER`(87)。
/// 而 `ntfs` crate 的 `Ntfs::new` 用 binrw 逐字段（3/7 字节）读引导扇区，
/// 在裸卷上必然触发 os error 87。本读取器把所有底层读聚合为 512 B 对齐：
/// 对齐大读（长度与偏移均为 512 倍数）直接分块下发；小读/非对齐读经
/// 64 KiB 对齐缓冲区拷贝，绝不对卷设备发出非对齐小系统调用。
pub(crate) const VOL_BUF_CAP: usize = 64 * 1024;

pub(crate) struct Entry {
    pub(crate) name: String,
    pub(crate) parent_frn: u64,
    pub(crate) own_size: u64, // file size from $DATA, or 0 for dirs
    pub(crate) is_dir: bool,
    pub(crate) children: Vec<u64>, // FRNs of direct children (filled in second pass)
}

/// Scan an entire NTFS volume by reading $MFT directly.
///
/// `volume_letter` is e.g. `'C'`. `subroot` lets you ask only for a subtree
/// of the volume (e.g. `D:\Foo`); when `None`, the whole volume is returned.
pub fn scan_volume<F>(
    volume_letter: char,
    subroot: Option<&Path>,
    keep_files: Option<usize>,
    max_dirs: Option<usize>,
    mut on_progress: F,
) -> anyhow::Result<Node>
where
    F: FnMut(u64, u64) -> bool, // (records_seen, bytes_seen) → false = 调用方要求中止
{
    let path = format!(r"\\.\{}:", volume_letter.to_ascii_uppercase());
    let file = OpenOptions::new()
        .read(true)
        .access_mode(GENERIC_READ)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_SEQUENTIAL_SCAN)
        .open(&path)
        .with_context(|| format!("opening volume {} (admin required)", path))?;

    let mut reader = AlignedVolumeReader::new(BufReader::with_capacity(1 << 20, file));
    let mut ntfs = Ntfs::new(&mut reader).context("parsing NTFS boot sector")?;
    ntfs.read_upcase_table(&mut reader).ok();

    let record_size = ntfs.file_record_size() as usize;
    let t0 = Instant::now();

    // 快路径：整块读 $MFT 进内存 + 内存解析。失败（记录碎片/边界异常）
    // 时如实回退到逐条慢路径——绝不让扫描因一条坏记录整个挂掉。
    if let Ok((mut entries, bytes_total)) = parse_mft_fast(
        &mut reader,
        &ntfs,
        volume_letter,
        record_size,
        &mut on_progress,
    ) {
        let elapsed = t0.elapsed();
        tracing::info!(
            "MFT fast path: volume={} entries={} bytes={} elapsed_ms={}",
            volume_letter,
            entries.len(),
            bytes_total,
            elapsed.as_millis()
        );
        // 进度回调：快路径解析完成后一次性回报（总条数 + 总字节），
        // 语义与慢路径的「扫到第 N 条」一致，只是没有中间回调。
        if !on_progress(entries.len() as u64, bytes_total) {
            return Err(anyhow::anyhow!("scan:cancelled: after mft fast parse"));
        }
        link_children(&mut entries);
        // 子路径（非盘根）解析失败 → 报错让上层回退 walkdir，绝不回退整卷
        // （8.3 短名 / 重解析点 / 大小写差异都会导致逐组件匹配失败）。
        match build_node_tree(
            volume_letter,
            subroot,
            keep_files,
            max_dirs,
            &entries,
            bytes_total,
        ) {
            Ok(node) => return Ok(node),
            Err(_) => {
                return Err(anyhow::anyhow!(
                    "MFT scan: subroot path not resolvable in $MFT"
                ))
            }
        }
    }

    // 慢路径（原实现）：逐条 ntfs::file()。
    tracing::warn!(
        "MFT fast path failed for volume {}, falling back to per-record parsing",
        volume_letter
    );
    scan_volume_slow(
        &mut reader,
        &ntfs,
        volume_letter,
        subroot,
        keep_files,
        max_dirs,
        record_size,
        on_progress,
    )
}

pub(crate) use crate::mft_parse::parse_mft_fast;
pub(crate) use crate::mft_reader::AlignedVolumeReader;
pub(crate) use crate::mft_tree::{build_node_tree, link_children, scan_volume_slow};

#[cfg(test)]
#[path = "mft_tests.rs"]
mod tests;

/// 真实卷快/慢路径计时对比（`cargo run -p diskpilot-scanner --features bench -- bench <盘符>`）。
///
/// 只做只读测量：不删文件、不写盘。需要管理员权限打开卷设备。
/// 输出每路径一次完整扫描的毫秒数（解析循环 + children 链接 + 树构建），
/// 以及各自总条数——条数不一致说明两条路径语义有偏差，慢路径为准。
#[cfg(feature = "bench")]
pub mod bench {
    use super::*;
    // R511 修复：bench feature 编译断裂——mft_parse 是 crate 私有模块，
    // fixup_all/parse_record 需显式引入（此前缺 use 导致 --features bench 构建失败）。
    use crate::mft_parse::{fixup_all, parse_record};

    pub fn run(volume_letter: char) -> anyhow::Result<()> {
        let path = format!(r"\\.\{}:", volume_letter.to_ascii_uppercase());
        let file = OpenOptions::new()
            .read(true)
            .access_mode(GENERIC_READ)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_SEQUENTIAL_SCAN)
            .open(&path)
            .with_context(|| format!("opening volume {} (admin required)", path))?;

        // 对齐读取器：卷设备所有底层读都按 512 B 对齐，绕开 binrw 逐字段
        // 小读触发的 os error 87。
        let mut reader = AlignedVolumeReader::new(file);
        let mut ntfs = Ntfs::new(&mut reader).context("parsing NTFS boot sector")?;
        ntfs.read_upcase_table(&mut reader).ok();
        let record_size = ntfs.file_record_size() as usize;

        // 快路径（整块读 + 内存解析）。
        let t0 = Instant::now();
        let (mut entries_fast, bytes_fast) = parse_mft_fast(
            &mut reader,
            &ntfs,
            volume_letter,
            record_size,
            &mut |_, _| true,
        )?;
        let fast_ms = t0.elapsed().as_millis();
        link_children(&mut entries_fast);
        // bench 只跑盘根（subroot=None），不会 Err；失败给个可读兜底。
        let node_fast = build_node_tree(volume_letter, None, None, None, &entries_fast, bytes_fast)
            .unwrap_or_else(|e| {
                println!("快路径树构建失败：{e:#}");
                Node {
                    name: volume_letter.to_string(),
                    path: format!("{}:\\", volume_letter.to_ascii_uppercase()),
                    is_dir: true,
                    size: 0,
                    file_count: 0,
                    children: Vec::new(),
                    scaffold_id: None,
                    top_extensions: Vec::new(),
                    children_truncated: None,
                    mtime: None,
                }
            });

        // 慢路径（逐条 ntfs::file()），无进度回调。
        let t1 = Instant::now();
        let node_slow = scan_volume_slow(
            &mut reader,
            &ntfs,
            volume_letter,
            None,
            None,
            None,
            record_size,
            |_, _| true,
        )?;
        let slow_ms = t1.elapsed().as_millis();

        println!("盘符 {}: 记录大小 {} B", volume_letter, record_size);
        println!(
            "快路径: {} ms, 条目 {} 个, 树根大小 {} B, 树根文件数 {}",
            fast_ms,
            entries_fast.len(),
            node_fast.size,
            node_fast.file_count
        );
        println!(
            "慢路径: {} ms, 树根大小 {} B, 树根文件数 {}",
            slow_ms, node_slow.size, node_slow.file_count
        );
        let ratio = if slow_ms > 0 {
            slow_ms as f64 / fast_ms.max(1) as f64
        } else {
            0.0
        };
        println!("提速比: {:.2}× (慢/快)", ratio);
        println!("注: 树根大小与文件数两路径应一致（共用 build_node_tree），差异即语义偏差。");
        Ok(())
    }

    /// 内存吞吐基准（无需管理员/NTFS 卷）：对 N 条合成 FILE 记录跑
    /// `fixup_all + parse_record` 热循环，模拟快路径按记录数线性解析的
    /// 纯 CPU 吞吐。每条记录带一条合法 $FILE_NAME（Win32AndDos），
    /// 与测试模块 synth_record 相同的属性布局。`records` 默认 1_000_000。
    pub fn run_mem(records: usize) -> anyhow::Result<()> {
        let record_size = 1024usize;
        // 复用测试合成记录的属性部分：FILE 签名 + USA + 驻留 $FILE_NAME。
        let (mut rec, _usa) = synth_record_for_bench();
        rec.resize(record_size, 0);
        let mut buf = vec![0u8; records * record_size];
        for chunk in buf.chunks_exact_mut(record_size) {
            chunk.copy_from_slice(&rec);
        }
        let total_bytes = buf.len() as u64;

        let t0 = Instant::now();
        fixup_all(&mut buf, record_size)?;
        let fixup_ms = t0.elapsed().as_millis();

        let t1 = Instant::now();
        let mut parsed = 0u64;
        let mut bytes_seen = 0u64;
        for chunk in buf.chunks_exact(record_size) {
            if let Some((_, _, size, _)) = parse_record(chunk) {
                parsed += 1;
                bytes_seen += size;
            }
        }
        let parse_ms = t1.elapsed().as_millis();

        println!(
            "内存吞吐基准: {} 条合成记录 × {} B = {} B",
            records, record_size, total_bytes
        );
        println!(
            "fixup_all: {} ms（{} 条/秒）",
            fixup_ms,
            records.saturating_mul(1000) / (fixup_ms.max(1) as usize)
        );
        println!(
            "parse_record: {} ms, 解析 {} 条（{} 条/秒）, 累计 {} B",
            parse_ms,
            parsed,
            records.saturating_mul(1000) / (parse_ms.max(1) as usize),
            bytes_seen
        );
        Ok(())
    }

    /// 与测试模块共用：合成一条带驻留 $FILE_NAME 的 FILE 记录。
    /// （测试里是 `#[cfg(test)]` 私有，这里单独复刻一份给 bench。）
    fn synth_record_for_bench() -> (Vec<u8>, Vec<u8>) {
        let record_size = 1024usize;
        let mut rec = vec![0u8; record_size];
        rec[0..4].copy_from_slice(b"FILE");
        rec[4..6].copy_from_slice(&0x30u16.to_le_bytes());
        rec[6..8].copy_from_slice(&3u16.to_le_bytes());
        rec[20..22].copy_from_slice(&0x38u16.to_le_bytes());
        rec[22..24].copy_from_slice(&0x0000u16.to_le_bytes());
        let attrs_off = 0x38usize;

        let fname_attr = attrs_off;
        let fname_alen = 24 + 66 + 8usize;
        rec[fname_attr..fname_attr + 4].copy_from_slice(&0x30u32.to_le_bytes());
        rec[fname_attr + 4..fname_attr + 8].copy_from_slice(&(fname_alen as u32).to_le_bytes());
        rec[fname_attr + 8] = 0; // resident
        rec[fname_attr + 9] = 0;
        rec[fname_attr + 16..fname_attr + 20].copy_from_slice(&(66u32 + 8).to_le_bytes());
        rec[fname_attr + 20..fname_attr + 22].copy_from_slice(&24u16.to_le_bytes());

        let value_off = fname_attr + 24;
        rec[value_off..value_off + 8].copy_from_slice(&5u64.to_le_bytes());
        rec[value_off + 64] = 2; // name_length (UTF-16 码元)
        rec[value_off + 65] = 3; // Win32AndDos
        rec[value_off + 66..value_off + 70].copy_from_slice(&[0xA5, 0x62, 0x4A, 0x54]);

        let usa: Vec<u8> = vec![0xAB, 0xCD, 0x11, 0x22, 0x33, 0x44];
        rec[0x30..0x36].copy_from_slice(&usa);
        rec[NTFS_BLOCK_SIZE - 2..NTFS_BLOCK_SIZE].copy_from_slice(&usa[0..2]);
        rec[2 * NTFS_BLOCK_SIZE - 2..2 * NTFS_BLOCK_SIZE].copy_from_slice(&usa[0..2]);
        rec[24..28].copy_from_slice(&(fname_attr as u32 + fname_alen as u32).to_le_bytes());
        (rec, usa)
    }
}
