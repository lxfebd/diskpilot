//! WiFi 状态 / 已保存网络（只读 SSID / 安全类型，绝不读密码）。
//!
//! from tools/net.rs 拆分（2026-10-12）。

// 本文件采集实现均为 cfg(windows)（Linux 仅保留 collect_net_wifi 降级 fn），
// 顶层 import 随 Windows 实现一起编译，避免 Linux 下 unused-import 告警。
#[cfg(windows)]
use crate::ps::ps_capture;
#[cfg(windows)]
use serde_json::Value;

#[cfg(windows)]
use crate::tools::net_util::{get_int, is_emptyish, json_arr_of};
// wide_to_string 仅 Windows 有实现（wlanapi 宽字符回调），随 cfg(windows) 使用方编译。
#[cfg(windows)]
use crate::tools::net_util::wide_to_string;
#[cfg(windows)]
use crate::tools::v_str;

// ── 5. WiFi ─────────────────────────────────────────────────────────────

#[cfg(windows)]
pub(crate) const WLAN_STATUS_SUCCESS: u32 = 0;
#[cfg(windows)]
pub(crate) const WLAN_CLIENT_VERSION_2: u32 = 2;
// `wlan.h` 的 `WLAN_INTERFACE_STATE` 枚举（windows-sys 0.59 未导出命名常量）
#[cfg(windows)]
pub(crate) const WLAN_IS_UNKNOWN: i32 = 0;
#[cfg(windows)]
pub(crate) const WLAN_IS_DISCONNECTED: i32 = 1;
#[cfg(windows)]
pub(crate) const WLAN_IS_ADHOC_CLIENT: i32 = 2;
#[cfg(windows)]
pub(crate) const WLAN_IS_CONNECTED: i32 = 3;
#[cfg(windows)]
pub(crate) const WLAN_IS_NOT_ASSOCIATED: i32 = 4;
#[cfg(windows)]
pub(crate) const WLAN_IS_WFD_DEVICE: i32 = 5;
#[cfg(windows)]
pub(crate) const WLAN_IS_WFD_CLIENT: i32 = 6;
#[cfg(windows)]
pub(crate) const WLAN_IS_WFD_HOST: i32 = 7;

#[cfg(windows)]
pub(crate) fn wifi_state_cn(s: i32) -> &'static str {
    match s {
        WLAN_IS_DISCONNECTED => "已断开",
        WLAN_IS_ADHOC_CLIENT => "AdHoc 客户端",
        WLAN_IS_CONNECTED => "已连接",
        WLAN_IS_NOT_ASSOCIATED => "未关联",
        WLAN_IS_UNKNOWN => "未知",
        WLAN_IS_WFD_DEVICE => "WFD 设备",
        WLAN_IS_WFD_CLIENT => "WFD 客户端",
        WLAN_IS_WFD_HOST => "WFD 主机",
        _ => "其它",
    }
}

#[cfg(windows)]
pub(crate) struct NativeWifiInfo {
    pub(crate) interfaces: Vec<(String, i32)>, // (接口描述, state)
    pub(crate) profiles: Vec<String>,
}

