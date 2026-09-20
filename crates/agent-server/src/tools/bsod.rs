//! 蓝屏分析（bsod.rs）：复刻 BlueScreenView 的只读部分——解析内核转储
//! `C:\Windows\Minidump\*.dmp` 与 `C:\Windows\MEMORY.DMP`，读取崩溃时间 /
//! bugcheck 代码 / 4 个参数，映射为可读的中文含义。
//!
//! **纯只读**：不修改/删除 dump 文件，只 `std::fs` 读文件头与元数据。
//!
//! ## 格式说明（Windows 内核转储 DUMP_HEADER，x64 自然对齐）
//! 无论小内存转储（256KB）还是完整内核转储，文件头都是 `_DUMP_HEADER`（`PAGEDUMP`
//! 签名），bugcheck 信息位于固定偏移：
//! - `0x00` Signature = b"PAGE"（`PAGEDUMP` 前 4 字节）
//! - `0x04` ValidDump = b"DUMP"
//! - `0xDC` BugCheckCode（u32）
//! - `0xE0..0xF8` BugCheckParameter1..4（u64 × 4）
//! 崩溃时间取文件修改时间（内核不会改 dump 文件内容，mtime 即崩溃时间）。

use std::io::Read;

/// 常见 bugcheck code → 中文含义映射（覆盖典型的 90% 崩溃，见真机/社区高频项）。
const BUGCHECK_NAMES: &[(u32, &str)] = &[
    (
        0x0000_000A,
        "IRQL_NOT_LESS_OR_EQUAL：驱动在过高 IRQL 访问可分页内存，几乎总是坏驱动",
    ),
    (
        0x0000_0019,
        "BAD_POOL_HEADER：内存池头部损坏，多为驱动写越界或内存条故障",
    ),
    (
        0x0000_001A,
        "MEMORY_MANAGEMENT：内存管理严重错误，常见根因是内存条/超频（XMP）不稳定",
    ),
    (
        0x0000_001E,
        "KMODE_EXCEPTION_NOT_HANDLED：内核模式异常未处理，坏驱动或坏硬件",
    ),
    (
        0x0000_0024,
        "NTFS_FILE_SYSTEM：文件系统损坏，多为磁盘坏道/线缆/SSD 故障",
    ),
    (
        0x0000_002E,
        "DATA_BUS_ERROR：数据总线错误，内存/主板/缓存硬件问题",
    ),
    (
        0x0000_003B,
        "SYSTEM_SERVICE_EXCEPTION：系统服务异常，驱动或系统服务崩溃",
    ),
    (
        0x0000_0050,
        "PAGE_FAULT_IN_NONPAGED_AREA：请求不存在的页，坏驱动或内存故障",
    ),
    (
        0x0000_007B,
        "INACCESSIBLE_BOOT_DEVICE：无法访问启动设备，硬盘/线缆/驱动问题",
    ),
    (
        0x0000_007E,
        "SYSTEM_THREAD_EXCEPTION_NOT_HANDLED：系统线程异常未处理",
    ),
    (
        0x0000_007F,
        "UNEXPECTED_KERNEL_MODE_TRAP：意外内核陷阱，常见于 CPU 过热/超频/电压不足",
    ),
    (
        0x0000_00C2,
        "BAD_POOL_CALLER：调用方对池执行非法操作，坏驱动",
    ),
    (
        0x0000_00D1,
        "DRIVER_IRQL_NOT_LESS_OR_EQUAL：驱动带错误 IRQL 访问内存，坏驱动",
    ),
    (
        0x0000_00EF,
        "CRITICAL_PROCESS_DIED：关键系统进程意外死亡，系统文件损坏或硬件",
    ),
    (
        0x0000_00F4,
        "CRITICAL_OBJECT_TERMINATION：关键进程/线程终止，磁盘或驱动问题",
    ),
    (
        0x0000_0101,
        "CLOCK_WATCHDOG_TIMEOUT：CPU 核心在时钟中断内未响应，超频/电压不足常见",
    ),
    (
        0x0000_0109,
        "CRITICAL_STRUCTURE_CORRUPTION：内核关键结构损坏，超频/挖矿/内存故障",
    ),
    (
        0x0000_0116,
        "VIDEO_TDR_FAILURE：显卡驱动超时未响应（显示器省电/驱动崩溃/显卡过热）",
    ),
    (
        0x0000_0119,
        "VIDEO_SCHEDULER_INTERNAL_ERROR：视频调度器内部错误，显卡驱动/硬件",
    ),
    (
        0x0000_0124,
        "WHEA_UNCORRECTABLE_ERROR：硬件不可恢复错误（CPU/内存/PCIe 过热或故障）",
    ),
    (
        0x0000_0127,
        "PAGE_NOT_ZERO：应归零的页非零，坏驱动或内存问题",
    ),
    (
        0x0000_0133,
        "DPC_WATCHDOG_VIOLATION：DPC 看门狗超时，存储驱动/固件常见",
    ),
    (
        0x0000_0139,
        "KERNEL_SECURITY_CHECK_FAILURE：内核安全机制检测失败，驱动越界或损坏",
    ),
    (
        0xC000_021A,
        "STATUS_SYSTEM_PROCESS_TERMINATED：系统进程被终止，用户态服务/安全软件冲突",
    ),
];

