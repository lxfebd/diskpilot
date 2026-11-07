//! drive-health —— DiskPilot 官方插件（只读诊断工具，Windows）。
//!
//! 用法: drive-health.exe [--smart]
//!
//! 通过 PowerShell 存储模块做只读健康审计：
//!   1. Get-PhysicalDisk：型号 / 介质(SSD|HDD) / 总线 / HealthStatus /
//!      OperationalStatus / 容量 —— 任何盘 HealthStatus != Healthy 立即标红。
//!   2. Get-Disk：逻辑盘号 / 卷标关联 / 健康状态。
//!   3. --smart 追加 Get-StorageAdvancedProperty（SATA 盘的 SMART 属性表；
//!      NVMe 盘或非管理员下该 cmdlet 不可用，届时明确标注而不是静默跳过）。
//! 只做查询与报告，**不做任何磁盘写入、不改电源计划、不跑 chkdsk**。

use std::env;
use std::process::Command;
use std::time::Instant;

#[derive(Debug, Clone, PartialEq, Eq)]
struct PhysicalDisk {
    name: String,
    media: String,
    bus: String,
    health: String,
    op_status: String,
    size_bytes: u64,
}

/// 解析 ConvertTo-Csv -NoTypeInformation 输出（带引号字段，字段内逗号/引号按 CSV 转义）。
fn parse_csv(out: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    for line in out.lines() {
        let line = line.trim_end_matches('\r').trim();
        if line.is_empty() {
            continue;
        }
        let mut row = Vec::new();
        let mut cur = String::new();
        let mut in_quotes = false;
        let mut chars = line.chars().peekable();
        while let Some(c) = chars.next() {
            if in_quotes {
                if c == '"' {
                    if chars.peek() == Some(&'"') {
                        cur.push('"');
                        chars.next();
                    } else {
                        in_quotes = false;
                    }
                } else {
                    cur.push(c);
                }
            } else if c == '"' {
                in_quotes = true;
            } else if c == ',' {
                row.push(std::mem::take(&mut cur));
            } else {
                cur.push(c);
            }
        }
        row.push(cur);
        rows.push(row);
    }
    rows
}

