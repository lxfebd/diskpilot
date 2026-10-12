//! HWiNFO 共享内存传感器直读（Windows-only）。
//!
//! HWiNFO 在后台运行时，会把全部实时传感器（CPU/GPU/主板温度、风扇转速、
//! 电压、功耗、频率、负载、显存、磁盘温度等）发布到命名共享内存段
//! `HWiNFO_SENS_SM2`。**普通权限即可读取**——不需要 Ring0 驱动、不需要
//! 管理员、不触发 UAC，这正是「AI 非管理员下读不到 CPU 温度」的根治通道。
//!
//! ## 协议（HWiNFO SM2，官方论坛公开布局）
//!
//! ```text
//! 段头（~24-40B，随 32/64 位对齐浮动，不硬编码）：
//!   DWORD       dwSignature           "SiWH"（小端 0x57576953）
//!   DWORD       dwVersion
//!   DWORD       dwRevision
//!   __time64_t  poll_time             （64 位，偏移 12 或 16）
//!   DWORD       dwOffsetOfSensorSection
//!   DWORD       dwSizeOfSensorElement
//!   DWORD       dwNumSensorElements
//!   DWORD       dwOffsetOfReadingSection
//!   DWORD       dwSizeOfReadingElement
//!   DWORD       dwNumReadingElements
//!
//! SensorElement（一组 = 一块芯片/设备）：
//!   DWORD dwSensorID; DWORD dwSensorInst; char szSensorNameOrig[128]; char szSensorNameUser[128];
//!
//! ReadingElement（组内一条读数）：
//!   DWORD tReading; DWORD dwSensorIndex; DWORD dwReadingID;
//!   char szLabelOrig[128]; char szLabelUser[128]; char szUnit[16];
//!   double Value, ValueMin, ValueMax, ValueAvg;   // 恒为元素尾部 32 字节
//! ```
//!
//! 关键点：**元素尺寸由段头携带**（`dwSizeOf*Element`），跨编译器 padding 也稳；
//! 4 个 double 恒占元素尾部 32B，从尾部定位防 padding 破坏。
//!
//! ## 已知限制
//! - 免费版 HWiNFO 的共享内存在运行约 12 小时后停止更新，需重启 HWiNFO。
//! - HWiNFO 未运行/未启用「Shared Memory Support」→ 打不开映射 → 如实报错，
//!   调用方降级到现有 fancmd/ACPI/nvidia-smi 通道。
//! - 本模块**只读**：不写寄存器、不装驱动、不创建/修改任何映射。
//!
//! 2026-10-12 拆分：常量与类型 → `hwinfo_types`，解析管道 → `hwinfo_parse`。

mod hwinfo_parse;
mod hwinfo_types;

#[cfg(windows)]
pub(crate) use hwinfo_parse::{format_snapshot, read_full_with};
#[cfg(windows)]
pub(crate) use hwinfo_types::map_names;

// 回归测试引用的解析管道符号。
#[cfg(test)]
pub(crate) use hwinfo_parse::{cstr, parse_header, parse_readings};

#[cfg(windows)]
pub(crate) fn read_hwinfo_text() -> Result<String, String> {
    let (snap, _) = read_full_with(&map_names())?;
    Ok(format_snapshot(&snap))
}

/// 尝试读 HWiNFO 共享内存，仅返回「温度类」读数（rtype == 温度）。
///
/// 供 `collect_temperature` 优先合并：能读到 CPU/GPU/主板/磁盘温度时直接给全，
/// 非管理员下也有效（HWiNFO 普通权限即发布）。
#[cfg(windows)]
pub(crate) fn read_temperature_lines() -> Result<Vec<(String, f64)>, String> {
    let (snap, _) = read_full_with(&map_names())?;
    let mut out = Vec::new();
    for r in &snap.readings {
        if r.rtype == "温度" && (0.0..=150.0).contains(&r.value) {
            out.push((format!("{} | {}", r.group, r.label), r.value));
        }
    }
    if out.is_empty() {
        Err("HWiNFO 共享内存中无温度类读数".into())
    } else {
        Ok(out)
    }
}

/// 防未使用告警：非 Windows 时本文件保持可编译（不引入任何符号）。
#[cfg(not(windows))]
pub(crate) fn read_hwinfo_text() -> Result<String, String> {
    Err("HWiNFO 共享内存仅 Windows 可用".into())
}

#[cfg(test)]
#[cfg(windows)]
mod tests {
    use super::*;