/// 蓝屏分析：列出 Minidump 目录 + 根目录 MEMORY.DMP 的转储，解析每个的
/// bugcheck 代码与参数，映射中文含义。
///
/// `max_dumps` 最多解析几个 dump（默认 10，上限 30）；`include_memory_dmp`
/// 是否也读根目录的 MEMORY.DMP（默认 true）。
/// 无任何 dump → 返回「从未蓝屏」的正面结论，不是错误。
pub fn analyze_bsod(
    max_dumps: Option<usize>,
    include_memory_dmp: Option<bool>,
) -> Result<String, String> {
    let max_dumps = max_dumps.unwrap_or(10).clamp(1, 30);
    let with_mem_dmp = include_memory_dmp.unwrap_or(true);

    let mut files: Vec<std::path::PathBuf> = Vec::new();
    let win_dir = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into());
    let minidump_dir = format!("{win_dir}\\Minidump");

    if let Ok(entries) = std::fs::read_dir(&minidump_dir) {
        for e in entries.flatten() {
            let p = e.path();
            if p.extension()
                .map(|x| x.to_string_lossy().to_lowercase() == "dmp")
                .unwrap_or(false)
            {
                files.push(p);
            }
        }
    }
    // 根目录 MEMORY.DMP（完整内核转储，存在时通常体积大）
    if with_mem_dmp {
        let mem = std::path::Path::new(&win_dir).join("MEMORY.DMP");
        if mem.exists() {
            files.push(mem);
        }
    }
    if files.is_empty() {
        return Ok(format!(
            "✅ 未发现蓝屏转储文件（{minidump_dir} 无 .dmp，{win_dir}\\MEMORY.DMP 不存在）——这台电脑没有记录到蓝屏，或 Minidump 写入已被禁用。"
        ));
    }

    // 按修改时间倒序（最新的排前面）
    files.sort_by_key(|p| {
        std::fs::metadata(p)
            .and_then(|m| m.modified())
            .unwrap_or(std::time::UNIX_EPOCH)
    });
    files.reverse();
    files.truncate(max_dumps);

    let mut out = format!(
        "蓝屏转储分析（{minidump_dir}，共解析 {} 个，按时间倒序）：\n",
        files.len()
    );
    let mut parsed_ok = 0usize;

    for f in &files {
        let mtime = std::fs::metadata(f)
            .and_then(|m| m.modified())
            .map(|t| iso_time(t))
            .unwrap_or_else(|_| "时间未知".into());
        match parse_dump_header(f) {
            Some(h) => {
                parsed_ok += 1;
                let name = bugcheck_name(h.code);
                out.push_str(&format!(
                    "\n— {}（{}\n  大小：{}）\n",
                    f.file_name()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                    mtime,
                    fmt_bytes(f)
                ));
                out.push_str(&format!("  BugCheck 代码：0x{:08X} {}\n", h.code, name));
                out.push_str(&format!(
                    "  参数 1-4：0x{:016X} / 0x{:016X} / 0x{:016X} / 0x{:016X}\n",
                    h.p1, h.p2, h.p3, h.p4
                ));
            }
            None => {
                out.push_str(&format!(
                    "\n— {}（{}）⚠️ 不是可解析的内核转储（头部签名非 PAGEDUMP），跳过\n",
                    f.file_name()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                    mtime
                ));
            }
        }
    }

    out.push_str(&format!("\n共 {} 个可解析转储。\n", parsed_ok));
    if parsed_ok > 0 {
        out.push_str(
            "提示：单次蓝屏不足以定论，结合相邻多次崩溃的相同 bugcheck 代码找规律；如反复 0x124/0x7F 偏 CPU 超频/过热，0x1A/0x50 偏内存，0x116/0x119 偏显卡驱动。",
        );
    } else {
        out.push_str("存在 .dmp 文件但都无法解析——可能是用户态转储（MINIDUMP_HEADER）或文件损坏；用户态转储分析方法与内核转储不同。");
    }
    Ok(out)
}

