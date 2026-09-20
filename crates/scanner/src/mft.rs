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

use anyhow::{anyhow, Context};
use ntfs::structured_values::{NtfsFileName, NtfsFileNamespace};
use ntfs::{KnownNtfsFileRecordNumber, Ntfs, NtfsAttributeType};
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::os::windows::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::{ExtShare, Node};

const FILE_SHARE_READ: u32 = 0x0000_0001;
const FILE_SHARE_WRITE: u32 = 0x0000_0002;
const FILE_FLAG_SEQUENTIAL_SCAN: u32 = 0x0800_0000;
const GENERIC_READ: u32 = 0x8000_0000;
const NTFS_BLOCK_SIZE: usize = 512;
/// 快路径每解析多少条记录回调一次进度（对齐慢路径 50_000）。
const PROGRESS_EVERY: u64 = 50_000;

/// 属性类型常量。
const ATTR_FILE_NAME: u32 = 0x30;
const ATTR_DATA: u32 = 0x80;

/// Windows 卷设备（`\\.\C:`）的 `ReadFile` 要求文件偏移与读长度都按
/// 逻辑扇区（512 B）对齐，非对齐小读返回 `ERROR_INVALID_PARAMETER`(87)。
/// 而 `ntfs` crate 的 `Ntfs::new` 用 binrw 逐字段（3/7 字节）读引导扇区，
/// 在裸卷上必然触发 os error 87。本读取器把所有底层读聚合为 512 B 对齐：
/// 对齐大读（长度与偏移均为 512 倍数）直接分块下发；小读/非对齐读经
/// 64 KiB 对齐缓冲区拷贝，绝不对卷设备发出非对齐小系统调用。
const VOL_BUF_CAP: usize = 64 * 1024;

struct AlignedVolumeReader<R: Read + Seek> {
    inner: R,
    /// 缓冲区内已有效区间 [buf_start, buf_end)（绝对文件偏移）。
    buf_start: u64,
    buf_end: u64,
    buf: Box<[u8]>,
    pos: u64,
}

impl<R: Read + Seek> AlignedVolumeReader<R> {
    fn new(inner: R) -> Self {
        Self {
            inner,
            buf_start: 0,
            buf_end: 0,
            buf: vec![0u8; VOL_BUF_CAP].into_boxed_slice(),
            pos: 0,
        }
    }

    /// 把 pos 对齐扇区起始处重新加载最多一缓冲区的数据。
    fn fill(&mut self, want: usize) -> std::io::Result<usize> {
        let in_buf = (self.buf_end - self.buf_start) as usize;
        if in_buf >= want {
            return Ok(in_buf);
        }
        let start = self.pos & !(NTFS_BLOCK_SIZE as u64 - 1);
        self.inner.seek(SeekFrom::Start(start))?;
        self.buf_start = start;
        let mut have = 0usize;
        while have < self.buf.len() {
            match self.inner.read(&mut self.buf[have..]) {
                Ok(0) => break,
                Ok(n) => have += n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            }
        }
        self.buf_end = start + have as u64;
        Ok(have)
    }

    fn read_some(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        // 先服务缓冲区内剩余数据（若有）。
        let off = (self.pos - self.buf_start) as usize;
        if self.buf_start <= self.pos && off < (self.buf_end - self.buf_start) as usize {
            let in_buf = (self.buf_end - self.buf_start) as usize;
            let n = (in_buf - off).min(out.len());
            out[..n].copy_from_slice(&self.buf[off..off + n]);
            self.pos += n as u64;
            return Ok(n);
        }
        // 对齐大读：偏移与长度都是 512 倍数 → 直接分块下发，无缓冲拷贝。
        if self.pos % NTFS_BLOCK_SIZE as u64 == 0
            && out.len() >= NTFS_BLOCK_SIZE
            && out.len() % NTFS_BLOCK_SIZE == 0
        {
            self.inner.seek(SeekFrom::Start(self.pos))?;
            let mut have = 0usize;
            while have < out.len() {
                match self.inner.read(&mut out[have..]) {
                    Ok(0) => break,
                    Ok(n) => have += n,
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(e) => return Err(e),
                }
            }
            self.pos += have as u64;
            return Ok(have);
        }
        // 小读/非对齐：经对齐缓冲。
        let want = out.len().min(self.buf.len()).max(1);
        let have = self.fill(want)?;
        if have == 0 {
            return Ok(0);
        }
        let off = (self.pos - self.buf_start) as usize;
        let n = (have - off).min(out.len());
        out[..n].copy_from_slice(&self.buf[off..off + n]);
        self.pos += n as u64;
        Ok(n)
    }
}

impl<R: Read + Seek> Read for AlignedVolumeReader<R> {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        self.read_some(out)
    }
}

