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

/// 尝试打开的映射名（Global 优先，部分配置落在 Local 或裸名）。
#[cfg(windows)]
const MAP_NAMES: [&str; 3] = [
    "Global\\HWiNFO_SENS_SM2",
    "Local\\HWiNFO_SENS_SM2",
    "HWiNFO_SENS_SM2",
];

/// 实际使用的映射名列表（生产路径：HWiNFO 官方共享内存名）。
#[cfg(windows)]
fn map_names() -> Vec<String> {
    MAP_NAMES.iter().map(|s| s.to_string()).collect()
}

/// SM2 段头签名 `"SiWH"`（小端 u32）。
#[cfg(windows)]
const SIGNATURE: u32 = 0x5757_6953; // 'S''i''W''H' 小端拼装

/// 读数类型编号 → 中文（与 HWiNFO 文档一致）。
#[cfg(windows)]
const READING_TYPES: [&str; 9] = [
    "未知", "温度", "电压", "风扇", "电流", "功耗", "频率", "负载", "其他",
];

/// 单条读数（组名 + 标签 + 类型 + 单位 + 值 + 统计）。
#[cfg(windows)]
#[derive(Debug, Clone)]
pub(crate) struct HwReading {
    pub group: String,
    pub label: String,
    pub rtype: String,
    pub value: f64,
    pub unit: String,
    pub min: f64,
    pub max: f64,
    pub avg: f64,
}

/// HWiNFO 共享内存读取结果。
#[cfg(windows)]
pub(crate) struct HwSnapshot {
    pub version: u32,
    pub revision: u32,
    pub poll_epoch: i64,
    pub num_groups: u32,
    pub total_readings: u32,
    pub readings: Vec<HwReading>,
}

/// 读共享内存全部内容（一次映射：先读头定尺寸，再从同视图拷贝正文）。
#[cfg(windows)]
fn read_full_with(map_names: &[String]) -> Result<(HwSnapshot, Vec<u8>), String> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Memory::{
        MapViewOfFile, OpenFileMappingW, UnmapViewOfFile, FILE_MAP_READ, MEMORY_MAPPED_VIEW_ADDRESS,
    };

    unsafe fn map(name: &str) -> Result<(*mut u8, usize), String> {
        let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
        let handle = unsafe { OpenFileMappingW(FILE_MAP_READ, 0, wide.as_ptr()) };
        if handle.is_null() {
            return Err("OpenFileMappingW 返回空（HWiNFO 未运行或未启用共享内存）".into());
        }
        let view = unsafe { MapViewOfFile(handle, FILE_MAP_READ, 0, 0, 0) };
        // 句柄用完后立刻关：映射视图已持有引用计数，view 生命周期不依赖句柄。
        unsafe { CloseHandle(handle) };
        let ptr = view.Value;
        if ptr.is_null() {
            return Err("MapViewOfFile 失败".into());
        }
        // 视图真实大小：MapViewOfFile(0,0,0) 映射整个文件对象，长度未知。
        // 用 VirtualQuery 查所在内存区域的 RegionSize 作为硬上限——后续所有
        // 拷贝都钳制到它，杜绝「段头字段被篡改 → 越界读映射外内存」。
        // VirtualQuery 失败（理论不可能）时降级为保守 64KB（HWiNFO 段 ≥64B，
        // 真实映射远大于此；宁小勿大，读取截断比越界安全）。
        let mut mbi: windows_sys::Win32::System::Memory::MEMORY_BASIC_INFORMATION =
            std::mem::zeroed();
        let len = unsafe {
            windows_sys::Win32::System::Memory::VirtualQuery(
                ptr as *const core::ffi::c_void,
                &mut mbi,
                std::mem::size_of::<windows_sys::Win32::System::Memory::MEMORY_BASIC_INFORMATION>(),
            )
        };
        let region_size = if len > 0 && mbi.RegionSize > 0 {
            mbi.RegionSize as usize
        } else {
            64 * 1024
        };
        Ok((ptr as *mut u8, region_size))
    }

    let mut mapped: Option<(*mut u8, usize)> = None;
    let mut last_err = String::new();
    for name in map_names {
        match unsafe { map(name) } {
            Ok(m) => {
                mapped = Some(m);
                break;
            }
            Err(e) => last_err = e,
        }
    }
    let (ptr, map_len) = mapped.ok_or_else(|| last_err)?;

    // 头在映射头部（HWiNFO 段至少 64B），直接读。
    // 拷贝长度钳制到真实映射长度（map_len），防段头异常时越界读。
    let header_copy = map_len.min(64);
    if header_copy < 64 {
        unsafe { UnmapViewOfFile(MEMORY_MAPPED_VIEW_ADDRESS { Value: ptr as _ }) };
        return Err("HWiNFO 共享内存视图过短（<64B），无法读取段头".into());
    }
    let mut header_buf = [0u8; 64];
    unsafe {
        std::ptr::copy_nonoverlapping(ptr, header_buf.as_mut_ptr(), header_copy);
    }
    let hdr = match parse_header(&header_buf) {
        Ok(h) => h,
        Err(e) => {
            unsafe { UnmapViewOfFile(MEMORY_MAPPED_VIEW_ADDRESS { Value: ptr as _ }) };
            return Err(e);
        }
    };

    // 按真实尺寸拷贝正文：元素尺寸由段头携带，不存在偏移浮动问题。
    let needed = (hdr.offset_readings as usize
        + hdr.num_readings as usize * hdr.size_reading as usize)
        .max(hdr.offset_sensors as usize + hdr.num_sensors as usize * hdr.size_sensor as usize)
        .max(64);
    // 钳制到真实映射长度：段头字段即使被损坏，也绝不越过视图边界读。
    let needed = needed.min(map_len);
    let mut buf = vec![0u8; needed];
    unsafe {
        std::ptr::copy_nonoverlapping(ptr, buf.as_mut_ptr(), needed);
        UnmapViewOfFile(MEMORY_MAPPED_VIEW_ADDRESS { Value: ptr as _ });
    }
    let snap = parse_readings(&buf, &hdr);
    Ok((snap, buf))
}

