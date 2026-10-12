//! HWiNFO SM2 常量与类型（从 hwinfo.rs 拆分，2026-10-12）。
//! MAP_NAMES / map_names / SIGNATURE / READING_TYPES / HwReading / HwSnapshot。

/// 尝试打开的映射名（Global 优先，部分配置落在 Local 或裸名）。
#[cfg(windows)]
pub(crate) const MAP_NAMES: [&str; 3] = [
    "Global\\HWiNFO_SENS_SM2",
    "Local\\HWiNFO_SENS_SM2",
    "HWiNFO_SENS_SM2",
];

/// 实际使用的映射名列表（生产路径：HWiNFO 官方共享内存名）。
#[cfg(windows)]
pub(crate) fn map_names() -> Vec<String> {
    MAP_NAMES.iter().map(|s| s.to_string()).collect()
}

/// SM2 段头签名 `"SiWH"`（小端 u32）。
#[cfg(windows)]
pub(crate) const SIGNATURE: u32 = 0x5757_6953; // 'S''i''W''H' 小端拼装

/// 读数类型编号 → 中文（与 HWiNFO 文档一致）。
#[cfg(windows)]
pub(crate) const READING_TYPES: [&str; 9] = [
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