impl<R: Read + Seek> Seek for AlignedVolumeReader<R> {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        let target: i128 = match pos {
            SeekFrom::Start(p) => p as i128,
            SeekFrom::Current(d) => self.pos as i128 + d as i128,
            SeekFrom::End(d) => {
                let size = self.inner.seek(SeekFrom::End(0))?;
                size as i128 + d as i128
            }
        };
        let target = u64::try_from(target.max(0))
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "negative seek"))?;
        // 目标不在当前缓冲覆盖范围 → 作废缓冲，fill() 会按新对齐位置重载。
        // 目标恰在缓冲内则保留缓冲（快路径的页式访问依赖缓存复用）。
        if target < self.buf_start || target >= self.buf_end {
            self.buf_start = 0;
            self.buf_end = 0;
        }
        self.pos = target;
        Ok(target)
    }
}

#[derive(Debug, Clone)]
struct Entry {
    name: String,
    parent_frn: u64,
    own_size: u64, // file size from $DATA, or 0 for dirs
    is_dir: bool,
    children: Vec<u64>, // FRNs of direct children (filled in second pass)
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
    F: FnMut(u64, u64), // (records_seen, bytes_seen)
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
        on_progress(entries.len() as u64, bytes_total);
        link_children(&mut entries);
        return Ok(build_node_tree(
            volume_letter,
            subroot,
            keep_files,
            max_dirs,
            &entries,
            bytes_total,
        ));
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

/// 快路径：一次性把 $MFT 读进内存并按偏移解析每条记录。
///
/// 返回 `(entries, bytes_total)`。`entries` 以 FRN 为键、含 parent/name/size/is_dir，
/// 尚未做 children 链接（调用方负责）。任何一步不一致都返回 Err 触发回退。
fn parse_mft_fast<R: Read + Seek, F: FnMut(u64, u64)>(
    reader: &mut R,
    ntfs: &Ntfs,
    volume_letter: char,
    record_size: usize,
    on_progress: &mut F,
) -> anyhow::Result<(HashMap<u64, Entry>, u64)> {
    // 定位 $MFT 的 $DATA 属性值。`NtfsFile` 只借用 `ntfs`（不借用 reader），
    // 因此拿到 value 后可以把 reader 独占给 `attach().read_to_end()`。
    let mft_file = ntfs
        .file(reader, KnownNtfsFileRecordNumber::MFT as u64)
        .context("locating $MFT")?;
    let mft_data = mft_file
        .data(reader, "")
        .ok_or_else(|| anyhow!("$MFT has no $DATA attribute"))?
        .context("reading $MFT $DATA")?;
    let mft_data_attribute = mft_data.to_attribute()?;
    let mft_data_value = mft_data_attribute.value(reader)?;
    let total_size = mft_data_value.len() as usize;

    // 顺序读完整块 $MFT（attach 的 read 会按 data run 逐个 seek+read，
    // run 通常只有一两个，所以是几次系统调用，而非每条记录几次）。
    let mut mft_bytes: Vec<u8> = Vec::with_capacity(total_size);
    mft_data_value
        .attach(reader)
        .read_to_end(&mut mft_bytes)
        .context("reading $MFT into memory")?;
    if mft_bytes.len() < record_size {
        return Err(anyhow!("$MFT too small ({} bytes)", mft_bytes.len()));
    }

    // 全量 USN fixup：每条记录内按 512 字节扇区修正末 2 字节。
    // fixup 需要可变缓冲；$FILE_NAME / $DATA 的长度都取自驻留属性头，
    // 与 fixup 后的数据无关（fixup 只改 sector 尾部冗余字节）。
    fixup_all(&mut mft_bytes, record_size)?;

    // 内存解析每条记录。
    let mut entries: HashMap<u64, Entry> = HashMap::new();
    let mut bytes_total: u64 = 0;
    let total_records = (mft_bytes.len() / record_size) as u64;
    let mut rec_num: u64 = 0;

    for chunk in mft_bytes.chunks_exact(record_size) {
        if let Some((parent_frn, name, own_size, is_dir)) = parse_record(chunk) {
            bytes_total = bytes_total.saturating_add(own_size);
            entries.insert(
                rec_num,
                Entry {
                    name,
                    parent_frn,
                    own_size,
                    is_dir,
                    children: Vec::new(),
                },
            );
        }
        rec_num += 1;
        if rec_num.is_multiple_of(PROGRESS_EVERY) {
            on_progress(rec_num, bytes_total);
            tracing::trace!("MFT fast path progress: {rec_num}/{}", total_records);
        }
    }
    let _ = volume_letter;
    // 整块 $MFT 已在上面解析完（entries 已全部拷出），这里显式释放原始
    // 字节缓冲，避免它与后续 link_children / build_node_tree 的 entries +
    // Node 树叠加占用峰值内存（大卷 $MFT 可达 1-2GB）。
    drop(mft_bytes);
    // 与慢路径一致：末尾补一次 100% 回调。
    on_progress(total_records, bytes_total);
    Ok((entries, bytes_total))
}

/// 全量 USN fixup。每条记录：USA 数组从 `usa_offset+2` 起，每项 2 字节；
/// 依次修正第 2..n 个 512 字节扇区末尾的 2 字节（这些位置写入的是 USN）。
fn fixup_all(buf: &mut [u8], record_size: usize) -> anyhow::Result<()> {
    let mut off = 0usize;
    while off + 8 <= buf.len() {
        let rec_len = buf.len().min(off + record_size);
        // USA 偏移/数量为 0 或越界 → 记录非法或为空，跳过该条。
        let usa_offset = u16::from_le_bytes([buf[off + 4], buf[off + 5]]) as usize;
        let usa_count = u16::from_le_bytes([buf[off + 6], buf[off + 7]]) as usize;
        if usa_offset >= 4 && usa_count >= 2 {
            let usa_end = usa_offset + usa_count * 2;
            if usa_end <= rec_len - off {
                let usn = [buf[off + usa_offset], buf[off + usa_offset + 1]];
                let mut sector_pos = off + NTFS_BLOCK_SIZE - 2;
                let mut arr_pos = usa_offset + 2;
                while arr_pos < usa_end && sector_pos + 2 <= rec_len {
                    // 只有当前位置确实是 USN 才替换（防止误改真实数据）。
                    if buf[sector_pos..sector_pos + 2] == usn {
                        buf[sector_pos] = buf[off + arr_pos];
                        buf[sector_pos + 1] = buf[off + arr_pos + 1];
                    }
                    arr_pos += 2;
                    sector_pos += NTFS_BLOCK_SIZE;
                }
            }
        }
        off += record_size;
    }
    Ok(())
}

/// 内存解析一条 FILE 记录。返回 `(parent_frn, name, own_size, is_dir)`；
/// 记录无效（非 FILE / 无 $FILE_NAME）时返回 None。
fn parse_record(rec: &[u8]) -> Option<(u64, String, u64, bool)> {
    if rec.len() < 56 || &rec[0..4] != b"FILE" {
        return None;
    }
    // FileRecordHeader 布局（相对记录起点，`RecordHeader` + FileRecordHeader 其余字段）：
    //   0 signature(4)
    //   4 usa_offset(2)  6 usa_count(2)  8 lsn(8)
    //  16 sequence(2) 18 link_count(2) 20 attrs_offset(2) 22 flags(2)
    //  24 used_size(4) 28 alloc_size(4) 32 base_ref(8) 40 next_attr_id(2) 42 align(2)
    //  44 record_number(4，NTFS 3.1+)
    // 参考 ntfs crate `FileRecordHeader`：
    //   <https://flatcap.github.io/linux-ntfs/ntfs/concepts/file_record.html>
    let attrs_offset = u16::from_le_bytes([rec[20], rec[21]]) as usize;
    let flags = u16::from_le_bytes([rec[22], rec[23]]);
    let is_dir = flags & 0x0002 != 0;
    let used_size = u32::from_le_bytes([rec[24], rec[25], rec[26], rec[27]]) as usize;
    if attrs_offset == 0 || attrs_offset >= rec.len() {
        return None;
    }
    let attrs_end = used_size.min(rec.len());

    let mut best_name: Option<(u64, String)> = None;
    let mut best_rank: u8 = 0; // 越大越适合显示；Dos 8.3 短名排除
    let mut size: u64 = 0;
    let mut off = attrs_offset;

    while off + 8 <= attrs_end {
        // NtfsAttributeHeader（ntfs crate attribute.rs）：
        //   0 ty(4)  4 length(4)  8 is_non_resident(1)  9 name_length(1)
        //  10 name_offset(2) 12 flags(2) 14 instance(2)  16 (res 头续 / non-res 头续)
        // Resident 头续（NtfsResidentAttributeHeader，offset 16 起）：
        //  16 value_length(4) 20 value_offset(2) 22 indexed_flag(1) 23 pad(1)
        // Non-resident 头续（NtfsNonResidentAttributeHeader）：
        //  16 lowest_vcn(8) 24 highest_vcn(8) 32 data_runs_offset(2) 34 comp_unit(1)
        //  35 reserved(5) 40 allocated_size(8) 48 data_size(8) 56 initialized_size(8)
        let alen =
            u32::from_le_bytes([rec[off + 4], rec[off + 5], rec[off + 6], rec[off + 7]]) as usize;
        if alen < 24 || off + alen > attrs_end {
            break;
        }
        let atype = u32::from_le_bytes([rec[off], rec[off + 1], rec[off + 2], rec[off + 3]]);
        let non_resident = rec[off + 8] != 0;
        let name_len = rec[off + 9] as usize;

        match atype {
            ATTR_FILE_NAME => {
                if non_resident {
                    // $FILE_NAME 理论上可以非驻留（极长文件名），罕见；跳过即可，
                    // 用下一条 Win32/Posix 名字。
                    off += alen;
                    continue;
                }
                // Resident value 的位置：属性起始 + value_offset。`value_offset`
                // 已经包含属性头 + 属性自身名字的长度（crate attribute.rs 的
                // resident_value() 就是 `attr.offset + value_offset`），不要再加 name_len*2。
                let value_offset = u16::from_le_bytes([rec[off + 20], rec[off + 21]]) as usize;
                let value_len = u32::from_le_bytes([
                    rec[off + 16],
                    rec[off + 17],
                    rec[off + 18],
                    rec[off + 19],
                ]) as usize;
                let name_off = off + value_offset;
                // FileNameHeader 定长部分至少 66 字节（FILE_NAME_HEADER_SIZE）。
                if name_off + 66 > rec.len() || name_off + value_len > rec.len() {
                    off += alen;
                    continue;
                }
                // NtfsFileName 布局（FileNameHeader，value 内，file_name.rs：
                //   FILE_NAME_HEADER_SIZE=66 已由 crate 源码确认）：
                //   0 parent_directory_reference(8，低 48 位是 FRN)
                //   8..48 四个时间戳(40)  48 allocated_size(8)  56 data_size(8)
                //  56 file_attributes(4) 60 reparse_point_tag(4) 64 name_length(1)
                //  65 namespace(1)  66 name(2*name_length)
                // 名字从 value 起点偏移 66 开始。
                let parent_frn =
                    u64::from_le_bytes(rec[name_off..name_off + 8].try_into().unwrap())
                        & 0x0000_FFFF_FFFF_FFFF;
                let nlen = rec[name_off + 64] as usize;
                if name_off + 66 + nlen * 2 > rec.len() {
                    off += alen;
                    continue;
                }
                let ns = rec[name_off + 65];
                let name_bytes = &rec[name_off + 66..name_off + 66 + nlen * 2];
                let name = String::from_utf16_lossy(
                    &name_bytes
                        .chunks_exact(2)
                        .map(|c| u16::from_le_bytes([c[0], c[1]]))
                        .collect::<Vec<_>>(),
                );
                // namespace：0=Posix 1=Win32 2=Dos 3=Win32AndDos（crate 源码 enum）。
                // rank 越大越适合显示：Win32AndDos(3)>Win32(1)>Posix(0)；
                // Dos 8.3 短名(2)直接不参与——它会覆盖完整长名。
                let rank = match ns {
                    3 => 3, // Win32AndDos
                    1 => 2, // Win32
                    0 => 1, // Posix
                    _ => 0, // Dos 8.3 短名，跳过
                };
                if rank != 0 {
                    // `best_rank` 只有 1..3；首条合法名字 rank>=1 成立，宽容重复名后续覆盖。
                    if rank > best_rank {
                        best_rank = rank;
                        best_name = Some((parent_frn, name));
                    }
                }
            }
            ATTR_DATA => {
                if name_len != 0 {
                    // 只取未命名 $DATA（文件主数据流）；命名流（Zone.Identifier 等）不算。
                    off += alen;
                    continue;
                }
                let vlen = if non_resident {
                    // 非驻留：data_size 在属性起始 + 48（8 字节）。
                    if off + 56 <= attrs_end {
                        u64::from_le_bytes(rec[off + 48..off + 56].try_into().unwrap())
                    } else {
                        0
                    }
                } else {
                    // 驻留：value_length 在属性起始 + 16（4 字节）。
                    u32::from_le_bytes([rec[off + 16], rec[off + 17], rec[off + 18], rec[off + 19]])
                        as u64
                };
                if size == 0 {
                    size = vlen;
                }
            }
            _ => {}
        }
        off += alen;
    }

    best_name.map(|(p, n)| (p, n, if is_dir { 0 } else { size }, is_dir))
}

/// 第二遍：把子 FRN 挂到父 FRN 的 children 上。
fn link_children(entries: &mut HashMap<u64, Entry>) {
    let frns: Vec<u64> = entries.keys().copied().collect();
    for frn in frns {
        let parent = entries.get(&frn).map(|e| e.parent_frn).unwrap_or(0);
        if parent != frn {
            if let Some(p) = entries.get_mut(&parent) {
                p.children.push(frn);
            }
        }
    }
}

/// DFS 汇总每个目录的总大小与文件数（快/慢路径共用，带环保护）。
fn rollup(
    frn: u64,
    entries: &HashMap<u64, Entry>,
    sizes: &mut HashMap<u64, (u64, u64)>,
) -> (u64, u64) {
    if let Some(c) = sizes.get(&frn) {
        return *c;
    }
    // Cycle guard.
    sizes.insert(frn, (0, 0));
    let mut total_bytes: u64 = 0;
    let mut total_files: u64 = 0;
    if let Some(e) = entries.get(&frn) {
        if !e.is_dir {
            total_bytes = e.own_size;
            total_files = 1;
        } else {
            for &c in &e.children {
                if c == frn {
                    continue;
                }
                if let Some(centry) = entries.get(&c) {
                    if centry.is_dir
                        && super::is_pruned_system_dir_at(
                            std::ffi::OsStr::new(&e.name),
                            std::ffi::OsStr::new(&centry.name),
                        )
                    {
                        continue;
                    }
                    if !centry.is_dir && super::is_pruned_system_file(&centry.name) {
                        continue;
                    }
                }
                let (b, n) = rollup(c, entries, sizes);
                total_bytes = total_bytes.saturating_add(b);
                total_files = total_files.saturating_add(n);
            }
        }
    }
    sizes.insert(frn, (total_bytes, total_files));
    (total_bytes, total_files)
}

/// 构建可见 Node 树（快/慢路径共用）。`bytes_total` 仅用于日志/统计。
fn build_node_tree(
    volume_letter: char,
    subroot: Option<&Path>,
    keep_files: Option<usize>,
    max_dirs: Option<usize>,
    entries: &HashMap<u64, Entry>,
    bytes_total: u64,
) -> Node {
    let root_frn = KnownNtfsFileRecordNumber::RootDirectory as u64;
    let volume_root = format!("{}:\\", volume_letter.to_ascii_uppercase());

    let mut sizes: HashMap<u64, (u64, u64)> = HashMap::new();
    rollup(root_frn, entries, &mut sizes);

    let start_frn = if let Some(sub) = subroot {
        find_frn_for_path(sub, root_frn, entries).unwrap_or(root_frn)
    } else {
        root_frn
    };
    let start_path = if let Some(sub) = subroot {
        sub.to_path_buf()
    } else {
        PathBuf::from(&volume_root)
    };

    let node = build_node(start_frn, &start_path, entries, &sizes, keep_files, max_dirs);
    let _ = bytes_total;
    node
}

/// 慢路径（原实现）：逐条 `ntfs::file()` + seek。保留作快路径失败的回退。
fn scan_volume_slow<R, F>(
    reader: &mut R,
    ntfs: &Ntfs,
    volume_letter: char,
    subroot: Option<&Path>,
    keep_files: Option<usize>,
    max_dirs: Option<usize>,
    record_size: usize,
    mut on_progress: F,
) -> anyhow::Result<Node>
where
    R: Read + Seek,
    F: FnMut(u64, u64), // (records_seen, bytes_seen)
{
    // 定位 $MFT，拿总记录数。
    let mft_file = ntfs.file(reader, KnownNtfsFileRecordNumber::MFT as u64)?;
    let mft_data = mft_file
        .data(reader, "")
        .ok_or_else(|| anyhow!("$MFT has no $DATA attribute"))?
        .context("reading $MFT $DATA")?;
    let mft_data_attribute = mft_data.to_attribute()?;
    let mft_data_value = mft_data_attribute.value(reader)?;
    let total_size = mft_data_value.len();
    let total_records = (total_size / record_size as u64) as u64;
    let _ = mft_data_value;
    let _ = mft_data_attribute;
    let _ = mft_file;

    // Pass 1: collect all entries by FRN.
    let mut entries: HashMap<u64, Entry> = HashMap::with_capacity(total_records as usize);
    let mut bytes_total: u64 = 0;

    for record_num in 0..total_records {
        let f = match ntfs.file(reader, record_num) {
            Ok(f) => f,
            Err(_) => continue,
        };

        let mut best_name: Option<NtfsFileName> = None;
        let mut best_namespace_rank: u8 = 0;
        let mut size: u64 = 0;
        let is_dir = f.is_directory();

        let mut attrs_iter = f.attributes();
        while let Some(item) = attrs_iter.next(reader) {
            let item = match item {
                Ok(a) => a,
                Err(_) => continue,
            };
            let attr = match item.to_attribute() {
                Ok(a) => a,
                Err(_) => continue,
            };
            match attr.ty() {
                Ok(NtfsAttributeType::FileName) => {
                    if let Ok(name) = attr.structured_value::<_, NtfsFileName>(reader) {
                        let rank = match name.namespace() {
                            NtfsFileNamespace::Posix => 4,
                            NtfsFileNamespace::Win32 => 3,
                            NtfsFileNamespace::Win32AndDos => 2,
                            NtfsFileNamespace::Dos => 1,
                        };
                        if rank > best_namespace_rank {
                            best_namespace_rank = rank;
                            best_name = Some(name);
                        }
                    }
                }
                Ok(NtfsAttributeType::Data) => {
                    if size != 0 {
                        continue;
                    }
                    let unnamed = attr.name().map(|n| n.is_empty()).unwrap_or(true);
                    if !unnamed {
                        continue;
                    }
                    size = attr.value_length();
                }
                _ => {}
            }
        }

        if let Some(fname) = best_name {
            let parent_frn = fname.parent_directory_reference().file_record_number();
            let name = fname.name().to_string_lossy();
            let own_size = if is_dir { 0 } else { size };
            bytes_total = bytes_total.saturating_add(own_size);
            entries.insert(
                record_num,
                Entry {
                    name,
                    parent_frn,
                    own_size,
                    is_dir,
                    children: Vec::new(),
                },
            );
        }

        if record_num.is_multiple_of(PROGRESS_EVERY) {
            on_progress(record_num, bytes_total);
        }
    }
    on_progress(total_records, bytes_total);

    // 公共的 children 链接 + 树构建。
    link_children(&mut entries);
    Ok(build_node_tree(
        volume_letter,
        subroot,
        keep_files,
        max_dirs,
        &entries,
        bytes_total,
    ))
}

fn find_frn_for_path(target: &Path, root_frn: u64, entries: &HashMap<u64, Entry>) -> Option<u64> {
    // Walk component-by-component from root looking for matching child names.
    let mut current = root_frn;
    let comps: Vec<String> = target
        .components()
        .filter_map(|c| match c {
            std::path::Component::Normal(s) => Some(s.to_string_lossy().to_string()),
            _ => None,
        })
        .collect();
    for c in comps {
        let e = entries.get(&current)?;
        let next = e.children.iter().copied().find(|frn| {
            entries
                .get(frn)
                .map(|x| x.name.eq_ignore_ascii_case(&c))
                .unwrap_or(false)
        })?;
        current = next;
    }
    Some(current)
}

fn build_node(
    frn: u64,
    path: &Path,
    entries: &HashMap<u64, Entry>,
    sizes: &HashMap<u64, (u64, u64)>,
    keep_files: Option<usize>,
    max_dirs: Option<usize>,
) -> Node {
    let entry = entries.get(&frn);
    let (size, file_count) = sizes.get(&frn).copied().unwrap_or((0, 0));
    let name = match entry {
        Some(e) => e.name.clone(),
        None => path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string_lossy().to_string()),
    };
    let is_dir = entry.map(|e| e.is_dir).unwrap_or(true);

