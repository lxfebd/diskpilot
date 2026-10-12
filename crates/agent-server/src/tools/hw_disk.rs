//! 磁盘 SMART 采集：健康状态/通电时长/温度/磨损 + USB 设备枚举（只读）。
//!
//! 从 hw.rs 拆分（2026-10-12）。
// 本文件采集实现均为 cfg(windows)（Linux 仅保留返回降级文案的 fn），
// 顶层 import 随 Windows 实现一起编译。
#[cfg(windows)]
use crate::ps::{json_array_of, ps_capture};
#[cfg(windows)]
use crate::tools::fmt_bytes;
#[cfg(windows)]
use crate::tools::hw_common::{format_num, get_f64, get_int};
#[cfg(windows)]
use crate::tools::v_str;
#[cfg(windows)]
use serde_json::Value;

// DISK_SMART_PS 由 hw_thermal 复用（collect_temperature 的磁盘温度通道，亦 cfg(windows)）
#[cfg(windows)]
pub(crate) const DISK_SMART_PS: &str = r#"$ErrorActionPreference='SilentlyContinue'
$disks=Get-PhysicalDisk
$out=@($disks | ForEach-Object {
  # 计数读不到有两种完全不同的原因（控制器真不上报 vs 普通权限被 CIM 拒绝），
  # 以前 SilentlyContinue 把异常吞了，界面只能显示 8 行「未上报」却不说是为什么。
  # 这里把错误文本带出来，Rust 侧据此给准确解释。
  $rc=$null; $rcErr=''
  try { $rc = $_ | Get-StorageReliabilityCounter -ErrorAction Stop } catch { $rcErr = [string]$_.Exception.Message }
  [ordered]@{
    friendly=[string]$_.FriendlyName
    media=[string]$_.MediaType
    bus=[string]$_.BusType
    size=$_.Size
    health=[string]$_.HealthStatus
    operational=[string]$_.OperationalStatus
    is_boot=$_.IsBoot
    rc_err=$rcErr
    power_on_hours=if($rc){$rc.PowerOnHours}else{$null}
    temperature=if($rc){$rc.Temperature}else{$null}
    temperature_max=if($rc){$rc.TemperatureMax}else{$null}
    wear=if($rc){$rc.Wear}else{$null}
    read_errors=if($rc){$rc.ReadErrorsTotal}else{$null}
    write_errors=if($rc){$rc.WriteErrorsTotal}else{$null}
    start_stop=if($rc){$rc.StartStopCycleCount}else{$null}
    load_unload=if($rc){$rc.LoadUnloadCycleCount}else{$null}
  }
})
$out | ConvertTo-Json -Depth 4 -Compress
"#;

