//! 防火墙规则域（从 sec.rs 拆分，2026-10-12）。
//! collect_firewall_rules：DisplayName / Port / Protocol / Remote 元数据。

use crate::ps::ps_capture;
#[cfg(windows)]
use crate::tools::sec::sec_common::{v_arr, validate_ps_value};
use crate::tools::v_str;

#[cfg(windows)]
use serde_json::Value;

#[cfg(not(windows))]
use super::NOT_WINDOWS;

// ── 2. 防火墙规则（元数据：DisplayName / Port / Protocol / Remote）──────

/// 防火墙规则查询脚本模板：联查 `Get-NetFirewallRule` +
/// `Get-NetFirewallPortFilter` + `Get-NetFirewallAddressFilter`，
/// 输出规则元数据（**绝不返回规则内部动作细节**，只输出只读元数据）。
const FIREWALL_PS_TMPL: &str = r#"
$dir = '__DIR__'
$act = '__ACTION__'
$n = __N__
$rules = @()
$err = ''
try {
  $rules = @(Get-NetFirewallRule -Direction $dir -Action $act -Enabled True -ErrorAction Stop | Sort-Object -Property LastModifiedTime -Descending | Select -First $n | ForEach-Object {
    $r = $_
    $p = $r | Get-NetFirewallPortFilter -ErrorAction SilentlyContinue
    $a = $r | Get-NetFirewallAddressFilter -ErrorAction SilentlyContinue
    [ordered]@{
      Name = [string]$r.Name
      DisplayName = [string]$r.DisplayName
      Direction = [string]$r.Direction
      Action = [string]$r.Action
      Enabled = [string]$r.Enabled
      Group = [string]$r.Group
      Profile = [string]$r.Profile
      LocalPort = if ($p) {
        $lp = $p.LocalPort
        if ($null -ne $lp) { [string]$lp } else { '' }
      } else { '' }
      RemotePort = if ($p) {
        $rp = $p.RemotePort
        if ($null -ne $rp) { [string]$rp } else { '' }
      } else { '' }
      Protocol = if ($p) { [string]$p.Protocol } else { '' }
      RemoteAddress = if ($a) { [string]$a.RemoteAddress } else { '' }
    }
  })
} catch {
  $err = $_.Exception.Message
}
[ordered]@{ Rules = $rules; Err = $err } | ConvertTo-Json -Depth 6 -Compress
"#;