    let mut children_nodes: Vec<Node> = Vec::new();
    let mut children_truncated: Option<u64> = None;
    let mut ext_bytes: HashMap<String, u64> = HashMap::new();
    let mut ext_count: HashMap<String, u64> = HashMap::new();

    if let Some(e) = entry {
        // Sort children by size desc for nicer display.
        let mut kids: Vec<(u64, u64)> = e
            .children
            .iter()
            .filter(|&&c| c != frn)
            .map(|&c| (c, sizes.get(&c).map(|(b, _)| *b).unwrap_or(0)))
            .collect();
        kids.sort_by_key(|k| std::cmp::Reverse(k.1));

        // 对齐 walkdir 模式的树形态：目录全部保留（系统目录在下方剪枝），
        // 文件按大小取 top-K（keep_files）。此前这里用 depth 分级 breadth_cap
        // （200/80/25）把文件和目录混在一起截断——深层目录超过 25 个孩子时
        // 大部分子目录根本不在树里，前端无法展开查看。kids 已按大小降序，
        // 文件 top-K = 顺序取前 keep 个。
        let keep = keep_files.unwrap_or(usize::MAX);
        let mut files_kept = 0usize;
        for (cfrn, _) in kids {
            let centry = match entries.get(&cfrn) {
                Some(c) => c,
                None => continue,
            };
            if centry.is_dir
                && super::is_pruned_system_dir_at(
                    std::ffi::OsStr::new(&e.name),
                    std::ffi::OsStr::new(&centry.name),
                )
            {
                continue;
            }
            if !centry.is_dir && super::is_pruned_system_file(&centry.name) {
                continue;
            }
            let cpath = path.join(&centry.name);

            // 扩展名统计覆盖全部直系文件（不受 top-K 截断影响；此前被
            // breadth_cap 截过，大目录的 top_extensions 偏小）。
            if !centry.is_dir {
                let ext = std::path::Path::new(&centry.name)
                    .extension()
                    .map(|e| e.to_string_lossy().to_lowercase())
                    .unwrap_or_else(|| "(none)".into());
                // get_mut 借用更新零复制；每个目录首次遇到某后缀才 clone 入桶
                if let Some(b) = ext_bytes.get_mut(&ext) {
                    *b += centry.own_size;
                } else {
                    ext_bytes.insert(ext.clone(), centry.own_size);
                }
                if let Some(c) = ext_count.get_mut(&ext) {
                    *c += 1;
                } else {
                    ext_count.insert(ext, 1);
                }
                if files_kept >= keep {
                    continue;
                }
                files_kept += 1;
            }

            let cnode = build_node(cfrn, &cpath, entries, sizes, keep_files, max_dirs);
            children_nodes.push(cnode);
        }
    }

