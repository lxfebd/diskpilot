//! mft.rs 单元测试：合成 FILE 记录解析 / fixup_all 扇区尾部还原。
//! 从 mft.rs 拆分（2026-10-12）。

use super::*;
use crate::mft_parse::{fixup_all, parse_record};

/// 构造一条合成 FILE 记录：
///   - FILE 签名 + USA fixup 数组（4 个扇区的记录 = 4 项，USA=0xABCD）
///   - 一个驻留 $FILE_NAME（Win32AndDos，含 parent FRN + 长度）
///   - 一个非驻留 $DATA（data_size=2048）
///
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
