//! 网络信息只读采集（Windows：原生 iphlpapi + wlanapi 首选，PowerShell 兜底；非 Windows 降级）。
//!
//! 纯工具文件，不接路由（由 `tools/mod.rs` 统一接 MCP tool 薄壳）。
//! **全部只读**：不改静态 IP、不开共享、不动防火墙、不调网口开关、不改 DHCP、
//! 不发起任何 TCP/UDP 连接；**绝不读 WiFi 密码**（不调 `WlanGetProfile`，
//! 即便技术上能取到明文）。全部走公开只读 API 或 CIM PowerShell，
//! 普通用户权限即可，不需要管理员、不触发 UAC。
//!
//! ## 本文件导出函数清单（供 `tools/mod.rs` 对接）
//! - `pub fn collect_net_status() -> Result<String, String>`
//! - `pub fn collect_net_connections(pid: Option<u32>, keyword: Option<String>, top_n: usize) -> Result<String, String>`
//! - `pub fn collect_net_speed(interval_ms: u64) -> Result<String, String>`
//! - `pub fn collect_net_share() -> Result<String, String>`
//! - `pub fn collect_net_wifi() -> Result<String, String>`
//!
//! ## 已实测核对过的坑（windows-sys 0.59.0 源码逐字段验证）
//! - **`IP_ADAPTER_ADDRESSES_LH` 是 `#[repr(C, packed(1))]`**：不能 `&*ptr` 取引用
//!   （会触发 "reference to packed field"），必须 `std::ptr::read_unaligned(ptr)` 取
//!   值拷贝后再用字段。子结构（unicast / gateway / dns）本身非 packed，可直接 `&*`。
//! - **`GetAdaptersAddresses` / `GetExtended*Table` 两次调用模式**：首调
//!   `size=0` 必须返回 `ERROR_INSUFFICIENT_BUFFER` (122)，其它值直接失败降级；
//!   不能用 `size==0` 判断「空表」（空表首调也会返回 122 并给 size = sizeof(header)）。
//! - **变长表（`table: [T; 1]` 的 C 尾数组）**：用 `table.as_ptr()` +
//!   `size_of::<T>() * i` 手工步进，**不能** `slice::from_raw_parts`（长度越界 UB）。
//!   `MIB_TCP6TABLE_OWNER_PID` / `MIB_UDP6TABLE_OWNER_PID` 均已导出，直接用。
//! - **端口是 u32 且为网络字节序**：`dwLocalPort` 高 16 位为 0，必须
//!   `u16::from_be((x & 0xFFFF) as u16)`；直接 `x as u16` 会字节序全错。
//! - **IPv4 地址是 u32 网络序**：`u32::from_be` 再转点分十进制。
//!   IPv6 在 `ucLocalAddr: [u8; 16]`（TCP6/UDP6）或 `IN6_ADDR.u.Byte`。
//! - **`SOCKADDR_IN.sin_port` / `SOCKADDR_IN6.sin6_port` 都是网络序**；
//!   `IN_ADDR.S_un.S_addr` 直接可当 u32 用；`SOCKADDR` 本体只有 16 字节
//!   （比 `SOCKADDR_IN6` 的 28 字节小），必须先读 `sa_family` 再按类型 cast。
//! - **`TransmitLinkSpeed` / `ReceiveLinkSpeed` 单位是 bytes/sec**（0 = 未报告），
//!   展示乘 8 转 bps。坑：把 `104857600`（100 MB/s = 800 Mbps，1G 口协商值）
//!   当 Mbps 直接显示会差 8 倍。
//! - **`GetExtendedTcpTable` 的 `ulaf` 参数是 `AF_INET`(2) / `AF_INET6`(23)**，
//!   不是 0；`tableclass` 是 i32，需 `as u32` 传参。
//! - **`WlanOpenHandle` 需 `WLAN_CLIENT_VERSION_2`（传 2）**；返回 0 表示成功。
//!   返回 `1168` (`ERROR_NOT_FOUND`) = 本机无 WiFi 适配器，属正常情况。
//! - **`WLAN_INTERFACE_STATE` / `WLAN_INTF_OPCODE` 无命名常量**：按 `wlan.h` 内联
//!   （STATE 0..=7；`GET_CURRENT_SETTINGS` = 11）。`WLAN_INTERFACE_CURRENT_SETTINGS`
//!   也未导出，用字节偏移解析（`dot11Ssid` 在偏移 5，`ucSSID` 在偏移 6）。
//! - **`WlanGetProfileList` 的返回内存必须 `WlanFreeMemory` 释放**（常见泄漏点）。
//! - **`MIB_TCP_STATE` 11 = TIME_WAIT** 约占连接表 43%，默认过滤；
//!   `Listen` 行与 UDP 的本地监听（含 `0.0.0.0` / `::` 通配）全部保留。
//! - **`Get-SmbShare` 普通权限可用**；`Get-SmbSession` / `net session` 普通权限必拒
//!   （Access Denied / 错误 5），必须 `try/catch` 后降级为中文说明，不能把 Err 抛上去。
//! - **`Get-NetAdapterStatistics` 字段是 `ReceivedBytes` / `SentBytes`**（不是 `BytesReceived`）。
//! - **PS 原始字符串 `r#"..."#` 内不得出现 `"#` 序列**；动态参数用 `__MS__` 占位后
//!   `replace` 注入，不用 `{SPEED_PS = ...}` 内联捕获。
//! - **rustc 1.98 拒绝 `{:.1f}`**：全部浮点用 `{:.*}`（`format!("{:.*}", 2, v)`）。
//!
//! ## 拆分说明（2026-10-12）
//! 本文件曾是 1741 行单体，按域拆为 7 个平级子模块（声明在 `tools/mod.rs`）：
//! `net_util`（通用小工具）/ `net_adapter`（网卡清单）/ `net_status`（状态+详情）/
//! `net_conn`（TCP/UDP 连接）/ `net_speed`（实时速率）/ `net_share`（SMB 共享）/
//! `net_wifi`（WiFi）。本文件保留文档与 tests，collect_* 入口经 `pub use` 重导出，
//! `tools/mod.rs` 的 `net::collect_xxx` 调用路径零变化。