    // 目录广度上限（max_dirs）与文件 top-K 对称：超过上限的层按 size 保留
    // 最大的 max_dirs 个子目录，并标记 children_truncated（截断前目录总数，
    // 与 walkdir 侧/深度 cap 同口径——前端徽标 = children_truncated - children）
    // 供 +N 按需取回（tree_subtree）。children_nodes 已按 size 降序（kids
    // 排序保证），retain 保序保留前 limit 个目录节点 = 保留最大的 limit 个
    // 目录；文件节点不受影响。这把整盘扫描树的内存钉在有界范围内。
    if let Some(limit) = max_dirs {
        let dir_count = children_nodes.iter().filter(|c| c.is_dir).count();
        if dir_count > limit {
            let mut dirs_seen = 0usize;
            children_nodes.retain(|c| {
                if c.is_dir {
                    dirs_seen += 1;
                    dirs_seen <= limit
                } else {
                    true
                }
            });
            children_truncated = Some(dir_count as u64);
        }
    }

    let mut top_extensions: Vec<ExtShare> = ext_bytes
        .into_iter()
        .map(|(ext, bytes)| ExtShare {
            ext: ext.clone(),
            bytes,
            count: ext_count.get(&ext).copied().unwrap_or(0),
        })
        .collect();
    top_extensions.sort_by_key(|e| std::cmp::Reverse(e.bytes));
    top_extensions.truncate(8);