/// 防火墙规则采集（**只读元数据**）：默认 Inbound+Allow 高危组合，`top_n` 限流（默认 20，上限 50）。
///
/// 参数走白名单校验：`direction ∈ {Inbound, Outbound, Any}`，`action ∈ {Allow, Block, Any}`。
/// `Get-NetFirewallRule` 需管理员权限（部分 Win10 版本），失败时降级为中文说明。
#[cfg(windows)]
pub fn collect_firewall_rules(
    direction: Option<String>,
    action: Option<String>,
    top_n: usize,
) -> Result<String, String> {
    let dir = match direction.as_deref().map(|s| s.trim().to_string()) {
        Some(d) if !d.is_empty() => validate_ps_value(&d, &["Inbound", "Outbound", "Any"], "方向")?,
        _ => "Inbound".to_string(),
    };
    let act = match action.as_deref().map(|s| s.trim().to_string()) {
        Some(a) if !a.is_empty() => validate_ps_value(&a, &["Allow", "Block", "Any"], "动作")?,
        _ => "Allow".to_string(),
    };
    let n = if top_n < 1 { 20 } else { top_n.min(50) };

    let script = FIREWALL_PS_TMPL
        .replace("__DIR__", &dir)
        .replace("__ACTION__", &act)
        .replace("__N__", &n.to_string());
    let raw = ps_capture(&script)?;
    let raw = raw.trim().trim_start_matches('\u{feff}');
    if raw.is_empty() {
        return Ok("防火墙规则不可用（PowerShell 无输出）".into());
    }
    let v: Value =
        serde_json::from_str(raw).map_err(|e| format!("解析防火墙规则查询结果失败：{e}"))?;
    let err = v_str(&v, "Err");
    if !err.is_empty() {
        let e = err.to_ascii_lowercase();
        if e.contains("access is denied") || err.contains("拒绝访问") || err.contains("管理员")
        {
            return Ok(
                "需管理员权限，无法枚举防火墙规则（请以管理员身份运行以启用防火墙审计）".into(),
            );
        }
        if e.contains("not recognized") || e.contains("does not exist") {
            return Ok(
                "防火墙模块不可用（Get-NetFirewallRule cmdlet 缺失，可能为精简系统镜像）".into(),
            );
        }
        return Ok(format!(
            "防火墙规则查询失败：{}",
            err.chars().take(200).collect::<String>()
        ));
    }
    let rules = v_arr(&v, "Rules");
    if rules.is_empty() {
        return Ok(format!(
            "无匹配规则（方向={dir}，动作={act}，已启用；系统共 {} 条防火墙规则需按需筛选）\n\
             （默认视图为 Inbound+Allow 高危入站放行；可传 direction/action 覆盖）",
            "未知（-Enabled True 过滤后为 0）"
        ));
    }
    let mut out = String::new();
    out.push_str(&format!(
        "防火墙规则（{dir}·{act}，已启用，共 {} 条）：\n",
        rules.len()
    ));
    for r in rules {
        let dir_disp = match v_str(r, "Direction").to_ascii_lowercase().as_str() {
            "inbound" => "入站",
            "outbound" => "出站",
            _ => "N/A",
        };
        let act_disp = match v_str(r, "Action").to_ascii_lowercase().as_str() {
            "allow" => "放行",
            "block" => "阻止",
            _ => "N/A",
        };
        let disp = v_str(r, "DisplayName");
        let nm = v_str(r, "Name");
        let disp_nm = if disp.is_empty() {
            if nm.is_empty() {
                "（无名称）"
            } else {
                nm.as_str()
            }
        } else {
            disp.as_str()
        };
        let port = v_str(r, "LocalPort");
        let remote_port = v_str(r, "RemotePort");
        let proto = v_str(r, "Protocol");
        let remote = v_str(r, "RemoteAddress");
        let group = v_str(r, "Group");
        let profile = v_str(r, "Profile");
        let mut head = format!("- {dir_disp}·{act_disp} · {disp_nm}");
        let mut meta: Vec<String> = Vec::new();
        if !port.is_empty() {
            meta.push(format!("端口 {port}"));
        }
        if !remote_port.is_empty() {
            meta.push(format!("远端端口 {remote_port}"));
        }
        if !proto.is_empty() {
            meta.push(format!("协议 {proto}"));
        }
        if !remote.is_empty() {
            meta.push(format!("远程地址 {remote}"));
        }
        if !group.is_empty() {
            meta.push(format!("组「{group}」"));
        }
        if !profile.is_empty() {
            meta.push(format!("配置文件 {profile}"));
        }
        if meta.is_empty() {
            head.push_str(" · （无端口/协议/地址元数据）");
        } else {
            head.push_str(" · ");
            head.push_str(&meta.join(" · "));
        }
        out.push_str(&head);
        out.push('\n');
    }
    out.push_str(&format!(
        "\n说明：默认仅列 Inbound+Allow 高危入站放行（可传 direction/action 覆盖）；\
         \n  结果按 LastModifiedTime 降序取前 {n} 条；仅展示规则元数据（DisplayName/Port/Protocol/Address），\
         \n  不展示规则内部动作细节；来自 Get-NetFirewallRule 联查 PortFilter/AddressFilter，需管理员权限。"
    ));
    Ok(out)
}

#[cfg(not(windows))]
pub fn collect_firewall_rules(
    _direction: Option<String>,
    _action: Option<String>,
    _top_n: usize,
) -> Result<String, String> {
    Ok(NOT_WINDOWS.into())
}