/// 段头解析（参考实现允许 poll_time 偏移 12/16 两处，取描述子合理的一组）。
#[cfg(windows)]
#[derive(Debug, Clone, Copy)]
struct Header {
    version: u32,
    revision: u32,
    poll_epoch: i64,
    offset_sensors: u32,
    size_sensor: u32,
    num_sensors: u32,
    offset_readings: u32,
    size_reading: u32,
    num_readings: u32,
}

#[cfg(windows)]
fn parse_header(buf: &[u8]) -> Result<Header, String> {
    if buf.len() < 4 {
        return Err("共享内存头太短".into());
    }
    let sig = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
    if sig != SIGNATURE {
        return Err(format!(
            "共享内存签名不匹配（期望 SiWH，实际 {})",
            String::from_utf8_lossy(&buf[0..4])
        ));
    }
    let version = u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]);
    let revision = u32::from_le_bytes([buf[8], buf[9], buf[10], buf[11]]);
    for poll_off in [16usize, 12usize] {
        if buf.len() < poll_off + 32 {
            continue;
        }
        let poll_time = i64::from_le_bytes(buf[poll_off..poll_off + 8].try_into().unwrap());
        let off = poll_off + 8;
        let rd = |i: usize| -> usize { off + i * 4 };
        let off_s = u32::from_le_bytes(buf[rd(0)..rd(1)].try_into().unwrap());
        let size_s = u32::from_le_bytes(buf[rd(1)..rd(2)].try_into().unwrap());
        let num_s = u32::from_le_bytes(buf[rd(2)..rd(3)].try_into().unwrap());
        let off_r = u32::from_le_bytes(buf[rd(3)..rd(4)].try_into().unwrap());
        let size_r = u32::from_le_bytes(buf[rd(4)..rd(5)].try_into().unwrap());
        let num_r = u32::from_le_bytes(buf[rd(5)..rd(6)].try_into().unwrap());
        // 合理性校验：尺寸在协议现实范围内，且偏移不与头重叠。
        if (200..=8192).contains(&size_r)
            && (200..=4096).contains(&size_s)
            && num_r <= 500_000
            && num_s <= 50_000
            && off_r >= (off + 24) as u32
            && off_s >= (off + 24) as u32
        {
            return Ok(Header {
                version,
                revision,
                poll_epoch: poll_time,
                offset_sensors: off_s,
                size_sensor: size_s,
                num_sensors: num_s,
                offset_readings: off_r,
                size_reading: size_r,
                num_readings: num_r,
            });
        }
    }
    Err("无法解析 HWiNFO 共享内存头布局（版本过旧或布局异常）".into())
}

/// 取定长缓冲区中的 C 字符串（遇到 `\0` 截断，解码 cp1252 兼容中文）。
#[cfg(windows)]
fn cstr(buf: &[u8], offset: usize, length: usize) -> String {
    let end = offset.saturating_add(length).min(buf.len());
    if offset >= end {
        return String::new();
    }
    let raw = &buf[offset..end];
    let nul = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
    String::from_utf8_lossy(&raw[..nul]).trim().to_string()
}

