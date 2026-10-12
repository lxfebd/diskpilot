//! MFT 记录解析：parse_mft_fast / fixup_all / parse_record。
//!
//! 从 mft.rs 拆分（2026-10-12）。纯解析（无 IO），跨平台可单测。

use anyhow::{anyhow, Context};
use ntfs::{KnownNtfsFileRecordNumber, Ntfs};
use std::collections::HashMap;
use std::io::{Read, Seek};

use crate::mft::{Entry, ATTR_DATA, ATTR_FILE_NAME, NTFS_BLOCK_SIZE, PROGRESS_EVERY};

pub(crate) fn parse_mft_fast<R: Read + Seek, F: FnMut(u64, u64)>(
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
pub(crate) fn fixup_all(buf: &mut [u8], record_size: usize) -> anyhow::Result<()> {
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
pub(crate) fn parse_record(rec: &[u8]) -> Option<(u64, String, u64, bool)> {
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
                        .as_chunks::<2>()
                        .0
                        .iter()
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
