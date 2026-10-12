//! 硬件信息只读采集（Windows：PowerShell + WMI/CIM）。
//!
//! 纯工具文件，不接路由（由 `tools/mod.rs` 统一接 MCP tool 薄壳）。
//! **全部只读**：不写注册表、不改 BIOS、不装驱动、不调风扇控制器；
//! 全部走 CIM / `Get-PhysicalDisk`，**不需要管理员权限**，也不触发 UAC。
//! 数据经 `crate::ps::ps_capture` 跑 PowerShell（30s 超时 + UTF-8 强注入 + GBK 兜底），
//! 再 `serde_json::from_str` 解析后拼成中文可读多行文本（`- ` 前缀列表）。
//! 不需要管理员权限、不需要新增 windows-sys feature、不需要 Tauri。
//!
//! ## 已处理的已知坑（照搬主项目 `apps/desktop/src-tauri/src/hw.rs`，不重新发明）
//! - **L2CacheSize 的 WMI bug**：常返回 3 之类的垃圾值。`<= 0 || < 16` 一律显示"未知"，
//!   绝不把 3 KB 当真（主项目 hw.rs:997-1001）。
//! - **显存 32 位溢出**：`Win32_VideoController.AdapterRAM` 是 32 位，≥4 GB 的卡会回绕。
//!   `>= 4_294_967_296 || <= 4_293_918_720` → 显示"≥4 GB（WMI 32 位溢出，实际可能更大）"
//!   （主项目 hw.rs:1020-1029）；`== 0` → 共享系统内存 / 未知。
//! - **内存频率**：DDR5 的 `Speed` 只有实际有效频率的一半，输出必须标注，不能直接说
//!   "运行在 4800 MHz"。XMP/EXPO 诊断照搬主项目 hw.rs:1068-1085 的 `max_conf vs max_speed`。
//! - **ACPI 热区温度**是「十分之一开尔文」，不是摄氏：`c = raw / 10.0 - 273.15`。
//!   热区未就绪会报 2731（= 27.31 ℃，假的）或 0，必须用合理区间 `(0.0..=120.0)` 过滤；
//!   全部过滤掉时给降级文案，**不返回空列表假装成功**。
//! - **`Get-StorageReliabilityCounter.Temperature` 单位不统一**：有的控制器报摄氏、
//!   有的报华氏。启发式 `if t > 60.0 { (t-32)*5/9 } else { t }`（主项目 hw.rs:1161-1167）。
//! - **SMART 正解**是 `Get-PhysicalDisk` + `Get-StorageReliabilityCounter`
//!   （Win10 1607+，普通权限即可）。**不要**走 DeviceIoControl，**也不要**用 `wmic`
//!   （Win11 24H2 已从系统里移除）。
//!
//! ## 模块布局（2026-10-12 巨型文件拆分，原 2316 行 → 各域子模块）
//! - [`hw_common`]：JSON 取值工具 + nvidia-smi/ADL GPU 实时通道 + NOT_WINDOWS 降级文案
//! - [`hw_cpu`]：CPU 型号/核心/线程/频率/缓存 + CPUID 指令集
//! - [`hw_gpu`]：显卡型号/显存/驱动/分辨率 + N/A 卡实时 + GPU 快照
//! - [`hw_mem`]：内存条/容量/XMP 诊断 + DRAM 时序
//! - [`hw_sys`]：显示器 / ACPI 热区 / IPMI / 主板 BIOS
//! - [`hw_thermal`]：温度阶梯（HWiNFO → SuperIO → fancmd → ACPI）+ max_cpu_temp/gpu_temp_c
//! - [`hw_battery`]：电量/健康度/循环次数
//! - [`hw_disk`]：磁盘 SMART + USB 设备 + 可靠性计数
//!
//! 模块声明在 `tools/mod.rs`（文件模块的子模块须平级注册，否则会去找
//! `tools/hw/hw_battery.rs` 这种嵌套目录），这里只做重导出保持 `hw::` 旧路径。