fn run_ps(script: &str) -> Option<String> {
    let out = Command::new("powershell")
        .args(["-NoProfile", "-Command", script])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn parse_physical_disks(out: &str) -> Vec<PhysicalDisk> {
    let rows = parse_csv(out);
    let Some(header) = rows.first() else { return vec![] };
    let col = |row: &[String], name: &str| -> String {
        header
            .iter()
            .position(|h| h == name)
            .and_then(|i| row.get(i))
            .cloned()
            .unwrap_or_default()
    };
    rows.iter()
        .skip(1)
        .filter(|r| r.len() == header.len() && !r[0].is_empty())
        .map(|r| PhysicalDisk {
            name: col(r, "FriendlyName"),
            media: col(r, "MediaType"),
            bus: col(r, "BusType"),
            health: col(r, "HealthStatus"),
            op_status: col(r, "OperationalStatus"),
            size_bytes: col(r, "Size").parse().unwrap_or(0),
        })
        .collect()
}

fn fmt_size(n: u64) -> String {
    const GB: u64 = 1024 * 1024 * 1024;
    const TB: u64 = GB * 1024;
    if n >= TB {
        format!("{:.2} TB", n as f64 / TB as f64)
    } else {
        format!("{:.1} GB", n as f64 / GB as f64)
    }
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!("drive-health —— 磁盘健康审计器（只读，不写盘不改配置）");
        println!("用法: drive-health.exe [--smart]");
        println!("  默认输出物理盘健康表；--smart 追加 SATA 盘 SMART 属性（NVMe/非管理员下标注不可用）。");
        return;
    }
    let want_smart = args.iter().any(|a| a == "--smart");

    let t0 = Instant::now();
    let ps_disks = run_ps(
        "Get-PhysicalDisk | Select-Object FriendlyName,MediaType,BusType,HealthStatus,OperationalStatus,Size | ConvertTo-Csv -NoTypeInformation",
    )
    .unwrap_or_else(|| {
        eprintln!("错误: 无法运行 powershell Get-PhysicalDisk（需要 Windows 存储模块）");
        std::process::exit(2);
    });
    let disks = parse_physical_disks(&ps_disks);

    let ps_logical = run_ps(
        "Get-Disk | Select-Object Number,FriendlyName,HealthStatus,BusType,Size | ConvertTo-Csv -NoTypeInformation",
    )
    .unwrap_or_default();
    let logical = parse_csv(&ps_logical);

    let unhealthy = disks.iter().filter(|d| d.health != "Healthy").count();
    println!(
        "磁盘健康审计: {} 块物理盘，{} 块非 Healthy，耗时 {:.2}s",
        disks.len(),
        unhealthy,
        t0.elapsed().as_secs_f64()
    );
    if disks.is_empty() {
        println!("(未读到任何物理盘——可能缺少权限或存储模块)");
        return;
    }
    println!("\n物理盘（Get-PhysicalDisk）:");
    println!("{:>4} {:>28} {:>5} {:>6} {:>9} {:>10}  {}", "#", "型号", "介质", "总线", "健康", "运行态", "容量");
    for (i, d) in disks.iter().enumerate() {
        let flag = if d.health != "Healthy" { " <-- 异常!" } else { "" };
        println!(
            "{:>4} {:>28} {:>5} {:>6} {:>9} {:>10}  {}{flag}",
            i,
            truncate(&d.name, 28),
            d.media,
            d.bus,
            d.health,
            d.op_status,
            fmt_size(d.size_bytes),
        );
    }

    if !logical.is_empty() {
        println!("\n逻辑盘（Get-Disk）:");
        println!("{:>4} {:>28} {:>9} {:>6}  {}", "盘号", "名称", "健康", "总线", "容量");
        let header = &logical[0];
        for r in logical.iter().skip(1) {
            if r.len() != header.len() || r[0].is_empty() {
                continue;
            }
            let get = |name: &str| {
                header
                    .iter()
                    .position(|h| h == name)
                    .and_then(|i| r.get(i))
                    .map(|s| s.as_str())
                    .unwrap_or("")
            };
            let size: u64 = get("Size").parse().unwrap_or(0);
            println!(
                "{:>4} {:>28} {:>9} {:>6}  {}",
                get("Number"),
                truncate(get("FriendlyName"), 28),
                get("HealthStatus"),
                get("BusType"),
                fmt_size(size)
            );
        }
    }

    if want_smart {
        println!("\nSMART 高级属性（Get-StorageAdvancedProperty，尽力而为）:");
        // 只对非 NVMe 总线的盘尝试（NVMe 走 NVMe log page，该 cmdlet 不支持）。
        for d in disks.iter().filter(|d| d.bus != "NVMe") {
            let script = format!(
                "$d = Get-Disk | Where-Object {{ $_.FriendlyName -eq '{}' }}; \
                 if ($d) {{ $d | Get-StorageAdvancedProperty | Select-Object -First 12 | ConvertTo-Csv -NoTypeInformation }} else {{ '盘未找到' }}",
                d.name.replace('\'', "''")
            );
            match run_ps(&script) {
                Some(out) if out.contains(',') => {
                    println!("  [{}] ({}):", d.name, d.bus);
                    for line in out.lines().take(13) {
                        println!("    {}", line.trim_end());
                    }
                }
                _ => {
                    println!("  [{}]: SMART 属性不可用（需管理员 + ATA/SCSI 驱动支持，NVMe 盘不适用）", d.name);
                }
            }
        }
    }

    if unhealthy > 0 {
        println!("\n警告: 有 {} 块物理盘健康状态非 Healthy——建议尽快备份数据并安排换盘。", unhealthy);
    } else {
        println!("\n全部物理盘 HealthStatus=Healthy。");
    }
    println!("提示: 本工具只读。如需做磁盘维护（碎片整理/chkdsk），请在 DiskPilot 主程序里走「确认后执行」流程。");
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(max - 1).collect();
        t.push('…');
        t
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_quoted_csv_with_commas() {
        let sample = "\"FriendlyName\",\"MediaType\",\"Size\"\n\"WDC WD20EZRX, 2TB\",\"HDD\",\"2000398934016\"\n\"NVME 4.0 1TB\",\"SSD\",\"1024209543168\"\n";
        let rows = parse_csv(sample);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[1][0], "WDC WD20EZRX, 2TB");
        assert_eq!(rows[2][2], "1024209543168");
    }

    #[test]
    fn parses_physical_disk_rows() {
        let sample = "\"FriendlyName\",\"MediaType\",\"BusType\",\"HealthStatus\",\"OperationalStatus\",\"Size\"\n\"SN770 1TB\",\"SSD\",\"NVMe\",\"Healthy\",\"OK\",\"1000204886016\"\n\"WD20EZRX\",\"HDD\",\"SATA\",\"Warning\",\"Pred Fail\",\"2000398934016\"\n";
        let disks = parse_physical_disks(sample);
        assert_eq!(disks.len(), 2);
        assert_eq!(disks[0].health, "Healthy");
        assert_eq!(disks[1].health, "Warning");
        assert_eq!(disks[1].op_status, "Pred Fail");
        assert_eq!(disks[1].size_bytes, 2000398934016);
    }

    #[test]
    fn skips_header_mismatch_rows() {
        let sample = "\"A\",\"B\"\n\"x\"\n\"y\",\"z\"\n";
        let rows = parse_csv(sample);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[1].len(), 1, "短行保留但下游按列数过滤");
    }

    #[test]
    fn fmt_size_units() {
        assert_eq!(fmt_size(1024 * 1024 * 1024), "1.0 GB");
        assert_eq!(fmt_size(12_000_138_625_024), "10.91 TB");
    }
}