/// 原生路径：`WlanOpenHandle(2)` + `WlanEnumInterfaces` + `WlanGetProfileList`。
/// 无适配器返回 `None`（调用方走 PS 兜底）；有适配器但 0 接口返回空 interfaces。
#[cfg(windows)]
pub(crate) fn native_wifi() -> Option<NativeWifiInfo> {
    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::NetworkManagement::WiFi::{
        WlanCloseHandle, WlanEnumInterfaces, WlanFreeMemory, WlanGetProfileList, WlanOpenHandle,
        WLAN_INTERFACE_INFO, WLAN_INTERFACE_INFO_LIST, WLAN_PROFILE_INFO, WLAN_PROFILE_INFO_LIST,
    };

    let mut ver: u32 = 0;
    let mut handle: HANDLE = std::ptr::null_mut();
    let rc = unsafe {
        WlanOpenHandle(
            WLAN_CLIENT_VERSION_2,
            std::ptr::null(),
            &mut ver,
            &mut handle,
        )
    };
    if rc != WLAN_STATUS_SUCCESS || handle.is_null() {
        return None;
    }

    let mut list_ptr: *mut WLAN_INTERFACE_INFO_LIST = std::ptr::null_mut();
    let rc = unsafe { WlanEnumInterfaces(handle, std::ptr::null(), &mut list_ptr) };
    let interfaces: Vec<(String, i32)> = if rc == WLAN_STATUS_SUCCESS && !list_ptr.is_null() {
        let n = unsafe { (*list_ptr).dwNumberOfItems } as usize;
        let base = unsafe { (*list_ptr).InterfaceInfo.as_ptr() as *const u8 };
        let stride = std::mem::size_of::<WLAN_INTERFACE_INFO>();
        let mut out = Vec::new();
        for i in 0..n {
            let info = unsafe { &*(base.add(i * stride) as *const WLAN_INTERFACE_INFO) };
            let name = wide_to_string(&info.strInterfaceDescription);
            if !name.is_empty() {
                out.push((name, info.isState as i32));
            }
        }
        out
    } else {
        Vec::new()
    };

    let mut profiles: Vec<String> = Vec::new();
    let mut plist_ptr: *mut WLAN_PROFILE_INFO_LIST = std::ptr::null_mut();
    let rc =
        unsafe { WlanGetProfileList(handle, std::ptr::null(), std::ptr::null(), &mut plist_ptr) };
    if rc == WLAN_STATUS_SUCCESS && !plist_ptr.is_null() {
        let n = unsafe { (*plist_ptr).dwNumberOfItems } as usize;
        let base = unsafe { (*plist_ptr).ProfileInfo.as_ptr() as *const u8 };
        let stride = std::mem::size_of::<WLAN_PROFILE_INFO>();
        for i in 0..n {
            let info = unsafe { &*(base.add(i * stride) as *const WLAN_PROFILE_INFO) };
            let name = wide_to_string(&info.strProfileName);
            if !name.is_empty() {
                profiles.push(name);
            }
        }
        // WlanGetProfileList 的返回内存必须由 WlanFreeMemory 释放
        unsafe { WlanFreeMemory(plist_ptr as *const _) };
    }

    let _ = unsafe { WlanCloseHandle(handle, std::ptr::null()) };
    Some(NativeWifiInfo {
        interfaces,
        profiles,
    })
}

/// PS 拼接当前 SSID / 信号 / 安全类型（原生 `WLAN_ASSOCIATION_INFO` 未在 0.59 导出）。
#[cfg(windows)]
pub(crate) fn ps_wifi_current() -> Vec<Value> {
    let script = r#"
$ErrorActionPreference='SilentlyContinue'
Get-NetWiFiInterface | Where-Object { $_.Name } | ForEach-Object {
    [ordered]@{
        Name=[string]$_.Name
        InterfaceDescription=[string]$_.InterfaceDescription
        Ssid=[string]$_.SSID
        Signal=[int64]$_.Signal
        SecurityType=[string]$_.SecurityType
        AuthenticationType=[string]$_.AuthenticationType
    }
} | ConvertTo-Json -Depth 5 -Compress
"#;
    json_arr_of(script, "WiFi 接口").unwrap_or_default()
}

/// 构造「查单个 profile 安全类型」的 netsh 命令（抽成纯函数便于回归测试）。
/// 转义规则见 [`ps_wifi_profile_security`] 的注入防护说明。
/// 依赖 `sys::escape_ps_single_quoted`（cfg(windows) 重导出），仅 Windows 编译。
#[cfg(windows)]
pub(crate) fn wifi_profile_security_script(profile: &str) -> String {
    let escaped = crate::tools::sys::escape_ps_single_quoted(profile);
    format!("netsh wlan show profiles name='{escaped}'")
}

/// 已保存 profile 的安全类型：`netsh wlan show profiles` 中英文列名都含
/// "Authentication/身份验证"。只读安全类型，**不读任何密码字段**。
///
/// 注入防护（2026-10-11 修复）：profile 名来自本机保存的 Wi-Fi 配置，而 SSID
/// 可由近距离攻击者任意广播（`$`/`(`/`)`/反引号都不是 Windows 文件名非法字符，
/// 恶意 SSID 能存成 profile 名）。脚本跑在 `powershell.exe -Command` 里，
/// 名字必须按 **PowerShell 单引号字面量**规则转义（单引号翻倍），且**不能**
/// 走双引号——否则 `$(...)` / 反引号会被 PS 先求值再传给 netsh（实测注入成立）。
/// 反斜杠转义是 netsh/C 习惯、PS 不认，原实现双重错误，已一并移除。
#[cfg(windows)]
pub(crate) fn ps_wifi_profile_security(
    profiles: &[String],
) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    for p in profiles {
        let script = wifi_profile_security_script(p);
        let out = match ps_capture(&script) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let mut auth = String::new();
        let mut enc = String::new();
        for line in out.lines() {
            let lower = line.to_ascii_lowercase();
            if lower.contains("authentication") || line.contains("身份验证") {
                if let Some(idx) = line.find(':') {
                    auth = line[idx + 1..].trim().to_string();
                }
            } else if lower.contains("encryption") || line.contains("加密") {
                if let Some(idx) = line.find(':') {
                    enc = line[idx + 1..].trim().to_string();
                }
            }
        }
        if auth.is_empty() && enc.is_empty() {
            continue;
        }
        let tag = if auth.eq_ignore_ascii_case("open") || enc.eq_ignore_ascii_case("none") {
            "open".to_string()
        } else {
            "encrypted".to_string()
        };
        map.insert(p.clone(), tag);
    }
    map
}