/// 解析全部 sensor 组 + reading 元素。
#[cfg(windows)]
fn parse_readings(buf: &[u8], hdr: &Header) -> HwSnapshot {
    // 组名表：reading 通过 dwSensorIndex 指回组。
    let mut groups: Vec<String> = Vec::with_capacity(hdr.num_sensors as usize);
    for i in 0..hdr.num_sensors as usize {
        let base = hdr.offset_sensors as usize + i * hdr.size_sensor as usize;
        let name_user = cstr(buf, base + 8 + 128, 128);
        let name_orig = cstr(buf, base + 8, 128);
        groups.push(if !name_user.is_empty() {
            name_user
        } else {
            name_orig
        });
    }

    // 读数：4 个 double 恒在元素尾部 32B（padding-proof）。
    let mut readings = Vec::with_capacity(hdr.num_readings as usize);
    let doubles_rel = hdr.size_reading as usize - 32;
    for i in 0..hdr.num_readings as usize {
        let base = hdr.offset_readings as usize + i * hdr.size_reading as usize;
        if base + hdr.size_reading as usize > buf.len() {
            break; // 防御：段被截断时停止，不 panic
        }
        let treading = u32::from_le_bytes(buf[base..base + 4].try_into().unwrap());
        let sensor_index = u32::from_le_bytes(buf[base + 4..base + 8].try_into().unwrap()) as usize;
        let label_user = cstr(buf, base + 140, 128);
        let label_orig = cstr(buf, base + 12, 128);
        let unit = cstr(buf, base + 268, 16);
        let vbase = base + doubles_rel;
        let value = f64::from_le_bytes(buf[vbase..vbase + 8].try_into().unwrap());
        let vmin = f64::from_le_bytes(buf[vbase + 8..vbase + 16].try_into().unwrap());
        let vmax = f64::from_le_bytes(buf[vbase + 16..vbase + 24].try_into().unwrap());
        let vavg = f64::from_le_bytes(buf[vbase + 24..vbase + 32].try_into().unwrap());
        let group = groups
            .get(sensor_index)
            .cloned()
            .unwrap_or_else(|| "未知组".into());
        readings.push(HwReading {
            group,
            label: if !label_user.is_empty() {
                label_user
            } else {
                label_orig
            },
            rtype: READING_TYPES
                .get(treading as usize)
                .copied()
                .unwrap_or("其他")
                .to_string(),
            value,
            unit,
            min: vmin,
            max: vmax,
            avg: vavg,
        });
    }
    HwSnapshot {
        version: hdr.version,
        revision: hdr.revision,
        poll_epoch: hdr.poll_epoch,
        num_groups: hdr.num_sensors,
        total_readings: hdr.num_readings,
        readings,
    }
}

/// 人类可读值：`{:.2} {unit}`，数值为 0 且单位为计数/百分比时直接给 0 缩写。
#[cfg(windows)]
fn fmt_val(v: f64, unit: &str) -> String {
    if unit.is_empty() {
        format!("{:.*}", 2, v)
    } else {
        format!("{:.*} {}", 2, v, unit)
    }
}

/// 把一次读取出的全部读数格式化成一棵按组分组的中文文本树。
///
/// 每条读数带当前值 + 单位 + 类型；对「温度/功耗/风扇」类额外带 min/max
/// （AIDA64 同类的高低值，AI 判断异常很有用），并附采样时间。
#[cfg(windows)]
pub(crate) fn format_snapshot(snap: &HwSnapshot) -> String {
    use chrono::{TimeZone, Utc};
    let mut lines: Vec<String> = Vec::new();
    let t = Utc
        .timestamp_opt(snap.poll_epoch, 0)
        .single()
        .map(|t| t.format("%H:%M:%S").to_string())
        .unwrap_or_else(|| "未知".into());
    lines.push(format!(
        "HWiNFO 共享内存传感器 v{}.{}（共 {} 组 / {} 条读数，采样 {}）",
        snap.version, snap.revision, snap.num_groups, snap.total_readings, t
    ));
    for r in &snap.readings {
        let mut line = format!(
            "- {} | {}：{}（{}）",
            r.group,
            r.label,
            fmt_val(r.value, &r.unit),
            r.rtype
        );
        // 温度/功耗/风扇/电压类：min/max/avg 是有效诊断信息，带上
        if matches!(r.rtype.as_str(), "温度" | "功耗" | "风扇" | "电压" | "负载") {
            line.push_str(&format!(
                "，区间 {} ~ {}，均值 {}",
                fmt_val(r.min, &r.unit),
                fmt_val(r.max, &r.unit),
                fmt_val(r.avg, &r.unit)
            ));
        }
        lines.push(line);
    }
    lines.join("\n")
}

/// 尝试读 HWiNFO 共享内存并返回全量传感器文本。
///
/// HWiNFO 未运行 / 未启用共享内存 → `Err(原因)`，调用方降级。
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