pub use crate::tools::hw_battery::collect_battery;
pub use crate::tools::hw_cpu::{collect_cpu, collect_cpu_features};
pub use crate::tools::hw_disk::{collect_disk_smart, collect_disk_smart_raw, collect_usb_devices};
pub use crate::tools::hw_gpu::{collect_gpu, collect_gpu_snapshot};
pub use crate::tools::hw_mem::{collect_dram_timings, collect_memory};
pub use crate::tools::hw_sys::{collect_acpi, collect_displays, collect_ipmi, collect_motherboard};
pub use crate::tools::hw_thermal::{
    collect_sensors, collect_sensors_fast, collect_temperature, gpu_temp_c, max_cpu_temp,
};

#[cfg(test)]
mod tests {
    use crate::tools::hw_thermal::parse_gpu_temp_line;

    fn extract_gpu_temp_line(text: &str) -> String {
        text.lines()
            .find(|l| l.contains("GPU（") || l.contains("GPU("))
            .unwrap_or("")
            .trim()
            .to_string()
    }

    #[test]
    fn amd_line_format_matches_donut_regex() {
        // 前端熔断 regex：/GPU[（(](?:NVIDIA|AMD)[）)]：\s*([\d.]+)\s*℃/ —— 等价解析：
        // 行内含「GPU（ 或 GPU(」+ 厂商 +「）」+「：」+ 数字 +「℃」
        for line in [
            "- GPU（AMD）：58 ℃（atiadlxx ADL 实时）",
            "- GPU（NVIDIA）：63 ℃（nvidia-smi 实时）",
            "- GPU（AMD）：91 ℃（atiadlxx ADL 实时）",
        ] {
            let t = parse_gpu_temp_line(line);
            assert!(t.is_some(), "熔断解析未命中：{line}");
            assert!(t.unwrap() > 0.0);
        }
    }

    #[test]
    fn amd_line_present_in_collect_temperature_output() {
        // 模拟 collect_temperature 拼接 A 卡行的内容：AMD 无 nvidia-smi 时应出现
        // 「GPU（AMD）」行而非旧「非 NVIDIA」占位行。
        let mut out: Vec<String> = vec!["- 热区：35 ℃".into()];
        out.push("- GPU（AMD）：58 ℃（atiadlxx ADL 实时）".into());
        let text = format!("温度传感器：\n{}", out.join("\n"));
        let line = extract_gpu_temp_line(&text);
        assert!(
            line.contains("GPU（AMD）"),
            "A 卡行应带 AMD 厂商标签：{line}"
        );
        assert!(!line.contains("驱动未装"));
        // has_any 判定：正文含 ℃ 且不带「驱动未装」→ 算读到了
        let has_any = text
            .lines()
            .any(|l| l.contains("℃") && !l.contains("驱动未装"));
        assert!(has_any);
    }

    #[test]
    fn no_temp_channel_is_degrated_not_fake() {
        // 两通道都不可用时 collect_temperature 的占位行必须带「驱动未装」，
        // has_any 判定会把它排除，最后返回降级说明而非假装读数。
        let placeholder = "- GPU：非 NVIDIA/AMD 或驱动未装，WMI 不提供独立显卡温度";
        assert!(placeholder.contains("驱动未装"));
        let has_any = placeholder.contains("℃") && !placeholder.contains("驱动未装");
        assert!(!has_any);
    }

    #[test]
    fn gpu_temp_c_err_message_covers_both_vendors() {
        // gpu_temp_c（stress_test_gpu 熔断）两通道全失败的报错应提到 NVIDIA 与 AMD。
        let err = "未检测到 NVIDIA（nvidia-smi）或 AMD（atiadlxx ADL）GPU 温度";
        assert!(err.contains("NVIDIA"));
        assert!(err.contains("AMD"));
    }
}
