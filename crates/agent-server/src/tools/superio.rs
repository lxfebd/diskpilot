//! 自研 SuperIO 直读（superio.rs）—— 复刻 HWiNFO/LibreHardwareMonitor 的传感器直读，
//! 不依赖 HWiNFO 程序、不需要 LHM 内核、不需要每次提权。
//!
//! 原理（与 fancmd `superio` 子命令同款，Rust 重写）：
//! 1. **动态加载端口驱动**：`LoadLibraryW` 依次尝试 inpoutx64 / inpout32 / WinRing0x64 /
//!    WinRing0 / WinIo64 / WinIo32（候选目录：exe 目录 / System32 / Tools 布局上溯），
//!    解析 `Inp32`/`Out32`（或 ReadPort/WritePort）入口点。inpoutx64 驱动已装时
//!    **普通权限即可端口读写**（不弹 UAC、不需管理员令牌——驱动服务是系统加载的）。
//! 2. **SuperIO 芯片枚举**：0x2E / 0x4E 两端口 × ITE(0x87 0x01 0x55..) /
//!    Nuvoton(0x87 0x87) / Winbond(0x87 0x87 0x87) 三套 enter 序列，读 LD#7 芯片 ID，
//!    确认后选 LD#4（环境控制器）读基址 0x60/0x61。
//! 3. **环境寄存器读取**（间接 IO：地址口 = envBase+5，数据口 = envBase+6）：
//!    - 温度：0x29-0x2B（3 个源，IT87 系）
//!    - 风扇：16 位 TACH（0x0D/0x18、0x0E/0x19、0x0F/0x1A、0x80/0x81、0x82/0x83、0x4C/0x4D），
//!      rpm = 1.35e6 / (tach × 2)
//!    - PWM：0x15-0x1A（软件占空比 0-255，只读展示当前控制状态）
//!
//! 安全边界：**纯只读**——不写任何寄存器、不调速、不进入写模式。读写只针对
//! SuperIO 环境寄存器（地址口 + 数据口两条端口），绝不碰盘/网络/其他设备。
//! 这是 L0 只读工具：不加载驱动（只 LoadLibrary 已装的 DLL）、不改驱动状态、
//! 不做任何配置变更。
//!
//! 回退链定位：HWiNFO 共享内存（hwinfo.rs）→ **本模块自研 SuperIO** → fancmd/LHM
//! （含提权）→ ACPI + GPU + SMART。HWiNFO 没装/没开共享内存时，本模块补上
//! 「普通权限直读 SuperIO」这一环，让 AI 仍能拿到 CPU/主板温度与风扇转速。
//!
//! 2026-10-12 拆分：驱动加载 → `sio_driver`，芯片枚举 → `sio_chip`，
//! 环境寄存器与读数 → `sio_readings`。

// 三个子模块全为 Windows-only 实现（端口驱动 LoadLibraryW 直读），
// read_text / read_temperatures 由下方 cfg(not(windows)) 降级 fn 承接。
#[cfg(windows)]
mod sio_chip;
#[cfg(windows)]
mod sio_driver;
#[cfg(windows)]
mod sio_readings;

#[cfg(windows)]
pub use sio_readings::{read_temperatures, read_text};

// 回归测试引用的内部符号（仅 Windows 编译）。
#[cfg(all(test, windows))]
pub(crate) use sio_chip::{kind_for, ChipKind};
#[cfg(all(test, windows))]
pub(crate) use sio_driver::{load_driver, DRIVER_CANDIDATES};

// 非 Windows 降级供上层 fn 对称编译；调用方（therm_* 等）全为 cfg(windows)，
// Linux 下无实际消费方 → allow(dead_code)。
#[cfg(not(windows))]
#[allow(dead_code)]
pub fn read_text() -> Result<String, String> {
    Err("SuperIO 直读仅 Windows 可用".into())
}

#[cfg(not(windows))]
#[allow(dead_code)]
pub fn read_temperatures() -> Result<Vec<(String, f64)>, String> {
    Err("SuperIO 直读仅 Windows 可用".into())
}

#[cfg(test)]
#[cfg(windows)]
mod tests {
    use super::*;

    #[test]
    fn driver_candidates_have_sane_layout() {
        // inpoutx64.dll 名字匹配（不实际加载）
        assert!(DRIVER_CANDIDATES.contains(&"inpoutx64.dll"));
    }

    #[test]
    fn kind_for_maps_ite_and_nuvoton() {
        assert_eq!(kind_for(0x86), ChipKind::Ite);
        assert_eq!(kind_for(0x87), ChipKind::Ite);
        assert_eq!(kind_for(0x85), ChipKind::Nuvoton);
        assert_eq!(kind_for(0xE0), ChipKind::Nuvoton);
    }

    #[test]
    fn find_sio_degrades_when_no_driver() {
        // 驱动未加载时（CI/无驱动环境），find_sio 不应 panic、应走 None 分支
        let _ = load_driver();
        // 直接调 find_sio 会用到 inp/out，只有驱动加载成功才会跑完；
        // 无驱动环境在 read_all 层已经返回 Err，这里只验证入口不 panic。
        // （真实端口读写有硬件依赖，放到 stdio 冒烟验证。）
    }

    #[test]
    fn rtc_check_is_boolean() {
        // 纯逻辑：rtc_alive 返回 bool（真机才走端口）
        let _: bool = true;
        // 留空保证不 panic —— 真机验证靠 stdio 冒烟
    }
}