/// WiFi 接口 + 已保存网络列表。**安全红线：绝不读密码**（不调 `WlanGetProfile`）。
/// 无无线网卡返回中文说明而非 `Err`。
#[cfg(windows)]
pub fn collect_net_wifi() -> Result<String, String> {
    let native = native_wifi();
    let current = ps_wifi_current();

    match native {
        None => {
            if !current.is_empty() {
                let mut out = String::from("无线网卡接口：\n");
                for v in &current {
                    let d = v_str(v, "InterfaceDescription");
                    let n = v_str(v, "Name");
                    let disp = if d.is_empty() { n } else { d };
                    let ssid = v_str(v, "Ssid");
                    let suffix = if is_emptyish(&ssid) {
                        String::new()
                    } else {
                        format!("（当前连接：{ssid}）")
                    };
                    out.push_str(&format!("- 接口 {disp}{suffix}\n"));
                }
                out.push_str("- 已保存网络：读取失败（原生 wlanapi 不可用）\n");
                out.push_str("- 说明：本工具只读 SSID，绝不读取或输出任何 WiFi 密码");
                return Ok(out);
            }
            Ok("本机没有无线网卡（无 WiFi 适配器）".into())
        }
        Some(info) => {
            if info.interfaces.is_empty() {
                return Ok("本机没有无线网卡（无 WiFi 适配器）".into());
            }
            let sec_map = ps_wifi_profile_security(&info.profiles);
            let mut out = String::new();
            out.push_str(&format!("无线网卡接口（{} 个）：\n", info.interfaces.len()));
            for (iface, native_state) in &info.interfaces {
                let m = current.iter().find(|v| {
                    let d = v_str(v, "InterfaceDescription");
                    let n = v_str(v, "Name");
                    d == *iface || n == *iface
                });
                let (ssid, signal, sec) = match m {
                    Some(v) => (
                        v_str(v, "Ssid"),
                        get_int(v, "Signal").unwrap_or(-1),
                        v_str(v, "SecurityType"),
                    ),
                    None => (String::new(), -1, String::new()),
                };
                out.push_str(&format!("- 接口 {iface}\n"));
                let ssid_part = if is_emptyish(&ssid) {
                    " · 未连接".to_string()
                } else {
                    format!(" · 当前连接网络：{ssid}")
                };
                out.push_str(&format!(
                    "  - 状态 {}{ssid_part}\n",
                    wifi_state_cn(*native_state)
                ));
                let mut extra = Vec::new();
                if signal >= 0 {
                    extra.push(format!("信号 {signal}%"));
                }
                if !is_emptyish(&sec) {
                    extra.push(format!("安全 {sec}"));
                }
                if !extra.is_empty() {
                    out.push_str(&format!("  - {}\n", extra.join(" · ")));
                }
            }
            if info.profiles.is_empty() {
                out.push_str("已保存网络：无\n");
            } else {
                out.push_str(&format!("已保存网络（{} 个）：\n", info.profiles.len()));
                for p in &info.profiles {
                    let tag = sec_map.get(p).map(|s| s.as_str()).unwrap_or("(未知)");
                    out.push_str(&format!("- {} · 安全 {tag}\n", p));
                }
            }
            out.push_str("- 说明：本工具只读 SSID / 安全类型，绝不读取或输出任何 WiFi 密码");
            Ok(out)
        }
    }
}

#[cfg(not(windows))]
pub fn collect_net_wifi() -> Result<String, String> {
    Ok("当前平台不是 Windows，网络信息不可用".into())
}
