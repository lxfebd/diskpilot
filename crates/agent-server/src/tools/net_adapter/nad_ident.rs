//! 网卡识别域（从 net_adapter.rs 拆分，2026-10-12）。
//! AdapterRow / is_virtual_adapter / is_ndis_filter_virtual / is_hidden_system_iface。
#[cfg(windows)]
pub(crate) struct AdapterRow {
    pub(crate) name: String,
    pub(crate) desc: String,
    pub(crate) status: i32,
    pub(crate) speed_bps: u64, // bytes/sec * 8，0 = 未上报
    pub(crate) mac: String,
    pub(crate) ips: Vec<String>,
    pub(crate) gateway: Vec<String>,
    pub(crate) dns: Vec<String>,
    pub(crate) is_virtual: bool,
}

/// 虚拟网卡识别：名称/描述命中关键字 → 标注「（虚拟）」而非丢弃。
/// 虚拟网卡链路速率恒 0，不标注容易被误判成「网卡坏了」。
#[cfg(windows)]
pub(crate) fn is_virtual_adapter(name: &str, desc: &str) -> bool {
    let hay = format!("{} {}", name, desc).to_ascii_lowercase();
    const KEYS: [&str; 13] = [
        "hyper-v",
        "vmware",
        "vbox",
        "virtualbox",
        "bluetooth",
        "loopback",
        "vethernet",
        "virtual",
        "virtualization",
        "vpn",
        "tap-win",
        "wsl",
        "tunnel",
    ];
    KEYS.iter().any(|k| hay.contains(k))
}

/// NDIS filter 层虚拟接口识别（`GetAdaptersAddresses` 的
/// `GAA_FLAG_INCLUDE_ALL_INTERFACES` 会把网卡下的 filter 层接口也列出来，
/// 名称形如「以太网-WFP Native MAC Layer LightWeight Filter-0000」）。
/// 它们不是用户视角的网卡：无 unicast 地址、链路速率恒 `u64::MAX`，
/// 应直接跳过而不是展示。
#[cfg(windows)]
pub(crate) fn is_ndis_filter_virtual(name: &str, desc: &str, has_unicast: bool) -> bool {
    if has_unicast {
        return false;
    }
    let hay = format!("{} {}", name, desc).to_ascii_lowercase();
    const FILTER_KEYS: [&str; 8] = [
        "wfp",
        "ndis lightweight",
        "lightweight filter",
        "filter driver",
        "qos packet scheduler",
        "virtual switch extension",
        "ndis filter",
        "-0000",
    ];
    FILTER_KEYS.iter().any(|k| hay.contains(k))
}

/// 静默隐藏接口识别：Windows 的 `GetAdaptersAddresses` 会把大量用户看不见的
/// 系统虚拟接口也列出来（WAN Miniport / Teredo / 6to4 / IP-HTTPS / 内核调试器 /
/// 隐藏的 WLAN 副卡等）。它们要么已禁用、要么是隧道占位，用户视角下是一堆噪音。
/// 判定：未连接 / 未插入 且 无 IP 且 名称/描述命中特征 → 视为隐藏接口直接跳过。
#[cfg(windows)]
pub(crate) fn is_hidden_system_iface(name: &str, desc: &str, status: i32, has_ip: bool) -> bool {
    if has_ip {
        return false;
    }
    // 只有「未连接 / 未插入 / 已禁用」的静默接口才可能被折叠
    let inactive = matches!(status, 2 | 6 | 7 | 8 | 9);
    if !inactive {
        return false;
    }
    let hay = format!("{} {}", name, desc).to_ascii_lowercase();
    const HIDDEN_KEYS: [&str; 9] = [
        "wan miniport",
        "teredo",
        "6to4",
        "ip-https",
        "kernel debug",
        "内核调试",
        "isatap",
        "microsoft wi-fi direct virtual",
        "pseudo-interface",
    ];
    HIDDEN_KEYS.iter().any(|k| hay.contains(k))
}