/// 磁盘健康与 SMART 采集：型号/介质/总线/容量/健康状态 + 通电时长/温度/磨损/累计错误。
#[cfg(windows)]
pub fn collect_disk_smart() -> Result<String, String> {
    let raw = ps_capture(DISK_SMART_PS)?;
    let arr: Vec<Value> =
        serde_json::from_str(&json_array_of(raw.trim().trim_start_matches('\u{feff}')))
            .map_err(|e| format!("解析磁盘健康查询结果失败：{e}"))?;
    if arr.is_empty() {
        return Ok("未检测到物理磁盘（Get-PhysicalDisk 无返回）".into());
    }
    let mut s = String::new();
    s.push_str(&format!("- 磁盘 SMART（共 {} 块物理磁盘）：\n", arr.len()));
    for d in &arr {
        let nm = if v_str(d, "friendly").is_empty() {
            "未命名磁盘".to_string()
        } else {
            v_str(d, "friendly")
        };
        let size = fmt_bytes(get_int(d, "size").unwrap_or(0));
        let media = if v_str(d, "media").is_empty() {
            "未知".to_string()
        } else {
            v_str(d, "media")
        };
        let bus = if v_str(d, "bus").is_empty() {
            "未知".to_string()
        } else {
            v_str(d, "bus")
        };
        let health = if v_str(d, "health").is_empty() {
            "未知".to_string()
        } else {
            v_str(d, "health")
        };
        let boot = if d.get("is_boot").and_then(Value::as_bool).unwrap_or(false) {
            "（系统盘）"
        } else {
            ""
        };
        s.push_str(&format!(
            "- {nm}：{media}，{bus}，{size}，健康状态 {health}{boot}\n"
        ));
        let poh = get_int(d, "power_on_hours").filter(|&n| n > 0);
        if let Some(p) = poh {
            s.push_str(&format!(
                "  - 通电时长：{} 小时（约 {} 天 / {} 年）\n",
                format_num(p),
                p / 24,
                p / 8760
            ));
        } else {
            s.push_str("  - 通电时长：未提供（控制器未上报 PowerOnHours）\n");
        }
        // 单位启发式：> 60 视为华氏并换算摄氏，否则当作摄氏（主项目 hw.rs:1161-1167）。
        if let Some(t) = get_f64(d, "temperature") {
            let c = if t > 60.0 { (t - 32.0) * 5.0 / 9.0 } else { t };
            s.push_str(&format!(
                "  - 当前温度：{:.*} ℃（原始读数 {t}，>60 视为华氏并换算）\n",
                0, c
            ));
        }
        if let Some(t) = get_f64(d, "temperature_max") {
            if t > 0.0 {
                let c = if t > 60.0 { (t - 32.0) * 5.0 / 9.0 } else { t };
                s.push_str(&format!("  - 历史最高温度：{:.*} ℃\n", 0, c));
            }
        }
        let wear = get_f64(d, "wear");
        if let Some(w) = wear {
            if (0.0..=100.0).contains(&w) {
                s.push_str(&format!(
                    "  - 磨损度：{:.*}%（寿命剩余约 {:.*}%）\n",
                    0,
                    w,
                    0,
                    100.0 - w
                ));
            } else {
                s.push_str(&format!(
                    "  - 磨损度：未上报（原始值 {w} 超出 0~100% 有效范围，按未上报处理）\n"
                ));
            }
        } else if media.to_ascii_uppercase().contains("HDD") {
            // 机械盘没有「写入寿命百分比」这个概念，别说成 NVMe 没上报（误导）
            s.push_str("  - 磨损度：机械盘无磨损百分比概念（Wear 仅对 SSD 有意义）\n");
        } else {
            s.push_str("  - 磨损度：未上报（NVMe 控制器常见行为，不代表盘体健康有问题）\n");
        }
        let re = get_int(d, "read_errors").unwrap_or(0);
        let we = get_int(d, "write_errors").unwrap_or(0);
        s.push_str(&format!(
            "  - 累计错误：读 {} / 写 {}（非零且持续增长时建议关注）\n",
            format_num(re),
            format_num(we)
        ));
        let cyc = |k: &str| -> String {
            get_int(d, k)
                .map(|n| n.to_string())
                .unwrap_or_else(|| "未提供".into())
        };
        s.push_str(&format!(
            "  - 启停循环：{}，加载/卸载循环：{}\n",
            cyc("start_stop"),
            cyc("load_unload")
        ));
    }
    s.push_str(
        "\n说明：外接 USB / SD 卡通常不出现在 Get-PhysicalDisk 里（只有内接存储）；\n  \
         本工具未用 wmic（Win11 24H2 已移除 wmic.exe），也未用 DeviceIoControl（普通权限被拒）。",
    );
    Ok(s)
}

#[cfg(not(windows))]
pub fn collect_disk_smart() -> Result<String, String> {
    Ok(crate::tools::hw_common::NOT_WINDOWS.into())
}

// ── 8. USB 设备（Get-PnpDevice Class USB） ───────────────────────────────────

/// USB 设备采集走 `Get-PnpDevice -Class USB`（PNP 枚举，工作站普通权限即可，
/// 不需要管理员）。**不读 WiFi 密码、不读序列号机密**：只取设备类型/名称/
/// 厂商/状态/实例 ID（实例 ID 用于区分设备，不含敏感内容）。
/// 树形输出：控制器 → 集线器 → 外设层级（保留 ParentInstanceId 关联）。
/// USB 枚举 PS 脚本仅 Windows 运行，Linux 下无消费方。
#[cfg(windows)]
const USB_PS: &str = r#"
$ErrorActionPreference='SilentlyContinue'
$devs = @(Get-PnpDevice -Class USB -Status OK -ErrorAction SilentlyContinue | Select-Object FriendlyName, InstanceId, Class, Service, Status, Manufacturer, Present)
$devs | ConvertTo-Json -Depth 4 -Compress
"#;