    Node {
        name,
        path: path.to_string_lossy().to_string(),
        is_dir,
        size,
        file_count,
        children: children_nodes,
        scaffold_id: None,
        top_extensions,
        children_truncated,
        mtime: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 构造一条合成 FILE 记录：
    ///   - FILE 签名 + USA fixup 数组（4 个扇区的记录 = 4 项，USA=0xABCD）
    ///   - 一个驻留 $FILE_NAME（Win32AndDos，含 parent FRN + 长度）
    ///   - 一个非驻留 $DATA（data_size=2048）
    /// 返回 (record, usa_array, record_size)。USA 数组单独返回，方便测试
    /// 把 sector 尾部填成 USN 后再合成完整记录。
    fn synth_record() -> (Vec<u8>, Vec<u8>) {
        let record_size = 1024usize; // 2 个 512 扇区 → USA 共 2 项
        let mut rec = vec![0u8; record_size];
        rec[0..4].copy_from_slice(b"FILE");
        // usa_offset=0x30, usa_count=2（2 项：USN + 1 个扇区修正值）
        rec[4..6].copy_from_slice(&0x30u16.to_le_bytes());
        rec[6..8].copy_from_slice(&2u16.to_le_bytes());
        // attrs_offset=0x38（USA 数组 0x30..0x34 之后）
        rec[20..22].copy_from_slice(&0x38u16.to_le_bytes());
        // flags：0x0000 = 普通文件（测试 size 提取；目录标志另有用例）
        rec[22..24].copy_from_slice(&0x0000u16.to_le_bytes());
        let attrs_off = 0x38usize;

        // ---- 驻留 $FILE_NAME 属性 ----
        let fname_attr = attrs_off;
        // ty=0x30, 头长 24 字节 + value 74 字节（FileNameHeader 66 + 名字 4 字符） = 98
        let fname_alen = 24 + 66 + 8usize;
        rec[fname_attr..fname_attr + 4].copy_from_slice(&0x30u32.to_le_bytes());
        rec[fname_attr + 4..fname_attr + 8].copy_from_slice(&(fname_alen as u32).to_le_bytes());
        rec[fname_attr + 8] = 0; // resident
        rec[fname_attr + 9] = 0; // name_length=0
        rec[fname_attr + 16..fname_attr + 20].copy_from_slice(&(66u32 + 8).to_le_bytes()); // value_length
        rec[fname_attr + 20..fname_attr + 22].copy_from_slice(&24u16.to_le_bytes()); // value_offset

        let value_off = fname_attr + 24;
        // parent_directory_reference = 5（低 48 位 FRN）
        rec[value_off..value_off + 8].copy_from_slice(&5u64.to_le_bytes());
        // name_length = 2（UTF-16 码元数；crate 的 name_length() 会 *2。4 字节名字）
        rec[value_off + 64] = 2;
        rec[value_off + 65] = 3;
        // name = "报告"（U+62A5 U+544A，UTF-16LE：低字节在前。0x62A5→[A5,62]，0x544A→[4A,54]）
        let name_utf16: Vec<u8> = vec![0xA5, 0x62, 0x4A, 0x54];
        rec[value_off + 66..value_off + 66 + 4].copy_from_slice(&name_utf16);

        // ---- 非驻留 $DATA（未命名）----
        let data_attr = fname_attr + fname_alen;
        rec[data_attr..data_attr + 4].copy_from_slice(&0x80u32.to_le_bytes());
        let data_alen = 64usize;
        rec[data_attr + 4..data_attr + 8].copy_from_slice(&(data_alen as u32).to_le_bytes());
        rec[data_attr + 8] = 1; // non-resident
        rec[data_attr + 9] = 0; // name_length=0
        rec[data_attr + 48..data_attr + 56].copy_from_slice(&2048u64.to_le_bytes()); // data_size

        // ---- USA 数组：USN + 每扇区末 2 字节的原始值 ----
        // 1024 字节记录 = 2 个 512 扇区 → usa_count = 扇区数 + 1 = 3
        //（crate 的 fixup() 从 offset 510 起逐扇区修正，与真实记录 30 00 03 00 一致）
        let usa: Vec<u8> = vec![0xAB, 0xCD, 0x11, 0x22, 0x33, 0x44];
        rec[6..8].copy_from_slice(&3u16.to_le_bytes()); // usa_count
        rec[0x30..0x36].copy_from_slice(&usa);
        // 每个扇区末 2 字节先写入 USN（真实盘上的格式）
        rec[NTFS_BLOCK_SIZE - 2..NTFS_BLOCK_SIZE].copy_from_slice(&usa[0..2]);
        rec[2 * NTFS_BLOCK_SIZE - 2..2 * NTFS_BLOCK_SIZE].copy_from_slice(&usa[0..2]);
        // used_size（u32）
        rec[24..28].copy_from_slice(&(data_attr as u32 + data_alen as u32).to_le_bytes());
        (rec, usa)
    }

    #[test]
    fn parse_record_extracts_name_size_dir() {
        let (rec, _) = synth_record();
        let r = parse_record(&rec).expect("valid record parses");
        assert_eq!(r.0, 5, "parent FRN");
        assert_eq!(r.1, "报告", "Win32AndDos name decodes as UTF-16LE");
        assert_eq!(r.2, 2048, "non-resident $DATA data_size");
        assert!(!r.3, "file record is not a directory");
    }

    #[test]
    fn parse_record_marks_directory_flag() {
        let (mut rec, _) = synth_record();
        rec[22..24].copy_from_slice(&0x0002u16.to_le_bytes()); // directory
        let r = parse_record(&rec).expect("valid record parses");
        assert!(r.3, "directory flag set");
        assert_eq!(r.2, 0, "directories report zero own size");
    }

    #[test]
    fn parse_record_rejects_non_file() {
        let mut rec = vec![0u8; 1024];
        rec[0..4].copy_from_slice(b"BAAD");
        assert!(parse_record(&rec).is_none());
        assert!(parse_record(&[]).is_none());
    }

    #[test]
    fn parse_record_skips_dos_short_name() {
        // 只有 Dos(2) 名字的记录 → 无可用名字，返回 None。
        let (mut rec, _) = synth_record();
        let value_off = 0x38 + 24;
        rec[value_off + 65] = 2; // namespace → Dos
        assert!(parse_record(&rec).is_none());
    }

    #[test]
    fn fixup_all_restores_sector_tails() {
        let (mut rec, usa) = synth_record();
        let usa_arr_pos = 0x30usize;
        // fixup_all 前：sector 末 2 字节是 USN(0xABCD)。
        assert_eq!(&rec[NTFS_BLOCK_SIZE - 2..NTFS_BLOCK_SIZE], &[0xAB, 0xCD]);
        // USA 数组第 2/3 项是各扇区末 2 字节的原始数据——fixup_all 应还原。
        // synth_record 已把 0x33 0x44 写进第 3 项（usa[4..6]）。
        fixup_all(&mut rec, 1024).unwrap();
        // 两个扇区末 2 字节都还原成 USA 数组里的原始值。
        assert_eq!(&rec[NTFS_BLOCK_SIZE - 2..NTFS_BLOCK_SIZE], &usa[2..4]);
        assert_eq!(
            &rec[2 * NTFS_BLOCK_SIZE - 2..2 * NTFS_BLOCK_SIZE],
            &usa[4..6]
        );
        // USA 数组本身不被改动。
        assert_eq!(&rec[usa_arr_pos..usa_arr_pos + 2], &[0xAB, 0xCD]);
    }
}

/// 真实卷快/慢路径计时对比（`cargo run -p diskpilot-scanner --features bench -- bench <盘符>`）。
///
/// 只做只读测量：不删文件、不写盘。需要管理员权限打开卷设备。
/// 输出每路径一次完整扫描的毫秒数（解析循环 + children 链接 + 树构建），
/// 以及各自总条数——条数不一致说明两条路径语义有偏差，慢路径为准。
#[cfg(feature = "bench")]
pub mod bench {
    use super::*;

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
            &mut |_, _| {},
        )?;
        let fast_ms = t0.elapsed().as_millis();
        link_children(&mut entries_fast);
        let node_fast = build_node_tree(volume_letter, None, None, None, &entries_fast, bytes_fast);

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
            |_, _| {},
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