    /// 构造一段合法的 SM2 头 + 元素，验证解析路径（不依赖真实 HWiNFO）。
    fn fake_buf() -> Vec<u8> {
        // 真实布局：头 24B（poll 偏移 16，off=24）→ 校验要求 off_s/off_r ≥ 48，
        // sensor 区 48 起 512B×2 → reading 区 1072 起 320B×2（真实元素尺寸区间内）。
        const OFF_S: usize = 48;
        const OFF_R: usize = 48 + 2 * 512;
        let mut b = vec![0u8; OFF_R + 2 * 320];
        // 签名 "SiWH" 小端
        b[0..4].copy_from_slice(&0x5757_6953u32.to_le_bytes());
        b[4..8].copy_from_slice(&2u32.to_le_bytes()); // version
        b[8..12].copy_from_slice(&1u32.to_le_bytes()); // revision
        b[16..24].copy_from_slice(&1_700_000_000i64.to_le_bytes()); // poll
        b[24..28].copy_from_slice(&(OFF_S as u32).to_le_bytes()); // off_sensors
        b[28..32].copy_from_slice(&512u32.to_le_bytes()); // size_sensor
        b[32..36].copy_from_slice(&2u32.to_le_bytes()); // num_sensors
        b[36..40].copy_from_slice(&(OFF_R as u32).to_le_bytes()); // off_readings
        b[40..44].copy_from_slice(&320u32.to_le_bytes()); // size_reading
        b[44..48].copy_from_slice(&2u32.to_le_bytes()); // num_readings

        // 组 0：CPU；组 1：GPU（user 名放偏移 8+128）
        let set_name = |b: &mut Vec<u8>, idx: usize, name: &str| {
            let base = OFF_S + idx * 512;
            for (i, ch) in name.bytes().enumerate() {
                b[base + 8 + 128 + i] = ch;
            }
        };
        set_name(&mut b, 0, "CPU");
        set_name(&mut b, 1, "GPU");

        // 读数 0：CPU 温度；读数 1：风扇（label 放 labelOrig=偏移12，doubles 恒在尾部 32B）
        let wr = |b: &mut Vec<u8>, idx: usize, base_off: usize, label: &str, rtype: u32| {
            let base = base_off + idx * 320;
            b[base..base + 4].copy_from_slice(&rtype.to_le_bytes()); // tReading: 1=温度,2=电压,3=风扇
            b[base + 4..base + 8].copy_from_slice(&(idx as u32).to_le_bytes()); // sensor_index
            for (i, ch) in label.bytes().enumerate() {
                b[base + 12 + i] = ch;
            }
            let db = base + 320 - 32;
            b[db..db + 8].copy_from_slice(&36.5f64.to_le_bytes());
            b[db + 8..db + 16].copy_from_slice(&30.0f64.to_le_bytes());
            b[db + 16..db + 24].copy_from_slice(&40.0f64.to_le_bytes());
            b[db + 24..db + 32].copy_from_slice(&35.0f64.to_le_bytes());
        };
        wr(&mut b, 0, OFF_R, "Package", 1);
        wr(&mut b, 1, OFF_R, "Chassis Fan", 3);

        b
    }

    #[test]
    fn parse_header_ok() {
        let b = fake_buf();
        let hdr = parse_header(&b[..64]).expect("头解析应成功");
        assert_eq!(hdr.version, 2);
        assert_eq!(hdr.revision, 1);
        assert_eq!(hdr.num_sensors, 2);
        assert_eq!(hdr.num_readings, 2);
        assert_eq!(hdr.size_reading, 320);
        assert_eq!(hdr.size_sensor, 512);
        assert_eq!(hdr.offset_sensors, 48);
        assert_eq!(hdr.offset_readings, 1072);
    }

    #[test]
    fn parse_header_bad_signature() {
        let mut b = fake_buf();
        b[0..4].copy_from_slice(&0xDEAD_BEEFu32.to_le_bytes());
        assert!(parse_header(&b[..64]).is_err());
    }

    #[test]
    fn parse_readings_decodes() {
        let b = fake_buf();
        let hdr = parse_header(&b[..64]).unwrap();
        let snap = parse_readings(&b, &hdr);
        assert_eq!(snap.readings.len(), 2);
        let cpu = &snap.readings[0];
        assert_eq!(cpu.group, "CPU");
        assert_eq!(cpu.label, "Package");
        assert_eq!(cpu.rtype, "温度");
        assert!((cpu.value - 36.5).abs() < 1e-9);
        let fan = &snap.readings[1];
        assert_eq!(fan.rtype, "风扇");
        assert_eq!(fan.group, "GPU"); // 读数 1 挂在组 1
    }