/// 从 dump 文件头解析 bugcheck（失败返回 None：非内核转储/文件太短/读取失败）。
fn parse_dump_header(path: &std::path::Path) -> Option<DumpHeader> {
    let mut f = std::fs::File::open(path).ok()?;
    let mut buf = vec![0u8; 0x200];
    let mut got = 0usize;
    loop {
        let n = f.read(&mut buf[got..]).ok()?;
        if n == 0 {
            break;
        }
        got += n;
        if got >= buf.len() {
            break;
        }
    }
    if got < 0x100 {
        return None;
    }
    if &buf[0..4] != b"PAGE" || &buf[4..8] != b"DUMP" {
        return None;
    }
    Some(DumpHeader {
        code: read_u32(&buf, 0xDC),
        p1: read_u64(&buf, 0xE0),
        p2: read_u64(&buf, 0xE8),
        p3: read_u64(&buf, 0xF0),
        p4: read_u64(&buf, 0xF8),
    })
}

struct DumpHeader {
    code: u32,
    p1: u64,
    p2: u64,
    p3: u64,
    p4: u64,
}

/// bugcheck code → 中文含义（未收录的返回「未知代码」提示查微软文档）。
fn bugcheck_name(code: u32) -> &'static str {
    BUGCHECK_NAMES
        .iter()
        .find(|(c, _)| *c == code)
        .map(|(_, name)| *name)
        .unwrap_or("未收录的 bugcheck 代码（可查微软 Bug Check Code Reference）")
}

fn read_u32(buf: &[u8], off: usize) -> u32 {
    u32::from_le_bytes(buf[off..off + 4].try_into().unwrap_or([0; 4]))
}

fn read_u64(buf: &[u8], off: usize) -> u64 {
    u64::from_le_bytes(buf[off..off + 8].try_into().unwrap_or([0; 8]))
}

fn iso_time(t: std::time::SystemTime) -> String {
    let secs = t
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    // 简略本地时间（不做时区转换库，直接 UTC + 本地偏差由系统区域决定；够用）
    let days = secs.div_euclid(86400);
    let sec_of_day = secs.rem_euclid(86400);
    let (y, m, d) = civil_from_days(days);
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        y,
        m,
        d,
        sec_of_day / 3600,
        (sec_of_day % 3600) / 60,
        sec_of_day % 60
    )
}

/// 天数 → 公历日期（Howard Hinnant 算法）。
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn fmt_bytes(p: &std::path::Path) -> String {
    let len = std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
    if len >= 1024 * 1024 * 1024 {
        format!("{:.1} GB", len as f64 / 1024.0 / 1024.0 / 1024.0)
    } else if len >= 1024 * 1024 {
        format!("{:.1} MB", len as f64 / 1024.0 / 1024.0)
    } else {
        format!("{len} B")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_minidump_dir_returns_ok() {
        // 无 dump 时应返回「未发现」正面结论（含系统实际路径），而非 Err
        let r = analyze_bsod(Some(1), Some(false));
        assert!(r.is_ok(), "无 dump 目录也应返回 Ok：{:?}", r.err());
        let s = r.unwrap();
        assert!(s.contains("未发现") || s.contains(".dmp"), "{s}");
    }

    #[test]
    fn bugcheck_name_lookup() {
        assert!(bugcheck_name(0x124).contains("WHEA"));
        assert!(bugcheck_name(0x116).contains("VIDEO_TDR"));
        assert!(bugcheck_name(0xDEAD_C0DE).contains("未收录"));
    }

    #[test]
    fn civil_from_days_known_date() {
        // 2026-09-13 = days since epoch 20703（约）；用已知日期校验：1970-01-01 → (1970,1,1)
        let (y, m, d) = civil_from_days(0);
        assert_eq!((y, m, d), (1970, 1, 1));
        // 2024-01-01 = epoch + 19723 天
        let (y2, m2, d2) = civil_from_days(19723);
        assert_eq!((y2, m2, d2), (2024, 1, 1));
    }
}
