//! 磁盘域工具处理器（从 tools/mod.rs 拆分，2026-10-12）。
//!
//! 原 mod.rs 的 `#[tool_router] impl AgentServer` 拆为 disk/sys/hw/misc 四个
//! 平级 impl 块，各自生成独立 router，mod.rs 用 `+` 合并后交给 `#[tool_handler]`。

use crate::tools::params::*;
use crate::tools::{blocking_call, blocking_call_progress, fmt_bytes, tool_error, AgentServer};
use crate::tools::{disk, diskx, files, hw};
use rmcp::{
    handler::server::wrapper::Parameters, model::CallToolResult, tool, tool_router,
    ErrorData as McpError,
};

#[tool_router(router = tool_router_disk, vis = "pub(crate)")]
impl AgentServer {
    #[tool(
        description = "列出本机所有磁盘的总/已用/可用空间与使用率（只读，实时读取）。用户问磁盘空间/哪块盘快满了/该清哪里之前先调用。"
    )]
    async fn disk_health(
        &self,
        _p: Parameters<DiskHealthParams>,
    ) -> Result<CallToolResult, McpError> {
        Ok(blocking_call(
            30,
            || -> Result<String, String> {
                let disks = disk::collect_disks();
                if disks.is_empty() {
                    return Err("未检测到可用磁盘。".into());
                }
                let lines = disks
                    .iter()
                    .map(|d| {
                        format!(
                            "- {}：总 {} / 已用 {}（{:.1}%）/ 可用 {}",
                            d.drive,
                            fmt_bytes(d.total_bytes),
                            fmt_bytes(d.used_bytes),
                            d.used_percent,
                            fmt_bytes(d.free_bytes)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                // 映射盘（subst）会被 GetLogicalDrives 数成一个盘符，但它没有自己的
                // 分区与空间：不说清楚，AI 会把同一份可用空间算两遍、把 7 个盘符说成 7 块盘。
                let mut notes = String::new();
                for (letter, target) in disk::collect_substs() {
                    let real = disk::path_drive_letter(&target);
                    let note = if target.contains('\u{FFFD}') {
                        // subst.exe 按控制台代码页输出，中文路径解码不出就不贴乱码，
                        // 只说明它落在哪块盘上。
                        if real.is_empty() {
                            format!("\n映射盘 {letter}: 是映射盘（不是独立物理盘，目标卷未识别）")
                        } else {
                            format!(
                                "\n映射盘 {letter}: 实际位于 {real}: 盘上（不是独立物理盘，空间与 {real}: 同一份）"
                            )
                        }
                    } else if real.is_empty() {
                        format!("\n映射盘 {letter}: → {target}（不是独立物理盘）")
                    } else {
                        format!(
                            "\n映射盘 {letter}: → {target}（不是独立物理盘，空间与 {real}: 同一份，别按两块盘重复计算）"
                        )
                    };
                    notes.push_str(&note);
                }
                Ok(format!(
                    "磁盘概览（{} 个分区）：\n{lines}{notes}",
                    disks.len()
                ))
            },
            "磁盘概览读取",
        )
        .await)
    }

    #[tool(
        description = "返回某路径所在卷（分区）的用量：总/已用/可用字节与使用率（只读）。比 disk_health 更细粒度——同一盘上不同挂载点用量可能不同。"
    )]
    async fn disk_partition_usage(
        &self,
        Parameters(p): Parameters<PartitionUsageParams>,
    ) -> Result<CallToolResult, McpError> {
        let path = p.path.clone();
        Ok(blocking_call(
            30,
            move || -> Result<String, String> {
                let pb = std::path::PathBuf::from(&path);
                match disk::collect_volume_usage(&pb) {
                    Some(d) => Ok(format!(
                        "{}（卷）：总 {} / 已用 {}（{:.1}%）/ 可用 {}",
                        d.drive,
                        fmt_bytes(d.total_bytes),
                        fmt_bytes(d.used_bytes),
                        d.used_percent,
                        fmt_bytes(d.free_bytes)
                    )),
                    None => Err(format!("无法读取 {path} 所在卷的用量")),
                }
            },
            "分区用量读取",
        )
        .await)
    }

    #[tool(
        description = "列出每个分区的类型元数据（只读）：本地磁盘/可移动/网络/光驱 + 文件系统（NTFS/exFAT/FAT32）+ 卷标。AI 回答『这个盘是什么盘/能不能插拔』时用。"
    )]
    async fn disk_volume_meta(
        &self,
        _p: Parameters<VolumeMetaParams>,
    ) -> Result<CallToolResult, McpError> {
        Ok(blocking_call(
            30,
            || -> Result<String, String> {
                let metas = disk::collect_volume_meta();
                if metas.is_empty() {
                    return Err("未检测到分区元数据。".into());
                }
                let lines = metas
                    .iter()
                    .map(|m| {
                        format!(
                            "- {}：{}，{}，卷标「{}」",
                            m.drive,
                            m.drive_type,
                            if m.fs.is_empty() {
                                "未知文件系统"
                            } else {
                                &m.fs
                            },
                            if m.label.is_empty() { "无" } else { &m.label }
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                Ok(format!("分区类型（{} 个）：\n{lines}", metas.len()))
            },
            "分区元数据读取",
        )
        .await)
    }

    #[tool(
        description = "递归统计某目录下按扩展名聚合的文件数与占用字节（只读）。AI 分析『什么文件占了空间』时用：返回 .mp4/.dll/.log 等各占多少。带硬上限（20 万文件/20 层深度），超限会标记 truncated。"
    )]
    async fn file_type_stats(
        &self,
        Parameters(p): Parameters<FileTypeStatsParams>,
    ) -> Result<CallToolResult, McpError> {
        let guard = files::PathGuard;
        let path = std::path::PathBuf::from(&p.path);
        if let Err(e) = guard.check_read(&path) {
            return Ok(tool_error(format!("路径被安全守卫拒绝：{e}")));
        }
        let top_n = p.top_n.unwrap_or(20).clamp(1, 100);
        let path_str = p.path.clone();
        Ok(blocking_call(
            45,
            move || -> Result<String, String> {
                let pb = std::path::PathBuf::from(&path_str);
                let (stats, total, truncated) = disk::file_type_stats(&pb, top_n);
                if stats.is_empty() {
                    return Ok(format!("{path_str} 下没有可统计的文件（或目录为空）"));
                }
                let lines = stats
                    .iter()
                    .map(|s| {
                        format!(
                            "- .{}：{} 个文件，共 {}",
                            s.ext,
                            s.count,
                            fmt_bytes(s.bytes)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                let mut head = format!(
                    "{path_str} 按扩展名统计（合计 {}）：\n{lines}",
                    fmt_bytes(total)
                );
                if truncated {
                    head.push_str("\n（已达遍历上限，结果不完整）");
                }
                Ok(head)
            },
            "文件类型统计",
        )
        .await)
    }

    #[tool(
        description = "列出目录树（深度限制，带文件大小；只读）。AI 想一眼看清『某目录下结构长什么样』时用，比 list_dir 更宏观。安全边界：盘根/系统目录/主目录根会被拒绝。"
    )]
    async fn file_tree(
        &self,
        Parameters(p): Parameters<FileTreeParams>,
    ) -> Result<CallToolResult, McpError> {
        let guard = files::PathGuard;
        let path = std::path::PathBuf::from(&p.path);
        if let Err(e) = guard.check_read(&path) {
            return Ok(tool_error(format!("路径被安全守卫拒绝：{e}")));
        }
        let max_depth = p.max_depth.unwrap_or(3).clamp(1, 10);
        let max_nodes = p.max_nodes.unwrap_or(100).clamp(1, 500);
        let path_str = p.path.clone();
        Ok(blocking_call(
            45,
            move || -> Result<String, String> {
                let pb = std::path::PathBuf::from(&path_str);
                match files::file_tree(&pb, max_depth, max_nodes) {
                    Ok((nodes, truncated)) => {
                        if nodes.is_empty() {
                            return Ok(format!("{path_str} 是空目录或不可读"));
                        }
                        let lines = nodes
                            .iter()
                            .map(|n| {
                                let pad = "  ".repeat(n.depth);
                                if n.is_dir {
                                    format!("{pad}[目录] {}", n.name)
                                } else {
                                    format!(
                                        "{pad}[文件] {}（{}）",
                                        n.name,
                                        n.size_bytes
                                            .map(fmt_bytes)
                                            .unwrap_or_else(|| "未知".into())
                                    )
                                }
                            })
                            .collect::<Vec<_>>()
                            .join("\n");
                        let mut s = format!("{path_str} 目录树：\n{lines}");
                        if truncated {
                            s.push_str(&format!(
                                "\n（已达 {} 条/{} 层上限）",
                                max_nodes, max_depth
                            ));
                        }
                        Ok(s)
                    }
                    Err(e) => Err(e),
                }
            },
            "目录树读取",
        )
        .await)
    }

    #[tool(
        description = "统计某目录的**直接子目录**占用 Top N（含各自全部子级大小，只读，单遍并行遍历）。用户问『哪个文件夹占了空间/什么目录最大』时比单文件视角更实用。安全边界：盘根/系统目录/主目录根会被拒绝。"
    )]
    async fn disk_top_directories(
        &self,
        Parameters(p): Parameters<TopDirectoriesParams>,
        ctx: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let guard = files::PathGuard;
        let path = std::path::PathBuf::from(&p.path);
        if let Err(e) = guard.check_read(&path) {
            return Ok(tool_error(format!("路径被安全守卫拒绝：{e}")));
        }
        Ok(blocking_call_progress(
            45,
            move |slot| {
                let cb = |seen: usize, total: usize| {
                    if let Some(v) = (seen * 1000).checked_div(total) {
                        slot.set(v as u64);
                    }
                };
                diskx::collect_top_directories(p.path.clone(), p.top_n.unwrap_or(20), Some(&cb))
            },
            "子目录占用统计",
            &ctx,
        )
        .await)
    }

    #[tool(
        description = "读取磁盘可靠性计数器明细：每块物理盘的通电时长/温度/磨损/累计读写错误/启停循环（只读）。注意：这是 Get-StorageReliabilityCounter 可靠性计数，**不是**标准 SMART 属性表（ID 0x05 重分配扇区等需 smartctl，本工具不装）；普通权限下 CIM 常拒绝读取计数（实测「Access to a CIM resource was not available to the client」），届时输出只有健康状态并说明原因——别据此向用户承诺读到了磨损/通电时长，想看容量与健康用 disk_health、想看温度用 hw_temperature。用户问『硬盘具体磨损多少/通电多久的明细』时调用。"
    )]
    async fn disk_smart_raw_attributes(
        &self,
        _p: Parameters<DiskSmartRawParams>,
    ) -> Result<CallToolResult, McpError> {
        Ok(blocking_call(45, hw::collect_disk_smart_raw, "磁盘可靠性计数读取").await)
    }

    // ── 安全/审计（sec.rs）─────────────────────────────────────────

    #[tool(
        description = "列出某目录下最大的文件（递归，只读）。安全边界：盘根/系统目录/主目录根会被拒绝。用户问『什么文件占了空间/哪个文件最大』时调用。"
    )]
    async fn disk_find_biggest_files(
        &self,
        Parameters(p): Parameters<BiggestFilesParams>,
        ctx: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let guard = files::PathGuard;
        let path = std::path::PathBuf::from(&p.path);
        if let Err(e) = guard.check_read(&path) {
            return Ok(tool_error(format!("路径被安全守卫拒绝：{e}")));
        }
        Ok(blocking_call_progress(
            45,
            move |slot| {
                let cb = |seen: usize, total: usize| {
                    if let Some(p) = (seen * 1000).checked_div(total) {
                        slot.set(p as u64);
                    }
                };
                diskx::collect_biggest_files(p.path.clone(), p.top_n.unwrap_or(20), Some(&cb))
            },
            "大文件扫描",
            &ctx,
        )
        .await)
    }

    #[tool(
        description = "找出某目录下疑似重复的文件（递归，按大小分组并按头部 64KB 哈希比对；只读。只比对头部 64KB，不是全文件哈希）。安全边界：盘根/系统目录/主目录根会被拒绝。用户问『哪些文件重复了/有什么可删的重复文件』时调用。"
    )]
    async fn disk_find_duplicate_files(
        &self,
        Parameters(p): Parameters<DuplicateFilesParams>,
        ctx: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let guard = files::PathGuard;
        let path = std::path::PathBuf::from(&p.path);
        if let Err(e) = guard.check_read(&path) {
            return Ok(tool_error(format!("路径被安全守卫拒绝：{e}")));
        }
        Ok(blocking_call_progress(
            45,
            move |slot| {
                let cb = |seen: usize, total: usize| {
                    if let Some(p) = (seen * 1000).checked_div(total) {
                        slot.set(p as u64);
                    }
                };
                diskx::collect_duplicate_files(p.path.clone(), p.top_n.unwrap_or(20), Some(&cb))
            },
            "重复文件扫描",
            &ctx,
        )
        .await)
    }

    #[tool(
        description = "读取磁盘实时 IO 使用率：每块盘的活动时间百分比与读写速率（只读，需要跑一次 PowerShell 采样，约 1 秒）。用户问『磁盘是不是满了/卡了/谁在读写盘』时调用。"
    )]
    async fn disk_io_usage(
        &self,
        Parameters(p): Parameters<DiskIoUsageParams>,
    ) -> Result<CallToolResult, McpError> {
        let sample_ms = p.sample_ms.unwrap_or(1000);
        Ok(blocking_call(
            45,
            move || diskx::collect_disk_io_usage(sample_ms),
            "磁盘 IO 采样",
        )
        .await)
    }

    // ── 自修复（selfheal.rs，跨机器环境匹配）────────────────────────
}