/// USB 设备列表：控制器/集线器/外设（只读，普通权限）。
#[cfg(windows)]
pub fn collect_usb_devices() -> Result<String, String> {
    let raw = ps_capture(USB_PS)?;
    let arr: Vec<Value> =
        serde_json::from_str(&json_array_of(raw.trim().trim_start_matches('\u{feff}')))
            .map_err(|e| format!("解析 USB 设备查询结果失败：{e}"))?;
    if arr.is_empty() {
        return Ok(
            "未检测到 USB 设备（Get-PnpDevice -Class USB 无返回，或设备全为非 OK 状态）".into(),
        );
    }
    let mut s = format!("- USB 设备清单（共 {} 个）：\n", arr.len());
    let mut ctrl = 0;
    let mut hub = 0;
    let mut dev = 0;
    for d in &arr {
        let cls = v_str(d, "Class");
        let name = v_str(d, "FriendlyName");
        let name = if name.is_empty() {
            v_str(d, "InstanceId")
        } else {
            name
        };
        let service = v_str(d, "Service");
        let manuf = v_str(d, "Manufacturer");
        let mut line = format!("- {}（{}", name, cls);
        if !manuf.is_empty() {
            line.push_str(&format!("，厂商 {manuf}"));
        }
        if !service.is_empty() {
            line.push_str(&format!("，服务 {service}"));
        }
        line.push('）');
        match cls.as_str() {
            "USB" if name.to_ascii_lowercase().contains("controller") => ctrl += 1,
            "USB" => hub += 1,
            _ => dev += 1,
        }
        s.push_str(&line);
        s.push('\n');
    }
    s.push_str(&format!(
        "\n统计：控制器 {ctrl} / 集线器 {hub} / 外设 {dev}\n\
         说明：枚举 PNP 设备（普通权限）；外接 U 盘/键鼠/摄像头/声卡等在此列出。\
         只读元数据，未读任何设备内容或配置。"
    ));
    Ok(s)
}

#[cfg(not(windows))]
pub fn collect_usb_devices() -> Result<String, String> {
    Ok(crate::tools::hw_common::NOT_WINDOWS.into())
}

// ── 9. SMART 原始属性表（可靠性计数器） ─────────────────────────────────────

/// SMART 原始属性：在 `collect_disk_smart` 的聚合计数器之外，补一块
/// **可靠性计数器**（通电时长/温度/磨损/错误/循环）。
/// Windows 普通权限拿不到标准 SMART 属性表（ID 0x05 重分配扇区等），
/// 需要 smartctl（smartmontools，第三方，默认未装）——这里先给能拿到的
/// 可靠性计数，并明确标注不是完整 SMART 表，避免误导。
/// SMART 可靠性计数器（只读，普通权限）。
///
/// **已知限制**：输出的是 `Get-StorageReliabilityCounter` 的可靠性计数，
/// **不是**标准 SMART 属性表 ID 0x01~0xE6。要完整 SMART 表需要 smartctl
/// （第三方可选，本工具不自动装）。标注清楚，避免误读。
///
/// 复用 `DISK_SMART_PS`（字段是其超集），不单独维护脚本。
#[cfg(windows)]
pub fn collect_disk_smart_raw() -> Result<String, String> {
    let raw = ps_capture(DISK_SMART_PS)?;
    let arr: Vec<Value> =
        serde_json::from_str(&json_array_of(raw.trim().trim_start_matches('\u{feff}')))
            .map_err(|e| format!("解析磁盘可靠性计数失败：{e}"))?;
    if arr.is_empty() {
        return Ok("未检测到物理磁盘（Get-PhysicalDisk 无返回）".into());
    }
    Ok(render_smart_raw(&arr))
}

