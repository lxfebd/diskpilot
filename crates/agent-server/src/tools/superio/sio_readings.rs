//! 环境寄存器读取域（从 superio.rs 拆分，2026-10-12）。
//! FAN_TACH_REG / write_env / read_env / select_bank_zero / Reading / rtc_alive / read_all / read_text / read_temperatures。

use super::sio_chip::{enter_chip, exit_chip, find_sio};
use super::sio_driver::{inp, load_driver, outp};

// ── 环境寄存器访问（间接 IO：地址口 = base+5，数据口 = base+6）──

#[cfg(windows)]
const FAN_TACH_REG: [u8; 6] = [0x0D, 0x0E, 0x0F, 0x80, 0x82, 0x4C];
#[cfg(windows)]
const FAN_TACH_EXT_REG: [u8; 6] = [0x18, 0x19, 0x1A, 0x81, 0x83, 0x4D];
#[cfg(windows)]
const FAN_PWM_CTRL_REG: [u8; 6] = [0x15, 0x16, 0x17, 0x18, 0x19, 0x1A];

#[cfg(windows)]
fn write_env(base: u16, reg: u8, val: u8) {
    outp(base + 5, reg);
    outp(base + 6, val);
}

#[cfg(windows)]
fn read_env(base: u16, reg: u8) -> u8 {
    outp(base + 5, reg);
    inp(base + 6)
}

/// 切到 bank0（IT87 环境寄存器 bank 选择，避免读到 bank1 残留值）。
#[cfg(windows)]
fn select_bank_zero(base: u16) {
    let v = read_env(base, 0x06);
    write_env(base, 0x06, v & 0x9F);
}

/// 单条传感器读数。
#[derive(Debug, Clone)]
pub struct Reading {
    pub kind: &'static str, // 温度 / 风扇 / PWM
    pub label: String,
    pub value: f64,
    pub unit: &'static str,
    pub note: String,
}

/// RTC 金标准：验证 Inp/Out 真实读写端口（全 0xFF 说明驱动未生效）。
#[cfg(windows)]
pub(crate) fn rtc_alive() -> bool {
    outp(0x70, 0x00);
    let sec = inp(0x71);
    outp(0x70, 0x02);
    let min = inp(0x71);
    outp(0x70, 0x04);
    let day = inp(0x71);
    !(sec == 0xFF && min == 0xFF && day == 0xFF)
}

/// 全量直读：驱动 → RTC 验证 → 芯片枚举 → 温度/风扇/PWM（只读）。
///
/// 返回 (读数列表, 说明文本)。驱动缺失 / 芯片未识别 → `Err(中文原因)`（调用方降级）。
#[cfg(windows)]
pub fn read_all() -> Result<(Vec<Reading>, String), String> {
    let drv = load_driver().ok_or_else(|| {
        "未找到端口驱动（inpoutx64/inpout32/WinRing0/WinIo）。HWiNFO 未运行且驱动缺失：\n  \
         CPU/主板温度直读不可用。可让 AI 用 fan_selfheal_fix action=install_driver 以管理员安装 \
         inpoutx64 驱动（仅一次，安装后普通权限即永久可用）"
            .to_string()
    })?;
    if !rtc_alive() {
        return Err("RTC 端口读回全 0xFF → Inp/Out 端口读写未生效（驱动加载了但未真正接管端口）。请重启后重试或检查驱动签名".into());
    }
    let sio = find_sio().ok_or_else(|| {
        format!(
            "SuperIO 芯片未识别（扫描 0x2E/0x4E × ITE/Nuvoton/Winbond，驱动 {} 可用）。\
             主板环境控制器不在标准 SuperIO 布局（部分笔记本/AMD 平台走 EC/SMBus），此路不通时自动降级其他通道",
            drv.name
        )
    })?;

    let mut readings: Vec<Reading> = Vec::new();
    // 保持配置态读环境寄存器（与 fancmd Run 相同流程）
    enter_chip(sio.addr, sio.kind);
    outp(sio.addr, 0x07);
    outp(sio.addr + 1, 0x04);
    select_bank_zero(sio.env_base);

    // 温度：0x29-0x2B（3 个源）
    let temp_labels = ["主板/System", "CPU", "辅助/Aux"];
    for (i, label) in temp_labels.iter().enumerate() {
        let t = read_env(sio.env_base, 0x29 + i as u8);
        if t != 0xFF && t != 0 {
            readings.push(Reading {
                kind: "温度",
                label: label.to_string(),
                value: f64::from(t),
                unit: "℃",
                note: "SuperIO 直读".into(),
            });
        }
    }

    // 风扇：16 位 TACH
    for (f, reg) in FAN_TACH_REG.into_iter().enumerate() {
        let lo = read_env(sio.env_base, reg);
        let hi = read_env(sio.env_base, FAN_TACH_EXT_REG[f]);
        let tach = (u16::from(hi) << 8) | u16::from(lo);
        let rpm = if tach > 0x3F && tach < 0xFFFF {
            1_350_000.0 / (f64::from(tach) * 2.0)
        } else {
            0.0
        };
        if rpm > 0.0 {
            readings.push(Reading {
                kind: "风扇",
                label: format!("风扇[{f}]"),
                value: rpm,
                unit: "RPM",
                note: "16 位 TACH".into(),
            });
        }
    }

    // PWM：当前占空比（只读展示控制状态，绝不写入）
    for (f, reg) in FAN_PWM_CTRL_REG.into_iter().enumerate() {
        let pwm = read_env(sio.env_base, reg);
        if pwm != 0xFF {
            let pct = if pwm > 0 && pwm < 255 {
                f64::from(pwm) * 100.0 / 255.0
            } else if pwm == 255 {
                100.0
            } else {
                0.0
            };
            readings.push(Reading {
                kind: "PWM",
                label: format!("风扇[{f}] 占空比"),
                value: pct,
                unit: "%",
                note: "软件控制状态（只读）".into(),
            });
        }
    }

    exit_chip(sio.addr);

    let info = format!(
        "自研 SuperIO 直读：驱动 {} / 芯片 0x{:04X} / 端口 0x{:02X} / 环境基址 0x{:04X}",
        drv.name, sio.id, sio.addr, sio.env_base
    );
    Ok((readings, info))
}

/// 文本快照（AI 可读）：每行一条读数 + 数据源说明。
#[cfg(windows)]
pub fn read_text() -> Result<String, String> {
    let (readings, info) = read_all()?;
    if readings.is_empty() {
        return Err(
            "SuperIO 直读成功但无有效读数（环境寄存器全 0xFF——该芯片可能未接传感器或布局不同）"
                .into(),
        );
    }
    let mut lines: Vec<String> = Vec::new();
    for r in &readings {
        lines.push(format!(
            "- {}：{:.*} {}（{}）",
            r.label, 1, r.value, r.unit, r.note
        ));
    }
    Ok(format!(
        "{info}\n传感器（{} 条）：\n{}",
        readings.len(),
        lines.join("\n")
    ))
}

/// 温度专用：仅返回温度类读数（label, value）。
#[cfg(windows)]
pub fn read_temperatures() -> Result<Vec<(String, f64)>, String> {
    let (readings, _) = read_all()?;
    let temps: Vec<(String, f64)> = readings
        .iter()
        .filter(|r| r.kind == "温度" && (0.0..=150.0).contains(&r.value))
        .map(|r| (r.label.clone(), r.value))
        .collect();
    if temps.is_empty() {
        Err("SuperIO 直读无温度类读数".into())
    } else {
        Ok(temps)
    }
}
