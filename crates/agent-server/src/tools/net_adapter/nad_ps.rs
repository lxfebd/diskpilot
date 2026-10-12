//! PowerShell 兜底域（从 net_adapter.rs 拆分，2026-10-12）。
//! ps_adapters_fallback / json_list_to_vec。

use crate::tools::net_util::{get_int, json_arr_of};
use crate::tools::v_str;
use serde_json::Value;

use super::nad_ident::AdapterRow;
#[cfg(windows)]
pub(crate) fn ps_adapters_fallback() -> Result<Vec<AdapterRow>, String> {
    let script = r#"
# PS 输出必须 @() 强制数组，否则单网卡会被折叠成对象
$ErrorActionPreference='SilentlyContinue'
$ns4=[System.Net.Sockets.AddressFamily]::InterNetwork
$ns6=[System.Net.Sockets.AddressFamily]::InterNetworkV6
$cfg=@(Get-NetIPConfiguration | ForEach-Object {
    [ordered]@{
        Alias=[string]$_.InterfaceAlias
        Ip=@(($_.IPv4Address | ForEach-Object { [string]$_.IPAddress + '/' + [string]$_.PrefixLength }) + ($_.IPv6Address | Where-Object { $_.AddressFamily -eq $ns6 -and $_.IPAddress -ne '::' } | ForEach-Object { [string]$_.IPAddress }))
        Gateway=@((($_.IPv4DefaultGateway | Select -Expand NextHop) -join ',') -split ',' + (($_.IPv6DefaultGateway | Select -Expand NextHop) -join ','))
        Dns=@((($_.DNSServer | Where-Object { $_.AddressFamily -eq $ns4 -or $_.AddressFamily -eq $ns6 } | Select -Expand ServerAddresses) -join ','))
    }
})
$ns=@{}
foreach($c in $cfg){ if($c.Alias){ $ns[[string]$c.Alias]=$c } }
$out=@(Get-NetAdapter | ForEach-Object {
    $a=$_
    $c=$ns[[string]$a.Name]
    $mac=[string]$a.MacAddress
    $status=switch($a.Status){'Up'{1}'Disabled'{6}'Unavailable'{6}'Disconnected'{2}default{2}}
    [ordered]@{
        Name=[string]$a.Name
        Description=[string]$a.Description
        Status=$status
        LinkSpeed=if($a.LinkSpeed){
            # LinkSpeed 是带单位字符串（如 "3 Mbps" / "1 Gbps"），需解析成 Mbps 数值
            $ls=[string]$a.LinkSpeed
            $lsm=if($ls -match '([\d.]+)\s*Mbps'){ [double]$matches[1] }
                 elseif($ls -match '([\d.]+)\s*Gbps'){ [double]$matches[1]*1000 }
                 elseif($ls -match '([\d.]+)\s*Kbps'){ [double]$matches[1]/1000 }
                 else { 0 }
            [uint64]$lsm
        }else{0}
        Mac=$mac
        IsVirtual=($a.Name -match 'Hyper-V|VMware|Bluetooth|Loopback|vEthernet|Virtual|VirtualBox|VPN|TAP-Win|WSL|Tunnel' -or $a.Description -match 'Hyper-V|VMware|Bluetooth|Loopback|vEthernet|Virtual|VirtualBox|VPN|TAP-Win|WSL|Tunnel')
        Ip=if($c){@($c.Ip)}else{@()}
        Gateway=if($c){@($c.Gateway)}else{@()}
        Dns=if($c){@($c.Dns)}else{@()}
    }
})
$out | ConvertTo-Json -Depth 5 -Compress
"#;
    let arr = json_arr_of(script, "网卡清单")?;
    let mut rows = Vec::new();
    for v in arr {
        let name = v_str(&v, "Name");
        if name.is_empty() {
            continue;
        }
        let desc = v_str(&v, "Description");
        let status = get_int(&v, "Status").unwrap_or(2) as i32;
        // PS 的 LinkSpeed 已是 Mbps，转 bps（×1e6，不是 ×8）
        let link_mbps = get_int(&v, "LinkSpeed").unwrap_or(0) as u64;
        rows.push(AdapterRow {
            name,
            desc,
            status,
            speed_bps: link_mbps.saturating_mul(1_000_000),
            mac: v_str(&v, "Mac"),
            ips: json_list_to_vec(&v, "Ip"),
            gateway: json_list_to_vec(&v, "Gateway"),
            dns: json_list_to_vec(&v, "Dns"),
            is_virtual: matches!(v.get("IsVirtual"), Some(Value::Bool(true))),
        });
    }
    Ok(rows)
}

#[cfg(windows)]
pub(crate) fn json_list_to_vec(v: &Value, k: &str) -> Vec<String> {
    v.get(k)
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .filter(|s| !s.trim().is_empty())
                .map(String::from)
                .collect()
        })
        .unwrap_or_default()
}