// 对外入口重导出（`tools/mod.rs` 走 `net::collect_xxx` 路径不变）。
// 各子模块内部符号（helper/struct）只做 pub(crate)，不进本文件的 pub 面。
pub use crate::tools::net_conn::collect_net_connections;
pub use crate::tools::net_share::collect_net_share;
pub use crate::tools::net_speed::collect_net_speed;
pub use crate::tools::net_status::{collect_adapter_detail, collect_net_status};
pub use crate::tools::net_wifi::collect_net_wifi;

#[cfg(test)]
mod tests {
    use crate::tools::net_status::{adapter_status_cn, collect_adapter_detail, prefix_to_mask};
    use crate::tools::net_wifi::wifi_profile_security_script;

    #[test]
    #[cfg(windows)]
    fn prefix_to_mask_known() {
        assert_eq!(prefix_to_mask(24), "255.255.255.0");
        assert_eq!(prefix_to_mask(16), "255.255.0.0");
        assert_eq!(prefix_to_mask(8), "255.0.0.0");
        assert_eq!(prefix_to_mask(0), "0.0.0.0");
        assert_eq!(prefix_to_mask(32), "255.255.255.255");
    }

    #[test]
    #[cfg(windows)]
    fn adapter_status_cn_known() {
        assert_eq!(adapter_status_cn(1), "已连接");
        assert_eq!(adapter_status_cn(2), "未连接");
        assert_eq!(adapter_status_cn(6), "未插入/已禁用");
        assert!(adapter_status_cn(99).contains("状态码"));
    }

    /// 注入回归：profile 名（恶意 SSID 可控制）必须被 PowerShell **单引号字面量**
    /// 完整包住——双引号里 `$(...)` / 反引号会被 PS 先求值再传给 netsh
    /// （2026-10-11 实测注入成立后修复）。
    #[test]
    fn wifi_profile_script_is_ps_single_quoted_literal() {
        // 注入载荷原样保留（单引号内 = 字面量）
        let s = wifi_profile_security_script("evil$(Write-Output pwned)`whoami");
        assert!(
            s.contains("name='evil$(Write-Output pwned)`whoami'"),
            "载荷必须整体落在单引号字面量内：{s}"
        );
        // 绝不能出现双引号包裹（双引号 = 可求值上下文）
        assert!(!s.contains("name=\""), "禁止双引号上下文：{s}");
        // 名字里的单引号必须翻倍逃逸，否则提前闭合字面量
        let s2 = wifi_profile_security_script("o'brien $(x)");
        assert!(
            s2.contains("name='o''brien $(x)'"),
            "单引号须翻倍且载荷仍被包住：{s2}"
        );
        // 反斜杠（网络共享名常见）保持原样——PS 单引号里反斜杠不是转义符
        let s3 = wifi_profile_security_script(r"\\server\share");
        assert!(s3.contains(r"name='\\server\share'"), "got: {s3}");
    }

    #[test]
    fn collect_adapter_detail_non_windows_degrade() {
        // 非 Windows 平台降级文案（纯函数，无 PS 依赖）
        let s = collect_adapter_detail();
        assert!(s.is_ok());
    }
}