    #[test]
    fn truncated_buffer_safe() {
        let mut b = fake_buf();
        b.truncate(1072 + 320); // 只保留第一个读数，第二个越界
        let hdr = parse_header(&b[..64]).unwrap();
        let snap = parse_readings(&b, &hdr);
        // 不 panic，至多读到可用部分
        assert!(snap.readings.len() <= 2);
    }

    #[test]
    fn cstr_trims_nul_and_space() {
        let mut b = [b'x'; 16];
        b[0..5].copy_from_slice(b"Core\x00");
        assert_eq!(cstr(&b, 0, 16), "Core");
        let mut b2 = [b'x'; 16];
        b2[0..7].copy_from_slice(b"  Fan  ");
        b2[7] = 0;
        assert_eq!(cstr(&b2, 0, 16), "Fan");
    }

    // ── 端到端：用真实 CreateFileMappingW 造共享内存，走完整 read_full() 链路 ──

    /// 创建一块共享内存并写入 fake_buf，返回映射名。测试结束由 Guard 释放句柄。
    struct ShmGuard {
        name: String,
        handle: windows_sys::Win32::Foundation::HANDLE,
    }
    impl ShmGuard {
        fn create(name: &str, data: &[u8]) -> Self {
            use windows_sys::Win32::System::Memory::{CreateFileMappingW, PAGE_READWRITE};
            let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
            let handle = unsafe {
                CreateFileMappingW(
                    std::ptr::null_mut(),
                    std::ptr::null(),
                    PAGE_READWRITE,
                    0,
                    data.len() as u32,
                    wide.as_ptr(),
                )
            };
            assert!(!handle.is_null(), "CreateFileMappingW 失败");
            // 写入内容
            let view = unsafe {
                windows_sys::Win32::System::Memory::MapViewOfFile(
                    handle,
                    windows_sys::Win32::System::Memory::FILE_MAP_WRITE,
                    0,
                    0,
                    0,
                )
            };
            let ptr = view.Value;
            assert!(!ptr.is_null());
            unsafe {
                std::ptr::copy_nonoverlapping(data.as_ptr(), ptr as *mut u8, data.len());
            }
            unsafe {
                windows_sys::Win32::System::Memory::UnmapViewOfFile(
                    windows_sys::Win32::System::Memory::MEMORY_MAPPED_VIEW_ADDRESS { Value: ptr },
                );
            }
            Self {
                name: name.to_string(),
                handle,
            }
        }
    }
    impl Drop for ShmGuard {
        fn drop(&mut self) {
            unsafe {
                windows_sys::Win32::Foundation::CloseHandle(self.handle);
            }
        }
    }

    #[test]
    fn end_to_end_shared_memory_read() {
        let guard = ShmGuard::create("Local\\TestDiskPilotSM2", &fake_buf());
        // 直传测试映射名，走完整 read_full_with 链路（不碰全局 env，避免并发竞态）
        let names = vec![guard.name.clone()];
        let (snap, _) = read_full_with(&names).expect("应成功读到测试共享内存");
        assert_eq!(snap.num_groups, 2);
        assert_eq!(snap.readings.len(), 2);
        let cpu = &snap.readings[0];
        assert_eq!(cpu.group, "CPU");
        assert_eq!(cpu.rtype, "温度");
        assert!((cpu.value - 36.5).abs() < 1e-9);
        let txt = format_snapshot(&snap);
        assert!(txt.contains("HWiNFO 共享内存传感器"));
        assert!(txt.contains("CPU"));
        assert!(txt.contains("36.5"));
        let tl = read_temperature_lines_fake(&names);
        assert!(tl.iter().any(|(n, _)| n.contains("CPU")));
    }

    /// 测试版温度行读取：直传映射名（生产版走 map_names()，测试无法覆盖）。
    fn read_temperature_lines_fake(names: &[String]) -> Vec<(String, f64)> {
        let (snap, _) = read_full_with(names).expect("读取应成功");
        snap.readings
            .iter()
            .filter(|r| r.rtype == "温度" && (0.0..=150.0).contains(&r.value))
            .map(|r| (format!("{} | {}", r.group, r.label), r.value))
            .collect()
    }

    #[test]
    fn end_to_end_shm_not_found_degrades() {
        let names = vec!["Local\\TestDiskPilotNonexistent_xyz".to_string()];
        assert!(read_full_with(&names).is_err(), "打不开映射应报错");
    }
}