/// 纯渲染（可单测）：磁盘 JSON → 文本。
/// 一块计数都没读到时，不再逐盘打印 8 行「未上报」（实测 5 块盘 = 40 行噪音），
/// 改成一行一盘 + 说清为什么读不到。
#[cfg(windows)]
fn render_smart_raw(arr: &[Value]) -> String {
    let fields: [(&str, &str); 8] = [
        ("power_on_hours", "通电时长(小时)"),
        ("temperature", "温度(原始)"),
        ("temperature_max", "历史最高温(原始)"),
        ("wear", "磨损度"),
        ("read_errors", "累计读错误"),
        ("write_errors", "累计写错误"),
        ("start_stop", "启停循环"),
        ("load_unload", "加载/卸载循环"),
    ];
    let name_of = |d: &Value| -> String {
        if v_str(d, "friendly").is_empty() {
            "未命名磁盘".to_string()
        } else {
            v_str(d, "friendly")
        }
    };
    let health_of = |d: &Value| -> String {
        if v_str(d, "health").is_empty() {
            "未知".to_string()
        } else {
            v_str(d, "health")
        }
    };

    let mut s = String::new();
    s.push_str(&format!(
        "- 磁盘可靠性计数（共 {} 块物理磁盘）：\n",
        arr.len()
    ));

    // 一块都没读到任何计数时，别再逐盘打印 8 行「未上报」（实测 5 块盘 = 40 行噪音，
    // 读的人只会以为「这软件读不出东西」）。改成一行一盘 + 说清为什么读不到。
    let any_value = arr
        .iter()
        .any(|d| fields.iter().any(|(k, _)| get_int(d, k).is_some()));
    if !any_value {
        for d in arr {
            s.push_str(&format!(
                "- {}：健康 {}（计数未读到）\n",
                name_of(d),
                health_of(d)
            ));
        }
        s.push_str("\n说明：本次所有可靠性计数都读不到，这不代表磁盘有问题。\n  ");
        if let Some(err) = arr
            .iter()
            .map(|d| v_str(d, "rc_err"))
            .find(|e| !e.is_empty())
        {
            s.push_str(&format!(
                "PowerShell 侧错误：{}。\n  ",
                err.chars().take(160).collect::<String>()
            ));
        }
        s.push_str(
            "Get-StorageReliabilityCounter 在普通权限下常被 CIM 拒绝访问，以管理员身份运行本应用才能读到通电时长/温度/磨损。\n  \
             只想看容量与健康状态：用 disk_health；想看温度：用 hw_temperature（SuperIO 直读，普通权限可用）。",
        );
        return s;
    }

    for d in arr {
        s.push_str(&format!("- {}：健康 {}\n", name_of(d), health_of(d)));
        let f = |k: &str| -> String {
            get_int(d, k)
                .map(format_num)
                .unwrap_or_else(|| "未上报".into())
        };
        for (k, label) in fields {
            s.push_str(&format!("  - {label}：{}\n", f(k)));
        }
    }
    s.push_str(
        "\n说明：以上是 Get-StorageReliabilityCounter 可靠性计数，**不是标准 SMART 属性表\n  \
         （ID 0x01~0xE6 如 0x05 重分配扇区 / 0x0C 通电计数）**。\n  \
         要完整 SMART 表需安装 smartmontools 的 smartctl（第三方可选，本工具不自动装）。\n  \
         外接 USB / SD 卡通常不出现在 Get-PhysicalDisk 里。",
    );
    s
}

#[cfg(not(windows))]
pub fn collect_disk_smart_raw() -> Result<String, String> {
    Ok(crate::tools::hw_common::NOT_WINDOWS.into())
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn all_counters_missing_collapses_to_one_line_per_disk() {
        // 实测：普通权限下 CIM 拒绝 Get-StorageReliabilityCounter，
        // 5 块盘 × 8 个字段 = 40 行「未上报」，读的人只会以为软件读不出东西。
        let arr = vec![
            json!({"friendly": "WD_BLACK SN770 1TB", "health": "Healthy",
                   "rc_err": "无法从客户端中访问 CIM 资源。",
                   "power_on_hours": null, "temperature": null, "wear": null}),
            json!({"friendly": "WDC WD20EZRX", "health": "Healthy",
                   "rc_err": "", "power_on_hours": null, "temperature": null}),
        ];
        let out = render_smart_raw(&arr);
        assert_eq!(
            out.lines().filter(|l| l.starts_with("  - ")).count(),
            0,
            "{out}"
        );
        assert!(out.contains("计数未读到"), "{out}");
        assert!(out.contains("以管理员身份运行"), "{out}");
        assert!(out.contains("无法从客户端中访问 CIM 资源"), "{out}");
        assert!(out.contains("disk_health"), "{out}");
    }

    #[test]
    fn counters_present_keep_the_full_listing() {
        let arr = vec![json!({
            "friendly": "KIOXIA 2TB", "health": "Healthy", "rc_err": "",
            "power_on_hours": 4321, "temperature": 38, "wear": 3,
            "read_errors": 0, "write_errors": 0
        })];
        let out = render_smart_raw(&arr);
        assert!(
            out.contains("通电时长(小时)：4,321") || out.contains("通电时长(小时)：4321"),
            "{out}"
        );
        assert!(out.contains("温度(原始)：38"), "{out}");
        // 读到了就继续说明「这不是完整 SMART 表」。
        assert!(out.contains("不是标准 SMART 属性表"), "{out}");
    }

    #[test]
    fn partial_counters_still_print_per_disk_lines() {
        // 只要有一块盘读到了东西，就保留逐盘明细（另一块没读到的字段仍写未上报）。
        let arr = vec![
            json!({"friendly": "A", "health": "Healthy", "power_on_hours": 10}),
            json!({"friendly": "B", "health": "Healthy", "power_on_hours": null}),
        ];
        let out = render_smart_raw(&arr);
        assert!(out.contains("通电时长(小时)：10"), "{out}");
        assert!(out.contains("通电时长(小时)：未上报"), "{out}");
    }
}
