//! 服务枚举的解析辅助域：Win32_Service PS 输出解析（parse_ps_services）、
//! 注册表 Start DWORD / ImagePath 清理、服务行结构（ServiceRow）。
//! 从 sys_services.rs 拆分（2026-10-12）。纯解析无 IO。

use std::collections::HashMap;

use serde_json::Value;

use crate::ps::json_array_of;
use crate::tools::v_str;

use super::state_zh;

/// `Win32_Service.StartMode` 的英文枚举值 → 中文。
/// 仅用于「注册表打不开」的降级路径（fallback）；正常路径走 `start_dword_zh`。
#[cfg(windows)]
pub(crate) fn start_mode_zh(s: &str) -> &str {
    match s.to_ascii_lowercase().as_str() {
        "automatic" => "自动",
        "manual" => "手动",
        "disabled" => "禁用",
        "boot" => "启动",
        "system" => "系统",
        _ => "未知",
    }
}

/// 注册表 `Start` DWORD → 中文（0=Boot, 1=System, 2=Automatic, 3=Manual, 4=Disabled）。
#[cfg(windows)]
pub(crate) fn start_dword_zh(n: Option<u32>) -> (String, bool) {
    // 返回 (中文, 是否禁用)
    match n {
        Some(0) => ("启动(Boot)".to_string(), false),
        Some(1) => ("系统(System)".to_string(), false),
        Some(2) => ("自动".to_string(), false),
        Some(3) => ("手动".to_string(), false),
        Some(4) => ("禁用".to_string(), true),
        _ => ("未知".to_string(), false),
    }
}

pub(crate) fn clean_image_path(raw: &str) -> String {
    let t = raw.trim();
    if t.is_empty() {
        return "未知".to_string();
    }
    // 资源串 `@%SystemRoot%\system32\drivers\tcpip.sys,-10001` 优先判断（在去引号前）
    if t.starts_with('@') {
        return format!("（资源字符串引用：{t}）");
    }
    // 普通 `ImagePath`：去首尾引号（`"C:\...\svc.exe" --flag`）
    let stripped = t.trim_matches('"');
    stripped.to_string()
}

/// 从 `Win32_Service` PS 结果建 `HashMap<name_lower, (state_zh, start_mode_zh, display_name)>`。
#[cfg(windows)]
pub(crate) fn parse_ps_services(raw: &str) -> HashMap<String, (String, String, String)> {
    let mut map = HashMap::new();
    let trimmed = raw.trim().trim_start_matches('\u{feff}');
    if trimmed.is_empty() {
        return map;
    }
    let arr_str = json_array_of(trimmed);
    let arr: Vec<Value> = match serde_json::from_str(&arr_str) {
        Ok(v) => v,
        Err(_) => return map,
    };
    for v in &arr {
        let name = v_str(v, "Name");
        if name.is_empty() {
            continue;
        }
        let state_s = v_str(v, "State");
        let sm_s = v_str(v, "StartMode");
        let state = state_zh(&state_s);
        let sm = start_mode_zh(&sm_s);
        let dn = v_str(v, "DisplayName");
        map.insert(
            name.to_ascii_lowercase(),
            (state.to_string(), sm.to_string(), dn),
        );
    }
    map
}

/// 单个服务行（先收集，再过滤+排序+截断）。
#[cfg(windows)]
pub(crate) struct ServiceRow {
    pub(crate) name: String,
    pub(crate) display: String,
    pub(crate) state: String,
    pub(crate) start_zh: String,
    pub(crate) image: String,
    pub(crate) is_disabled: bool,
    pub(crate) is_driver: bool,
}
