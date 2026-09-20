//! MCP 工具路由（rmcp 0.4.1 API）：
//!
//! - 每个工具 = 一个参数 struct（derive Deserialize + JsonSchema）
//! - 每个工具 = 一个带 `#[tool(description)]` 的方法
//!
//! 纯逻辑在子模块（disk/files/process/system/apps），这里只做「参数 →
//! 纯函数 → 格式化结果」薄壳。

pub mod apps;
pub mod bench;
pub mod bsod;
pub mod cleanup;
pub mod disk;
pub mod diskx;
pub mod envx;
pub mod files;
pub mod hw;
pub mod hwinfo;
pub mod net;
pub mod process;
pub mod report;
pub mod sec;
pub mod selfheal;
pub mod steam;
pub mod superio;
pub mod sys;
pub mod system;
pub mod toolbelt;

mod lenient;

use rmcp::{
    handler::server::{
        tool::{Parameters, ToolRouter},
        ServerHandler,
    },
    model::{AnnotateAble, CallToolResult, RawContent, ServerCapabilities, ServerInfo},
    schemars, tool, tool_handler, tool_router, ErrorData as McpError,
};
use serde::Deserialize;
// rmcp 宏展开引用裸 `Future`，需入作用域
use std::future::Future;

fn text_result(s: String) -> CallToolResult {
    CallToolResult::success(vec![RawContent::text(s).no_annotation()])
}

fn tool_error(msg: impl Into<String>) -> CallToolResult {
    CallToolResult::error(vec![RawContent::text(msg.into()).no_annotation()])
}

/// 统一「阻塞任务」执行壳：spawn_blocking 把同步代码移出 tokio 运行时线程
/// （避免单点卡死拖垮整个 MCP 服务）+ tokio timeout 外层兜底（任何阻塞调用
/// 最长 secs 秒，挂起即返回错误不 hang 死 AI 调用链路）。
async fn blocking_call<T>(
    secs: u64,
    f: impl FnOnce() -> Result<T, String> + Send + 'static,
    label: &str,
) -> CallToolResult
where
    T: std::fmt::Display + Send + 'static,
{
    match tokio::time::timeout(
        std::time::Duration::from_secs(secs),
        tokio::task::spawn_blocking(f),
    )
    .await
    {
        Ok(Ok(Ok(v))) => text_result(v.to_string()),
        Ok(Ok(Err(e))) => tool_error(e),
        Ok(Err(_)) => tool_error(format!("{label}线程异常退出")),
        Err(_) => tool_error(format!("{label}执行超时（>{secs}s），已中断防止挂起")),
    }
}

/// 字节数人类可读（与主项目 formatBytes 同语义）。
pub fn fmt_bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = n as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 {
        format!("{n} B")
    } else {
        format!("{v:.1} {}", UNITS[u])
    }
}

#[derive(Debug, Clone)]
pub struct AgentServer {
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl AgentServer {
    pub fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }

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
                Ok(format!("磁盘概览（{} 个分区）：\n{lines}", disks.len()))
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
        if let Err(e) = guard.check(&path) {
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
        if let Err(e) = guard.check(&path) {
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
        description = "读取本机系统信息与实时状态：系统版本/CPU 逻辑核心/内存总量与使用率/开机时长（只读）。用户问『电脑什么配置/现在卡不卡/帮我看看电脑状态』时调用。"
    )]
    async fn system_info(
        &self,
        _p: Parameters<SystemInfoParams>,
    ) -> Result<CallToolResult, McpError> {
        Ok(blocking_call(
            30,
            || -> Result<String, String> {
                let s = system::collect_system_info();
                let up_days = s.uptime_secs / 86400;
                let up_hrs = (s.uptime_secs % 86400) / 3600;
                Ok(format!(
                "系统：{}\nCPU 逻辑核心：{}\n内存：{}，已用 {}（{:.1}%）\n开机时长：{} 天 {} 小时",
                s.os,
                s.cpu_cores,
                fmt_bytes(s.mem_total_bytes),
                fmt_bytes(s.mem_used_bytes),
                s.mem_percent,
                up_days,
                up_hrs
            ))
            },
            "系统信息读取",
        )
        .await)
    }

    #[tool(
        description = "列出某目录的直接子项（不递归）：名称/是否目录/文件大小。AI 核实『目录里到底有什么』时用。安全边界：盘根/系统目录/主目录根会被拒绝。"
    )]
    async fn list_dir(
        &self,
        Parameters(p): Parameters<ListDirParams>,
    ) -> Result<CallToolResult, McpError> {
        let guard = files::PathGuard;
        let path = std::path::PathBuf::from(&p.path);
        if let Err(e) = guard.check(&path) {
            return Ok(tool_error(format!("路径被安全守卫拒绝：{e}")));
        }
        let limit = p.limit.unwrap_or(100).clamp(1, 500);
        let path_str = p.path.clone();
        Ok(blocking_call(
            45,
            move || -> Result<String, String> {
                let pb = std::path::PathBuf::from(&path_str);
                match files::list_dir(&pb, limit) {
                    Ok((items, truncated)) => {
                        if items.is_empty() {
                            return Ok(format!("{path_str} 是空目录或不可读"));
                        }
                        let lines = items
                            .iter()
                            .map(|e| {
                                if e.is_dir {
                                    format!("[目录] {}", e.name)
                                } else {
                                    format!(
                                        "[文件] {}（{}）",
                                        e.name,
                                        e.size_bytes
                                            .map(fmt_bytes)
                                            .unwrap_or_else(|| "未知".into())
                                    )
                                }
                            })
                            .collect::<Vec<_>>()
                            .join("\n");
                        let mut s = format!("{path_str}（{} 项）：\n{lines}", items.len());
                        if truncated {
                            s.push_str(&format!("\n（只显示前 {} 项）", limit));
                        }
                        Ok(s)
                    }
                    Err(e) => Err(e),
                }
            },
            "目录列表读取",
        )
        .await)
    }

    #[tool(
        description = "读取文本文件内容（只读，UTF-8/UTF-16 自动识别，超 256KB 截断）。AI 核实某个配置文件/脚本/日志内容时用。只接受文件，拒绝目录。"
    )]
    async fn read_file(
        &self,
        Parameters(p): Parameters<ReadFileParams>,
    ) -> Result<CallToolResult, McpError> {
        let guard = files::PathGuard;
        let path = std::path::PathBuf::from(&p.path);
        if let Err(e) = guard.check(&path) {
            return Ok(tool_error(format!("路径被安全守卫拒绝：{e}")));
        }
        let path_str = p.path.clone();
        Ok(blocking_call(
            30,
            move || -> Result<String, String> {
                let pb = std::path::PathBuf::from(&path_str);
                match files::read_text_file(&pb) {
                    Ok((content, truncated)) => {
                        let mut s = format!("【{path_str}】\n{content}");
                        if truncated {
                            s.push_str("\n（内容已截断到 256KB）");
                        }
                        Ok(s)
                    }
                    Err(e) => Err(e),
                }
            },
            "文件读取",
        )
        .await)
    }

    #[tool(
        description = "在指定目录下按文件名关键字递归查找文件/目录（只读，大小写不敏感，带深度/数量上限）。AI 找『某个文件在哪』时用。安全边界：盘根/系统目录/主目录根会被拒绝。"
    )]
    async fn find_files(
        &self,
        Parameters(p): Parameters<FindFilesParams>,
    ) -> Result<CallToolResult, McpError> {
        let guard = files::PathGuard;
        let root = std::path::PathBuf::from(&p.root);
        if let Err(e) = guard.check(&root) {
            return Ok(tool_error(format!("路径被安全守卫拒绝：{e}")));
        }
        let max_hits = p.max_hits.unwrap_or(50).clamp(1, 500);
        let root_str = p.root.clone();
        let keyword = p.keyword.clone();
        Ok(blocking_call(
            45,
            move || -> Result<String, String> {
                let rb = std::path::PathBuf::from(&root_str);
                match files::find_files(&rb, &keyword, max_hits) {
                    Ok((hits, truncated)) => {
                        if hits.is_empty() {
                            return Ok(format!("{root_str} 下没有文件名包含「{keyword}」的项目"));
                        }
                        let lines = hits
                            .iter()
                            .map(|h| {
                                if h.is_dir {
                                    format!("[目录] {}", h.path)
                                } else {
                                    format!(
                                        "[文件] {}（{}）",
                                        h.path,
                                        h.size_bytes
                                            .map(fmt_bytes)
                                            .unwrap_or_else(|| "未知".into())
                                    )
                                }
                            })
                            .collect::<Vec<_>>()
                            .join("\n");
                        let mut s = format!("命中 {} 项：\n{lines}", hits.len());
                        if truncated {
                            s.push_str(&format!("\n（已达 {} 条上限，还有更多）", max_hits));
                        }
                        Ok(s)
                    }
                    Err(e) => Err(e),
                }
            },
            "文件查找",
        )
        .await)
    }

    #[tool(
        description = "列出本机进程（只读）：PID/名称/内存占用/可执行文件路径，按内存占用降序。用户问『有什么程序在跑/哪个进程占内存/某进程存在吗』时调用。"
    )]
    async fn list_processes(
        &self,
        Parameters(p): Parameters<ListProcessesParams>,
    ) -> Result<CallToolResult, McpError> {
        let top_n = p.top_n.unwrap_or(50).clamp(1, 500);
        Ok(blocking_call(
            45,
            move || -> Result<String, String> {
                let procs = process::list_processes(top_n);
                if procs.is_empty() {
                    return Err("无法枚举进程（非 Windows 环境或权限不足）。".into());
                }
                let lines = procs
                    .iter()
                    .map(|pr| {
                        format!(
                            "- PID {}：{}（内存 {}，路径 {}）",
                            pr.pid,
                            pr.name,
                            fmt_bytes(pr.mem_bytes),
                            if pr.exe_path.is_empty() {
                                "未知".into()
                            } else {
                                pr.exe_path.clone()
                            }
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                Ok(format!(
                    "进程列表（{} 个，按内存降序）：\n{lines}",
                    procs.len()
                ))
            },
            "进程列表读取",
        )
        .await)
    }

    #[tool(
        description = "查询单个进程的详情（只读）：PID/名称/内存/可执行文件路径/父进程 PID。AI 回答『这个进程是什么/谁拉起的』时用，找不到返回提示。"
    )]
    async fn process_info(
        &self,
        Parameters(p): Parameters<ProcessInfoParams>,
    ) -> Result<CallToolResult, McpError> {
        let pid = p.pid;
        Ok(blocking_call(
            45,
            move || -> Result<String, String> {
                match process::process_info(pid) {
                    Some(pr) => Ok(format!(
                        "PID {}：{}\n内存占用：{}\n可执行文件：{}\n父进程 PID：{}",
                        pr.pid,
                        pr.name,
                        fmt_bytes(pr.mem_bytes),
                        if pr.exe_path.is_empty() {
                            "未知".into()
                        } else {
                            pr.exe_path.clone()
                        },
                        pr.parent_pid
                    )),
                    None => Err(format!("未找到 PID {pid} 的进程")),
                }
            },
            "进程详情查询",
        )
        .await)
    }

    #[tool(
        description = "结束指定 PID 的进程（**写操作**）。安全守卫：系统关键进程（PID < 5）、系统目录中的进程会被拒绝。**此工具会改变系统状态，主项目侧会先弹确认门，用户确认后才真正执行**。AI 在用户明确要求『把某进程/某程序结束掉』时才调用；结束后建议用 list_processes 复查。"
    )]
    async fn process_kill(
        &self,
        Parameters(p): Parameters<ProcessKillParams>,
    ) -> Result<CallToolResult, McpError> {
        if let Err(e) = files::require_confirmed() {
            return Ok(tool_error(e));
        }
        let pid = p.pid;
        Ok(blocking_call(30, move || process::kill_process(pid), "结束进程").await)
    }

    #[tool(
        description = "启动一个已安装程序（**写操作，白名单执行**）。安全守卫：只允许系统目录或已安装程序目录（System32 / Program Files / LocalAppData 等）下的 .exe，命令必须存在，参数原样传入（不拼 shell 字符串）。**此工具会改变系统状态，主项目侧会先弹确认门，用户确认后才真正执行**。用户明确要求『打开/启动某程序』（如记事本、计算器、某已装软件）时调用；启动即返回（不等待进程退出），结束后可用 list_processes 复查。"
    )]
    async fn process_start(
        &self,
        Parameters(p): Parameters<ProcessStartParams>,
    ) -> Result<CallToolResult, McpError> {
        if let Err(e) = files::require_confirmed() {
            return Ok(tool_error(e));
        }
        let command = p.command.clone();
        let args = p.args.clone().unwrap_or_default();
        let cwd = p.cwd.clone().unwrap_or_default();
        Ok(blocking_call(
            30,
            move || process::start_process(&command, &args, &cwd),
            "启动进程",
        )
        .await)
    }

    #[tool(
        description = "把单个文件/目录移入系统回收站（**写操作，可逆**）。安全守卫：盘根 / 系统目录 / 用户主目录根会被拒绝。**此工具会改变系统状态，主项目侧会先弹确认门，用户确认后才真正执行**。AI 在用户明确要求『把某个文件/文件夹扔进回收站』（如确认是垃圾的临时文件、重复文件）时调用；只进回收站绝不直接删除，误删可从系统回收站还原。"
    )]
    async fn file_recycle(
        &self,
        Parameters(p): Parameters<FileRecycleParams>,
    ) -> Result<CallToolResult, McpError> {
        let path = p.path.clone();
        Ok(blocking_call(30, move || files::recycle_file(&path), "回收文件").await)
    }

    #[tool(
        description = "管理计划任务（**写操作，仅可逆动作**）：action 取 enable（启用）/ disable（禁用）/ query（只读查询当前状态）。安全守卫：系统内置任务（任务路径在 \\Microsoft\\ / \\Windows\\ / \\System32\\ 下）一律拒绝修改（query 仍放行）；不开放 delete / run；目标任务不存在则拒绝。**此工具会改变系统状态，主项目侧会先弹确认门，用户确认后才真正执行**。用户明确要求『禁用/启用某个计划任务』时调用；dry_run=true 只出计划不执行。"
    )]
    async fn scheduled_task_manage(
        &self,
        Parameters(p): Parameters<ScheduledTaskManageParams>,
    ) -> Result<CallToolResult, McpError> {
        if let Err(e) = files::require_confirmed() {
            return Ok(tool_error(e));
        }
        let name = p.name.clone();
        let action = p.action.clone();
        let dry_run = p.dry_run.unwrap_or(false);
        Ok(blocking_call(
            30,
            move || sys::manage_scheduled_task(&name, &action, dry_run),
            "计划任务管理",
        )
        .await)
    }

    #[tool(
        description = "列出本机已安装程序（只读，来自注册表 Uninstall 键）：名称/版本/发布者/安装位置/预估大小。AI 回答『这台电脑装了什么软件/这个软件能卸载吗』时调用。"
    )]
    async fn app_list(
        &self,
        Parameters(p): Parameters<AppListParams>,
    ) -> Result<CallToolResult, McpError> {
        let keyword = p.keyword.clone().unwrap_or_default().to_ascii_lowercase();
        let limit = p.limit.unwrap_or(50).clamp(1, 200);
        Ok(blocking_call(
            30,
            move || {
                let all = apps::list_installed_apps();
                if all.is_empty() {
                    return Err("未读取到已安装程序（非 Windows 环境或注册表不可读）。".to_string());
                }
                let mut matched: Vec<_> = if keyword.is_empty() {
                    all
                } else {
                    all.into_iter()
                        .filter(|a| {
                            a.name.to_ascii_lowercase().contains(&keyword)
                                || a.publisher.to_ascii_lowercase().contains(&keyword)
                        })
                        .collect()
                };
                let truncated = matched.len() > limit;
                matched.truncate(limit);
                let lines = matched
                    .iter()
                    .map(|a| {
                        let mut s = format!("- {}（{}）", a.name, a.version);
                        if !a.publisher.is_empty() {
                            s.push_str(&format!("，{}", a.publisher));
                        }
                        if let Some(mb) = a.estimated_size_mb {
                            if mb > 0 {
                                s.push_str(&format!("，约 {} MB", mb));
                            }
                        }
                        if !a.install_location.is_empty() {
                            s.push_str(&format!("，位于 {}", a.install_location));
                        }
                        s
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                let mut head = format!("已安装程序（{} 个）：\n{lines}", matched.len());
                if truncated {
                    head.push_str(&format!("\n（只显示前 {} 个）", limit));
                }
                Ok(head)
            },
            "已安装程序列表",
        )
        .await)
    }

    #[tool(
        description = "卸载已安装程序（**写操作**，启动其官方卸载器）。安全守卫：只允许已安装程序目录（Program Files / LocalAppData 等）下 UninstallString 指向的卸载器，卸载参数只允许静默白名单（/S /silent /quiet /qn 等），拒绝 runas/delete 等危险参数。**此工具会改变系统状态，主项目侧会先弹确认门，用户确认后才真正执行**。AI 在用户明确要求『卸载某软件』时先调 app_list 确认名称再调用；卸载器以分离方式启动（有自己界面/进度），完成后建议 app_list 复查。"
    )]
    async fn uninstall_app(
        &self,
        Parameters(p): Parameters<UninstallAppParams>,
    ) -> Result<CallToolResult, McpError> {
        if let Err(e) = files::require_confirmed() {
            return Ok(tool_error(e));
        }
        let name = p.name.clone();
        Ok(blocking_call(120, move || apps::uninstall_app(&name), "卸载程序").await)
    }

    #[tool(
        description = "查询软件许可证：Windows 激活状态（slmgr /xpr）+ Office 激活状态（只读，不修改任何系统状态）。用户问『这个系统正版吗/激活了吗/Office 什么版本』时调用。Windows 激活查询可能需要管理员权限，无权限时如实返回。"
    )]
    async fn app_licenses(
        &self,
        _p: Parameters<AppLicensesParams>,
    ) -> Result<CallToolResult, McpError> {
        Ok(blocking_call(45, apps::collect_licenses, "许可证查询").await)
    }

    // ── 硬件（hw.rs）──────────────────────────────────────────────

    #[tool(
        description = "读取 CPU 信息：型号/核心/线程数/基频与当前频率/缓存/实时占用率（只读，WMI，不需要管理员权限）。用户问『CPU 是什么/几个核/占用高不高』时调用。"
    )]
    async fn hw_cpu(&self, _p: Parameters<HwCpuParams>) -> Result<CallToolResult, McpError> {
        Ok(blocking_call(30, || hw::collect_cpu(), "CPU 信息采集").await)
    }

    #[tool(
        description = "读取显卡信息：型号/显存/驱动版本与日期/分辨率/刷新率，NVIDIA 卡附实时温度与占用（只读，不需要管理员权限）。用户问『什么显卡/显存多大/GPU 占用』时调用。"
    )]
    async fn hw_gpu(&self, _p: Parameters<HwGpuParams>) -> Result<CallToolResult, McpError> {
        Ok(blocking_call(30, || hw::collect_gpu(), "显卡信息读取").await)
    }

    #[tool(
        description = "读取内存信息：每根内存条容量/频率/厂商/型号 + 总容量 + XMP/EXPO 是否生效诊断（只读，不需要管理员权限）。用户问『内存多大/频率多少/该不该开 XMP』时调用。"
    )]
    async fn hw_memory(&self, _p: Parameters<HwMemoryParams>) -> Result<CallToolResult, McpError> {
        Ok(blocking_call(30, || hw::collect_memory(), "内存信息读取").await)
    }

    #[tool(
        description = "读取主板/整机信息：主板厂商与型号/BIOS 版本与日期/整机品牌型号（只读，不需要管理员权限）。用户问『这是什么主机/主板是什么』时调用。"
    )]
    async fn hw_motherboard(
        &self,
        _p: Parameters<HwMotherboardParams>,
    ) -> Result<CallToolResult, McpError> {
        Ok(blocking_call(30, || hw::collect_motherboard(), "主板信息读取").await)
    }

    #[tool(
        description = "读取本机温度：优先走 HWiNFO 共享内存（HWiNFO 后台运行时以普通权限发布 CPU/GPU/主板/磁盘全部温度，无需管理员）；HWiNFO 未运行时按顺序尝试自研 SuperIO 直读（inpoutx64 驱动已装则普通权限直读主板 CPU/主板温度，无需 HWiNFO/LHM）、ACPI 热区 + NVIDIA GPU 实时温度 + 磁盘 SMART（只读，传感器读不到会明确说明，不会假装有数据）。用户问『电脑温度/烫不烫』时调用。"
    )]
    async fn hw_temperature(
        &self,
        _p: Parameters<HwTemperatureParams>,
    ) -> Result<CallToolResult, McpError> {
        Ok(blocking_call(45, || hw::collect_temperature(), "温度读取").await)
    }

    #[tool(
        description = "自研 SuperIO 直读工具（只读 L0，复刻 HWiNFO/LibreHardwareMonitor 的纯端口直读能力）：动态加载本机已装的端口驱动（inpoutx64 优先，普通权限即可，无需管理员/UAC/Ring0），扫描 0x2E/0x4E 双端口识别 ITE/Nuvoton/Winbond SuperIO 芯片，直读环境控制器寄存器返回主板/CPU 温度 + 风扇转速 + PWM 占空比。HWiNFO 没装/没开共享内存且不想依赖 LHM 时用它兜底 CPU/主板传感器。用户问『主板读到的温度/风扇转速是什么』或 HWiNFO 不可用时调用。纯只读，不写任何寄存器、不调速。"
    )]
    async fn hw_superio(
        &self,
        _p: Parameters<HwSuperioParams>,
    ) -> Result<CallToolResult, McpError> {
        Ok(blocking_call(30, || superio::read_text(), "SuperIO 直读").await)
    }

    #[tool(
        description = "读取全量硬件传感器快照（AIDA64 同类）：CPU/GPU/主板温度、风扇转速、电压、功耗、频率、负载，按硬件分组输出（只读）。优先走 HWiNFO 共享内存（普通权限即可读全部传感器，无需管理员/Ring0）；HWiNFO 未运行/未启用共享内存时降级 LibreHardwareMonitor 内核（fancmd sensors），仍不可用再回退 ACPI 热区 + GPU + SMART 通道。用户问『温度/风扇/电压/功耗/整机健康』时调用，比 hw_temperature 更全。"
    )]
    async fn hw_sensors(
        &self,
        _p: Parameters<HwSensorsParams>,
    ) -> Result<CallToolResult, McpError> {
        // 同步采集 + 外层超时兜底：fancmd/UAC 提权/系统通道任何单点卡死
        // 都不能拖垮 AI 调用（25s 到点返回明确信息，不挂起）。
        match tokio::time::timeout(
            std::time::Duration::from_secs(25),
            tokio::task::spawn_blocking(move || -> Result<String, String> {
                hw::collect_sensors()
            }),
        )
        .await
        {
            Ok(Ok(Ok(s))) => Ok(text_result(s)),
            Ok(Ok(Err(e))) => Ok(tool_error(e)),
            Ok(Err(_)) => Ok(tool_error("硬件传感器采集线程异常退出")),
            Err(_) => Ok(tool_error(
                "硬件传感器采集超时（>25s，多为 UAC 等待或驱动扫描慢）；请重试或提权后直读",
            )),
        }
    }

    #[tool(
        description = "读取电池信息：电量百分比/电池健康度/充电状态/循环次数/设计容量（只读，不需要管理员权限；台式机返回『未检测到电池』）。用户问『笔记本电池健康吗/续航/要不要换电池』时调用。"
    )]
    async fn hw_battery(
        &self,
        _p: Parameters<HwBatteryParams>,
    ) -> Result<CallToolResult, McpError> {
        Ok(blocking_call(30, || hw::collect_battery(), "电池信息读取").await)
    }

    #[tool(
        description = "读取磁盘健康（SMART）：每块物理磁盘的容量/接口/健康状态/通电时间/温度/磨损与错误计数（只读，Win10 1607+ 普通权限即可，不需要管理员，不使用已移除的 wmic）。用户问『硬盘健康吗/通电多久/要坏了吗』时调用。"
    )]
    async fn hw_disk_smart(
        &self,
        _p: Parameters<HwDiskSmartParams>,
    ) -> Result<CallToolResult, McpError> {
        Ok(blocking_call(45, || hw::collect_disk_smart(), "磁盘健康读取").await)
    }

    // ── 网络（net.rs）─────────────────────────────────────────────

    #[tool(
        description = "读取本机网络状态：每个网卡名称/连接状态/IP/链接速率/是否虚拟（只读，不需要管理员权限）。用户问『电脑联网了吗/网卡是什么/IP 多少/网速多少』时调用。"
    )]
    async fn net_status(
        &self,
        _p: Parameters<NetStatusParams>,
    ) -> Result<CallToolResult, McpError> {
        Ok(blocking_call(30, || net::collect_net_status(), "网络状态读取").await)
    }

    #[tool(
        description = "读取网络连接列表：本地/远端地址与端口/状态/所属进程（只读，不需要管理员权限）。默认只看已建立的连接；可按 PID 或关键字（进程名/IP）过滤，也可查看全部。用户问『谁在连网/哪个程序在联网/某某连接是什么』时调用。"
    )]
    async fn net_connections(
        &self,
        Parameters(p): Parameters<NetConnectionsParams>,
    ) -> Result<CallToolResult, McpError> {
        let pid = p.pid;
        let keyword = p.keyword.clone();
        let top_n = p.top_n.unwrap_or(30);
        Ok(blocking_call(
            45,
            move || net::collect_net_connections(pid, keyword, top_n),
            "网络连接列表读取",
        )
        .await)
    }

    #[tool(
        description = "实测网络实时速率：采集 interval_ms 毫秒（1000-10000，默认 2000）窗口内的收发流量并换算为 KB/s（只读，需要跑一次 PowerShell 采样，约 2-10 秒）。用户问『当前网速快不快/下载多少兆』时调用。"
    )]
    async fn net_speed(
        &self,
        Parameters(p): Parameters<NetSpeedParams>,
    ) -> Result<CallToolResult, McpError> {
        let interval_ms = p.interval_ms.unwrap_or(2000);
        Ok(blocking_call(
            45,
            move || net::collect_net_speed(interval_ms),
            "网络速率采集",
        )
        .await)
    }

    #[tool(
        description = "读取本机共享文件夹（SMB 共享）列表：共享名/路径/权限类型（只读；会话/打开文件统计需要管理员权限，非管理员会明确降级说明）。用户问『电脑共享了什么文件夹/开了哪些共享』时调用。"
    )]
    async fn net_share(&self, _p: Parameters<NetShareParams>) -> Result<CallToolResult, McpError> {
        Ok(blocking_call(30, || net::collect_net_share(), "共享文件夹读取").await)
    }

    #[tool(
        description = "读取无线网络信息：本机无线网卡状态 + 已保存的 WiFi 网络名称列表（绝不读取密码，只读）。用户问『WiFi 连的是什么/保存了哪些 WiFi』时调用；无无线网卡会明确说明。"
    )]
    async fn net_wifi(&self, _p: Parameters<NetWifiParams>) -> Result<CallToolResult, McpError> {
        Ok(blocking_call(30, || net::collect_net_wifi(), "WiFi 信息读取").await)
    }

    #[tool(
        description = "读取每张网卡的详细配置：状态/速率/逐 IP 地址（IPv4 带子网掩码、IPv6 带前缀长度）/MAC 地址/网关/DNS 服务器（只读，不需要管理员权限）。用户问『我的 IP 是什么/子网掩码是多少/MAC 地址/网关 DNS』时调用。"
    )]
    async fn net_adapter_detail(
        &self,
        _p: Parameters<NetAdapterDetailParams>,
    ) -> Result<CallToolResult, McpError> {
        Ok(blocking_call(30, || net::collect_adapter_detail(), "网卡配置读取").await)
    }

    // ── 系统服务/驱动/启动（sys.rs）───────────────────────────────

    #[tool(
        description = "列出 Windows 服务：服务名/显示名/状态/启动类型/进程 PID（只读，不需要管理员权限）。可按关键字过滤、可含已禁用服务——服务启动类型与 ImagePath 直接读注册表，准确度高。用户问『什么服务在跑/某服务是干嘛的/怎么启动方式』时调用。"
    )]
    async fn sys_services(
        &self,
        Parameters(p): Parameters<SysServicesParams>,
    ) -> Result<CallToolResult, McpError> {
        let keyword = p.keyword.clone();
        let show_disabled = p.show_disabled.unwrap_or(false);
        let top_n = p.top_n.unwrap_or(50);
        Ok(blocking_call(
            45,
            move || sys::collect_services(keyword, show_disabled, top_n),
            "服务列表读取",
        )
        .await)
    }

    #[tool(
        description = "列出驱动（设备驱动）：名称/类型/厂商/驱动日期与版本/设备状态（只读，不需要管理员权限）。可按关键字过滤。用户问『装了什么驱动/某驱动是干嘛的/驱动是不是太旧』时调用。"
    )]
    async fn sys_drivers(
        &self,
        Parameters(p): Parameters<SysDriversParams>,
    ) -> Result<CallToolResult, McpError> {
        let keyword = p.keyword.clone();
        let top_n = p.top_n.unwrap_or(50);
        Ok(blocking_call(
            45,
            move || sys::collect_drivers(keyword, top_n),
            "驱动列表读取",
        )
        .await)
    }

    #[tool(
        description = "列出开机启动项：注册表 Run 键（含策略强制项与 32 位视图）+ 启动文件夹（只读，不删除不禁用），可附带计划任务。僵尸启动项（文件已不存在）会标红提示。用户问『开机自动启动了什么/怎么精简开机项』时调用。"
    )]
    async fn sys_boot_items(
        &self,
        Parameters(p): Parameters<SysBootItemsParams>,
    ) -> Result<CallToolResult, McpError> {
        let include_scheduled = p.include_scheduled.unwrap_or(false);
        Ok(blocking_call(
            45,
            move || sys::collect_boot_items(include_scheduled),
            "启动项读取",
        )
        .await)
    }

    #[tool(
        description = "控制 Windows 服务：启动 / 停止 / 重启（**写操作，可逆**）。系统关键服务（内核/系统核心）会被安全守卫拒绝。**此工具会改变系统状态，主项目侧会先弹确认门，用户确认后才真正执行**。用户明确要求『帮我停掉某服务/启动某服务/重启某服务』时调用；参数 action 取 start / stop / restart。"
    )]
    async fn service_control(
        &self,
        Parameters(p): Parameters<ServiceControlParams>,
    ) -> Result<CallToolResult, McpError> {
        if let Err(e) = files::require_confirmed() {
            return Ok(tool_error(e));
        }
        let name = p.name.clone();
        let action = p.action.clone();
        Ok(blocking_call(60, move || sys::control_service(&name, &action), "服务控制").await)
    }

    // ── 盘点/建议（steam.rs + cleanup.rs）──────────────────────────────

    #[tool(
        description = "盘点本机 Steam 游戏库：库根/每库游戏数/总占用 + 各游戏（appid/名称/安装目录/大小/上次游玩/是否幽灵安装/是否建议清理及原因）。只读（注册表定位 Steam + 读 ACF 清单）。用户问『装了哪些 Steam 游戏/哪个游戏占地/Steam 该清谁』时调用。"
    )]
    async fn steam_games(
        &self,
        Parameters(p): Parameters<SteamGamesParams>,
    ) -> Result<CallToolResult, McpError> {
        let top_n = p.top_n.unwrap_or(20);
        Ok(blocking_call(45, move || steam::collect_steam_games(top_n), "Steam 盘点").await)
    }

    #[tool(
        description = "按软件分类给出某目录（**必须传盘根，如 C:\\**）的可清理建议：统计各已知软件（scaffold）的缓存/临时目录占用（只读，单遍并行遍历 + glob 匹配）。**只出建议，绝不删除任何文件**（执行需用户确认走主项目清理流程）。用户问『C 盘/某盘有什么可清的/微信 QQ 缓存占了多少』时调用；未命中已知软件会明确说明。"
    )]
    async fn cleanup_suggestions(
        &self,
        Parameters(p): Parameters<CleanupSuggestionsParams>,
    ) -> Result<CallToolResult, McpError> {
        let root = p.root.clone();
        let top_n = p.top_n.unwrap_or(20);
        Ok(blocking_call(
            120,
            move || cleanup::collect_cleanup_suggestions(root, top_n),
            "清理建议计算",
        )
        .await)
    }

    #[tool(
        description = "统计某目录的**直接子目录**占用 Top N（含各自全部子级大小，只读，单遍并行遍历）。用户问『哪个文件夹占了空间/什么目录最大』时比单文件视角更实用。安全边界：盘根/系统目录/主目录根会被拒绝。"
    )]
    async fn disk_top_directories(
        &self,
        Parameters(p): Parameters<TopDirectoriesParams>,
    ) -> Result<CallToolResult, McpError> {
        let guard = files::PathGuard;
        let path = std::path::PathBuf::from(&p.path);
        if let Err(e) = guard.check(&path) {
            return Ok(tool_error(format!("路径被安全守卫拒绝：{e}")));
        }
        Ok(blocking_call(
            45,
            move || diskx::collect_top_directories(p.path.clone(), p.top_n.unwrap_or(20)),
            "子目录占用统计",
        )
        .await)
    }

    #[tool(
        description = "读取进程 CPU 占用 Top 30（同步采样窗口 200ms：两次 GetProcessTimes 差值，只读）。占用率为单核百分比（多核满载 = 核数 × 100%）。用户问『哪个进程吃 CPU/为什么卡/谁在烧 CPU』时调用，与 list_processes（内存视角）互补。"
    )]
    async fn process_cpu_usage(
        &self,
        _p: Parameters<ProcessCpuUsageParams>,
    ) -> Result<CallToolResult, McpError> {
        Ok(blocking_call(45, || process::collect_cpu_usage(), "CPU 占用采样").await)
    }

    #[tool(
        description = "列出 USB 设备：控制器/集线器/外设 + 厂商/服务/状态（只读，PNP 枚举，普通权限，不读设备内容）。用户问『插了什么 USB 设备/U 盘/键鼠/摄像头识别到没有』时调用。"
    )]
    async fn usb_devices(
        &self,
        _p: Parameters<UsbDevicesParams>,
    ) -> Result<CallToolResult, McpError> {
        Ok(blocking_call(45, || hw::collect_usb_devices(), "USB 设备枚举").await)
    }

    #[tool(
        description = "读取磁盘可靠性计数器明细：每块物理盘的通电时长/温度/磨损/累计读写错误/启停循环（只读，普通权限）。注意：这是 Get-StorageReliabilityCounter 可靠性计数，**不是**标准 SMART 属性表（ID 0x05 重分配扇区等需 smartctl，本工具不装）。用户问『硬盘具体磨损多少/通电多久的明细』时调用。"
    )]
    async fn disk_smart_raw_attributes(
        &self,
        _p: Parameters<DiskSmartRawParams>,
    ) -> Result<CallToolResult, McpError> {
        Ok(blocking_call(45, || hw::collect_disk_smart_raw(), "磁盘可靠性计数读取").await)
    }

    // ── 安全/审计（sec.rs）─────────────────────────────────────────

    #[tool(
        description = "读取系统事件日志：指定日志名（逗号分隔，默认 System）、最近 hours 小时、最多 max_events 条（只读）。安全日志无管理员权限会降级说明。用户问『最近系统出了什么错误/蓝屏/警告/日志』时调用。"
    )]
    async fn security_event_logs(
        &self,
        Parameters(p): Parameters<EventLogsParams>,
    ) -> Result<CallToolResult, McpError> {
        let logs = p.logs.clone();
        let hours = p.hours.unwrap_or(24);
        let max_events = p.max_events.unwrap_or(20);
        Ok(blocking_call(
            45,
            move || sec::collect_event_logs(logs, hours, max_events),
            "事件日志读取",
        )
        .await)
    }

    #[tool(
        description = "列出防火墙规则：方向（默认 Inbound 入站）与动作（默认 Allow 允许）过滤，最多 top_n 条（只读，不需要管理员权限）。用户问『防火墙开了什么/某程序被放行了吗』时调用。"
    )]
    async fn security_firewall_rules(
        &self,
        Parameters(p): Parameters<FirewallRulesParams>,
    ) -> Result<CallToolResult, McpError> {
        let direction = p.direction.clone();
        let action = p.action.clone();
        let top_n = p.top_n.unwrap_or(20);
        Ok(blocking_call(
            45,
            move || sec::collect_firewall_rules(direction, action, top_n),
            "防火墙规则读取",
        )
        .await)
    }

    #[tool(
        description = "读取最近的登录/注销事件（安全日志 4624/4625 + PowerShell 脚本块日志 4104，只读）。需要管理员权限才读得到安全日志，无权限会明确降级。用户问『谁登录过这台电脑/有没有异常登录/有没有人跑过脚本』时调用。"
    )]
    async fn security_login_events(
        &self,
        Parameters(p): Parameters<LoginEventsParams>,
    ) -> Result<CallToolResult, McpError> {
        let max_events = p.max_events.unwrap_or(20);
        Ok(blocking_call(
            45,
            move || sec::collect_login_events(max_events),
            "登录事件读取",
        )
        .await)
    }

    #[tool(
        description = "读取 Windows Defender 安全中心状态（只读）：服务状态 + 实时保护开关 + 病毒库/引擎版本 + 最近快速扫描时间。用户问『杀毒软件开没开/Windows 安全中心状态/病毒库新不新』时调用；若系统用第三方杀软（Defender 被替换）会如实说明。"
    )]
    async fn sys_defender_status(
        &self,
        _p: Parameters<DefenderStatusParams>,
    ) -> Result<CallToolResult, McpError> {
        Ok(blocking_call(45, || sec::collect_defender_status(), "Defender 状态读取").await)
    }

    #[tool(
        description = "枚举本机用户账户（只读）：账户名/全名/是否禁用/是否锁定/本地或域账户/SID 尾段，排除内置系统账户（defaultuser0 等）。用户问『这台电脑有哪些用户/有几个账户/禁用账户是谁』时调用；无需管理员即可读取，受限时如实降级。隐私红线：绝不输出任何密码/token 信息。"
    )]
    async fn sys_user_accounts(
        &self,
        _p: Parameters<UserAccountsParams>,
    ) -> Result<CallToolResult, McpError> {
        Ok(blocking_call(45, || sec::collect_user_accounts(), "用户账户枚举").await)
    }

    #[tool(
        description = "查询本机回收站状态（只读，Windows API）：逐卷文件数与总大小 + 合计。用户问『回收站占了多少空间/回收站里有多少东西/能不能清回收站腾空间』时调用。只读状态，不执行清空。"
    )]
    async fn recycle_bin_stats(
        &self,
        _p: Parameters<RecycleBinStatsParams>,
    ) -> Result<CallToolResult, McpError> {
        Ok(blocking_call(30, || envx::collect_recycle_bin_stats(), "回收站状态查询").await)
    }

    #[tool(
        description = "读取环境变量值（只读）：传 name 查指定变量；不传列出常用子集（PATH/TEMP/USERPROFILE/APPDATA 等）。用户问『某个路径在哪/环境变量配没配/命令找不到是不是 PATH 问题』时调用。"
    )]
    async fn env_vars(
        &self,
        Parameters(p): Parameters<EnvVarsParams>,
    ) -> Result<CallToolResult, McpError> {
        let name = p.name.clone();
        Ok(blocking_call(30, move || envx::collect_env_vars(name), "环境变量读取").await)
    }

    // ── 磁盘深入（diskx.rs）───────────────────────────────────────

    #[tool(
        description = "列出某目录下最大的文件（递归，只读）。安全边界：盘根/系统目录/主目录根会被拒绝。用户问『什么文件占了空间/哪个文件最大』时调用。"
    )]
    async fn disk_find_biggest_files(
        &self,
        Parameters(p): Parameters<BiggestFilesParams>,
    ) -> Result<CallToolResult, McpError> {
        let guard = files::PathGuard;
        let path = std::path::PathBuf::from(&p.path);
        if let Err(e) = guard.check(&path) {
            return Ok(tool_error(format!("路径被安全守卫拒绝：{e}")));
        }
        Ok(blocking_call(
            45,
            move || diskx::collect_biggest_files(p.path.clone(), p.top_n.unwrap_or(20)),
            "大文件扫描",
        )
        .await)
    }

    #[tool(
        description = "找出某目录下疑似重复的文件（递归，按大小分组并按头部 64KB 哈希比对；只读。只比对头部 64KB，不是全文件哈希）。安全边界：盘根/系统目录/主目录根会被拒绝。用户问『哪些文件重复了/有什么可删的重复文件』时调用。"
    )]
    async fn disk_find_duplicate_files(
        &self,
        Parameters(p): Parameters<DuplicateFilesParams>,
    ) -> Result<CallToolResult, McpError> {
        let guard = files::PathGuard;
        let path = std::path::PathBuf::from(&p.path);
        if let Err(e) = guard.check(&path) {
            return Ok(tool_error(format!("路径被安全守卫拒绝：{e}")));
        }
        Ok(blocking_call(
            45,
            move || diskx::collect_duplicate_files(p.path.clone(), p.top_n.unwrap_or(20)),
            "重复文件扫描",
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

    #[tool(
        description = "诊断风扇写控通道（只读 L0）：调用 fancmd diag json，返回驱动是否加载/驱动名/RTC 端口验证/SuperIO 芯片与 env_base/风扇列表/通道可写性，并给出明确的可修复点清单（缺驱动？芯片不可识别？可写但需 reset？）。用户在其他机器上发现『风扇控制不可用』时，先调这个拿诊断，再决定是否让用户授权 fan_selfheal_fix 修复。"
    )]
    async fn fan_selfheal_diag(
        &self,
        _p: Parameters<FanSelfhealDiagParams>,
    ) -> Result<CallToolResult, McpError> {
        Ok(blocking_call(45, || selfheal::selfheal_diag(), "风扇通道诊断").await)
    }

    #[tool(
        description = "修复风扇写控通道（**写操作 L2**，由主项目权限中心确认门 + 管理员权限双重守护）。action 可选：install_driver=安装并启动 inpoutx64 端口驱动（需管理员，复制 DLL 到 System32 + sc create/start，绝不覆盖已存在文件）；reset_fan <idx>=恢复指定风扇为主板自动控制（安全可逆，不需管理员）。dry_run=true 只出计划不执行。AI 应先跑 fan_selfheal_diag 确认根因，再用本工具修复，并明确告知用户要做的操作、请用户确认。"
    )]
    async fn fan_selfheal_fix(
        &self,
        Parameters(p): Parameters<FanSelfhealFixParams>,
    ) -> Result<CallToolResult, McpError> {
        if let Err(e) = files::require_confirmed() {
            return Ok(tool_error(e));
        }
        let action = p.action.clone();
        let dry_run = p.dry_run.unwrap_or(true);
        Ok(blocking_call(
            60,
            move || selfheal::selfheal_fix(&action, dry_run),
            "风扇自修复",
        )
        .await)
    }

    #[tool(
        description = "直接调速风扇（**写操作 L2**，由主项目权限中心确认门守护）：把指定风扇设为软件控制的固定转速百分比。参数 idx=风扇索引（**字符串**，先用 fan_selfheal_diag 查 fans 列表确认，如 \"0\"）、pct=目标转速 0-100（数字，越界自动钳制）。执行前自动 diag 校验写通道（驱动未加载/芯片不可识别时拒绝）。dry_run=true 只出计划不执行。软件接管后可随时用 fan_selfheal_fix action=reset_fan <idx> 恢复主板自动控制。AI 应先 fan_selfheal_diag 看可写性与风扇列表，dry-run 给用户确认后再传 dry_run=false 真正调速。"
    )]
    async fn fan_control(
        &self,
        Parameters(p): Parameters<FanControlParams>,
    ) -> Result<CallToolResult, McpError> {
        if let Err(e) = files::require_confirmed() {
            return Ok(tool_error(e));
        }
        let idx = p.idx.clone();
        let pct = p.pct;
        let dry_run = p.dry_run.unwrap_or(true);
        Ok(blocking_call(
            30,
            move || selfheal::fan_control(&idx, pct, dry_run),
            "风扇调速",
        )
        .await)
    }

    #[tool(
        description = "列出图吧工具箱集成工具清单（只读 L0）：工具名/分类/CLI|GUI/权限级/风险/用途。AI 判断『这个场景该用工具箱哪个工具』时调用，或先于 toolbelt_run 查询可用工具。CLI 工具（A 类）可被 AI 直接执行，GUI 工具（B 类）只能启动提示用户。"
    )]
    async fn toolbelt_list(
        &self,
        _p: Parameters<ToolbeltListParams>,
    ) -> Result<CallToolResult, McpError> {
        Ok(blocking_call(30, || Ok(toolbelt::list_tools()), "工具箱清单读取").await)
    }

    #[tool(
        description = "执行图吧工具箱的 CLI 工具（**写操作 L2**，三层防线：L3 永久禁止如 FPT64 刷 BIOS；medium/high 风险必须 confirmed=true 用户确认；cli.run 权限未开启拒绝）。参数 tool=工具名（先 toolbelt_list 确认名称，如 crystaldiskinfo/wiztree）、args=参数列表（透传不拼 shell，空则用工具默认）、confirmed=用户已确认（medium+ 风险必须 true）、timeout_secs=超时秒（默认 30）。**此工具会启动外部程序，主项目侧会先弹确认门**。AI 在用户明确要求『用 XX 工具跑一下』时调用；只读工具（风险 low 如 CrystalDiskInfo/WizTree）可不确认，写类（风险 medium/high）必须先展示命令给用户确认再传 confirmed=true。"
    )]
    async fn toolbelt_run(
        &self,
        Parameters(p): Parameters<ToolbeltRunParams>,
    ) -> Result<CallToolResult, McpError> {
        if let Err(e) = files::require_confirmed() {
            return Ok(tool_error(e));
        }
        let tool = p.tool.clone();
        let args = p.args.clone().unwrap_or_default();
        let confirmed = p.confirmed.unwrap_or(false);
        let timeout = p.timeout_secs;
        match tokio::task::spawn_blocking(move || {
            toolbelt::run_tool(&tool, &args, confirmed, timeout)
        })
        .await
        {
            Ok(Ok(s)) => Ok(text_result(s)),
            Ok(Err(e)) => Ok(tool_error(e)),
            Err(e) => Ok(tool_error(format!("工具箱执行任务崩溃: {e}"))),
        }
    }

    // ── 阶段二：AIDA64 核心复刻 —— 基准 + 压测（bench.rs） ──────────────

    #[tool(
        description = "CPU 基准（只读 L0，复刻 AIDA64 CPU Queen/FPU Julia）：多线程整数（质数筛）+ 浮点（π 级数）吞吐测试。参数 secs=每项时长秒（默认 3，上限 30）、threads=线程数（默认全部可用）。纯计算不写盘，几秒出结果。AI 在用户问『CPU 性能/跑分』时调用。"
    )]
    async fn bench_cpu(
        &self,
        Parameters(p): Parameters<BenchCpuParams>,
    ) -> Result<CallToolResult, McpError> {
        let secs = p.secs;
        let threads = p.threads;
        match tokio::task::spawn_blocking(move || bench::bench_cpu(secs, threads)).await {
            Ok(Ok(s)) => Ok(text_result(s)),
            Ok(Err(e)) => Ok(tool_error(e)),
            Err(e) => Ok(tool_error(format!("CPU 基准任务崩溃: {e}"))),
        }
    }

    #[tool(
        description = "内存基准（只读 L0，复刻 AIDA64 Cache & Memory Benchmark）：读/写/复制带宽（MB/s）+ 延迟（指针追逐 ns）。参数 secs=每项时长秒（默认 3，上限 20）。纯只读（写基准写自身堆内存），几秒出结果。AI 在用户问『内存性能/带宽/延迟』时调用。"
    )]
    async fn bench_memory(
        &self,
        Parameters(p): Parameters<BenchMemoryParams>,
    ) -> Result<CallToolResult, McpError> {
        let secs = p.secs;
        match tokio::task::spawn_blocking(move || bench::bench_memory(secs)).await {
            Ok(Ok(s)) => Ok(text_result(s)),
            Ok(Err(e)) => Ok(tool_error(e)),
            Err(e) => Ok(tool_error(format!("内存基准任务崩溃: {e}"))),
        }
    }

    #[tool(
        description = "磁盘基准（只读 L0 为主，复刻 AIDA64 Disk Benchmark）：顺序/随机读 + 顺序/随机写（MB/s）。参数 path=测试目录（默认系统临时目录，**必须用临时目录或用户指定目录，绝不碰用户数据**）、size_mb=测试文件大小（默认 256，16-2048）、dry_run=只出计划不创建文件（默认 true）。dry_run=true 先给 AI 看计划；用户同意后再传 dry_run=false 真正跑。测完临时文件立即删除。AI 在用户问『磁盘速度/读写性能』时调用。"
    )]
    async fn bench_disk(
        &self,
        Parameters(p): Parameters<BenchDiskParams>,
    ) -> Result<CallToolResult, McpError> {
        let dry_run = p.dry_run.unwrap_or(true);
        // 真跑（dry_run=false）会在目标目录创建临时文件写入——写操作，必须过确认门
        if !dry_run {
            if let Err(e) = files::require_confirmed() {
                return Ok(tool_error(e));
            }
        }
        let path = p.path.clone();
        let size_mb = p.size_mb;
        match tokio::task::spawn_blocking(move || bench::bench_disk(path, size_mb, Some(dry_run)))
            .await
        {
            Ok(Ok(s)) => Ok(text_result(s)),
            Ok(Err(e)) => Ok(tool_error(e)),
            Err(e) => Ok(tool_error(format!("磁盘基准任务崩溃: {e}"))),
        }
    }

    #[tool(
        description = "稳定性压测（**写操作 L2**，复刻 AIDA64 System Stability Test）：CPU 多线程满载压测，期间每 1s 采样 CPU 温度，超过 max_temp 阈值自动熔断停机保护硬件。参数 secs=时长秒（默认 5 演示，上限 600）、max_temp=熔断阈值℃（默认 95，60-110）、threads=线程数（默认全部可用）。纯计算不写盘。**满负荷压测会大幅升温/占 CPU，AI 必须先告知用户风险并等用户确认（主项目确认门），用户同意后才调用**。压测结束时给出峰值温度与是否熔断。"
    )]
    async fn stress_test(
        &self,
        Parameters(p): Parameters<StressTestParams>,
    ) -> Result<CallToolResult, McpError> {
        if let Err(e) = files::require_confirmed() {
            return Ok(tool_error(e));
        }
        let secs = p.secs;
        let max_temp = p.max_temp;
        let threads = p.threads;
        match tokio::task::spawn_blocking(move || bench::stress_test(secs, max_temp, threads)).await
        {
            Ok(Ok(s)) => Ok(text_result(s)),
            Ok(Err(e)) => Ok(tool_error(e)),
            Err(e) => Ok(tool_error(format!("压测任务崩溃: {e}"))),
        }
    }

    #[tool(
        description = "GPU 稳定性压测（**写操作 L1**，复刻 FurMark 的温度熔断守门版）：纯后端无 OpenCL/CUDA/WebGL 引擎无法让 GPU 满载（深度压载需厂商 SDK，同 GPGPU 基准结论），本工具承担温度熔断守门——你正在用前端甜甜圈压测页 / 外挂 FurMark / 游戏 / 渲染任务做 GPU 负载时，本工具在 secs 秒内持续采样 GPU 温度（N 卡 nvidia-smi），超 max_temp 即报熔断信号保护硬件。参数 secs=监控秒数（默认 5，上限 120）、max_temp=熔断阈值℃（默认 90，60-110）。无 N 卡/无 nvidia-smi 时如实降级「无温度护栏」。hw.stress 权限门，需用户确认。"
    )]
    async fn stress_test_gpu(
        &self,
        Parameters(p): Parameters<StressTestGpuParams>,
    ) -> Result<CallToolResult, McpError> {
        if let Err(e) = files::require_confirmed() {
            return Ok(tool_error(e));
        }
        let secs = p.secs;
        let max_temp = p.max_temp;
        match tokio::task::spawn_blocking(move || bench::stress_test_gpu(secs, max_temp)).await {
            Ok(Ok(s)) => Ok(text_result(s)),
            Ok(Err(e)) => Ok(tool_error(e)),
            Err(e) => Ok(tool_error(format!("GPU 压测任务崩溃: {e}"))),
        }
    }

    #[tool(
        description = "停止当前压测（只读 L0 控制信号，无参数）：向正在运行的 stress_test / stress_test_gpu 发送停止信号（进程内 CANCEL flag + 跨进程停止文件，两种通道任一命中即退出），压测循环检测到后立即退出并报告「已按停止信号结束」。前端「停止」按钮 / 用户在 AI 对话里说『停』时调用。无压测运行时调用是安全空操作（返回『当前无压测在跑』）。"
    )]
    async fn stress_cancel(&self) -> Result<CallToolResult, McpError> {
        let had = bench::stress_cancel();
        Ok(text_result(if had {
            "已发送停止信号：正在运行的压测将在下一次采样循环检测后退出。".to_string()
        } else {
            "当前无压测在运行（停止信号为空操作）。".to_string()
        }))
    }

    #[tool(
        description = "内存稳定性测试（只读 L0，MemTest86 简化版）：进程内分配 size_mb 缓冲，用 全零/全一/AA55/递增/伪随机 5 种 pattern 反复写入并读回校验，发现位翻转/不稳定内存。参数 size_mb=缓冲大小（默认 256，64-4096）、rounds=轮数（默认 1，1-10）。纯进程堆读写，无磁盘副作用。用于『内存条稳不稳/超频是否稳定』场景；注意只能覆盖进程寻址内存，无法替代 pre-boot 版 MemTest86 的硬件寻址层。"
    )]
    async fn mem_test(
        &self,
        Parameters(p): Parameters<MemTestParams>,
    ) -> Result<CallToolResult, McpError> {
        let size_mb = p.size_mb;
        let rounds = p.rounds;
        match tokio::task::spawn_blocking(move || bench::mem_test(size_mb, rounds)).await {
            Ok(Ok(s)) => Ok(text_result(s)),
            Ok(Err(e)) => Ok(tool_error(e)),
            Err(e) => Ok(tool_error(format!("内存测试任务崩溃: {e}"))),
        }
    }

    #[tool(
        description = "蓝屏转储分析（只读 L0，复刻 BlueScreenView）：扫描 C:\\Windows\\Minidump\\*.dmp 与根目录 MEMORY.DMP，解析每个转储的崩溃时间 / bugcheck 代码 / 4 参数，映射为中文含义（0x124 WHEA=CPU/内存硬件、0x116 VIDEO_TDR=显卡驱动、0x1A MEMORY_MANAGEMENT=内存等 24 个常见代码）。参数 max_dumps=最多解析几个（默认 10，上限 30）、include_memory_dmp=是否读根目录 MEMORY.DMP（默认 true）。无转储时返回『未记录到蓝屏』正面结论。用于『为什么蓝屏了/蓝屏代码是什么意思』场景。纯只读不改删文件。"
    )]
    async fn bsod_analyze(
        &self,
        Parameters(p): Parameters<BsodAnalyzeParams>,
    ) -> Result<CallToolResult, McpError> {
        let max_dumps = p.max_dumps;
        let include_memory_dmp = p.include_memory_dmp;
        Ok(blocking_call(
            60,
            move || bsod::analyze_bsod(max_dumps, include_memory_dmp),
            "蓝屏转储分析",
        )
        .await)
    }

    // ── 阶段三：体检报告 / 传感器趋势 / 阈值告警（report.rs） ────────────

    #[tool(
        description = "一键系统体检报告（只读 L0，复刻 AIDA64 Report）：汇总 CPU/GPU/内存/主板/温度/磁盘健康/系统/电池全部只读采集，输出 Markdown 报告。AI 在用户问『体检/电脑状态总结/报告』时调用，拿它当结论依据。"
    )]
    async fn system_report(
        &self,
        _p: Parameters<SystemReportParams>,
    ) -> Result<CallToolResult, McpError> {
        Ok(blocking_call(120, report::system_report, "体检报告生成").await)
    }

    #[tool(
        description = "传感器趋势（只读 L0，写应用自有历史文件）：追加一条当前传感器快照（含 CPU 最高温）到历史 JSONL（%LOCALAPPDATA%/diskpilot/agent-server/sensor-history.jsonl，追加式 + 自动裁剪 500 条——只写应用自己的数据目录，不改不动用户文件），返回最近 n 条（默认 10，上限 50）。format=json 时返回结构化数组供前端折线图。AI 在用户问『温度趋势/散热表现』时调用（多次调用可见随时间变化）。"
    )]
    async fn sensor_trend(
        &self,
        Parameters(p): Parameters<SensorTrendParams>,
    ) -> Result<CallToolResult, McpError> {
        let n = p.n;
        let format = p.format;
        Ok(blocking_call(45, move || report::sensor_trend(n, format), "传感器趋势").await)
    }

    #[tool(
        description = "传感器阈值告警（只读 L0，persist=true 时写应用自有告警日志）：体检 CPU 温度/磁盘温度/GPU 温度/内存占用/磁盘剩余是否超阈值，输出告警清单（只读采集 + 阈值比较，无系统改动）。参数 cpu_temp=CPU 温度阈值℃（默认 85）、disk_temp=磁盘温度阈值℃（默认 55）、gpu_temp=GPU 阈值℃（默认 85）、mem_percent=内存占用阈值%（默认 90）、disk_free_gb=磁盘剩余阈值 GB（默认 20）、persist=是否把告警写历史到 %LOCALAPPDATA%/diskpilot/agent-server/sensor-alerts.jsonl（默认 false，只写应用自有数据目录）。AI 在用户问『电脑状态健康吗/该注意什么』时调用。"
    )]
    async fn sensor_alert(
        &self,
        Parameters(p): Parameters<SensorAlertParams>,
    ) -> Result<CallToolResult, McpError> {
        let cpu_temp = p.cpu_temp;
        let disk_temp = p.disk_temp;
        let gpu_temp = p.gpu_temp;
        let mem_percent = p.mem_percent;
        let disk_free_gb = p.disk_free_gb;
        let persist = p.persist;
        Ok(blocking_call(
            45,
            move || {
                report::sensor_alert(
                    cpu_temp,
                    disk_temp,
                    gpu_temp,
                    mem_percent,
                    disk_free_gb,
                    persist,
                )
            },
            "传感器告警",
        )
        .await)
    }

    // ── 阶段四：信息补全（hw.rs 扩展） ───────────────────────────────────

    #[tool(
        description = "CPU 指令集与特性（只读 L0，复刻 AIDA64 CPUID）：硬件虚拟化/VT-x/AMD-V、SLAT（EPT/NPT）、VMX/SVM，以及运行时探测确认的指令集（SSE2/SSE4.2/AVX/AVX2/FMA/AES-NI）。AI 在用户问『CPU 支持哪些指令集/虚拟化』时调用。"
    )]
    async fn hw_cpu_features(
        &self,
        _p: Parameters<HwCpuFeaturesParams>,
    ) -> Result<CallToolResult, McpError> {
        Ok(blocking_call(45, hw::collect_cpu_features, "CPU 指令集采集").await)
    }

    #[tool(
        description = "DRAM 时序（只读 L0，复刻 AIDA64 SPD/DRAM Timings）：每根内存条的容量/标称与运行频率/品牌型号 + 频率一致性诊断（XMP/EXPO 是否生效）。完整 CL 时序需 SPD 读取工具（CPU-Z/Thaiphoon），本工具不编造。AI 在用户问『内存频率/时序/XMP 生效没』时调用。"
    )]
    async fn hw_dram_timings(
        &self,
        _p: Parameters<HwDramTimingsParams>,
    ) -> Result<CallToolResult, McpError> {
        Ok(blocking_call(45, hw::collect_dram_timings, "DRAM 时序采集").await)
    }

    #[tool(
        description = "显示器信息（只读 L0，复刻 AIDA64 Monitor）：每台显示器的物理尺寸（cm/英寸）/输入类型/激活状态。分辨率与刷新率见 collect_gpu。AI 在用户问『显示器型号/尺寸/支持什么输入』时调用。"
    )]
    async fn hw_displays(
        &self,
        _p: Parameters<HwDisplaysParams>,
    ) -> Result<CallToolResult, McpError> {
        Ok(blocking_call(45, hw::collect_displays, "显示器采集").await)
    }

    // ── 阶段五：长尾 G12/G14/G17（bench_gpu 简化版 / IPMI 探测 / ACPI 信息） ──

    #[tool(
        description = "GPU 快照（只读 L0，复刻 AIDA64 GPGPU Benchmark 的只读部分）：显卡型号/显存/驱动/分辨率/温度（N 卡）。深度 GPGPU 基准（像素填充率/OpenCL）需 GPU 厂商 SDK，本工具集未实现，如实标注。AI 在用户问『显卡参数/驱动/温度』或要求 GPU 基准时先调用它。"
    )]
    async fn bench_gpu(&self, _p: Parameters<BenchGpuParams>) -> Result<CallToolResult, McpError> {
        Ok(blocking_call(45, hw::collect_gpu_snapshot, "GPU 快照采集").await)
    }

    #[tool(
        description = "IPMI 服务器接口探测（只读 L0，复刻 AIDA64 IPMI）：检测主板是否带 BMC/IPMI 接口（服务器/HEDT 板才有，家用台式机通常无——如实返回未检测到，不编造传感器）。检测到时列出接口实例。AI 在用户问『这板子支不支持 IPMI/远程管理』时调用。"
    )]
    async fn hw_ipmi(&self, _p: Parameters<HwIpmiParams>) -> Result<CallToolResult, McpError> {
        Ok(blocking_call(45, hw::collect_ipmi, "IPMI 探测").await)
    }

    #[tool(
        description = "ACPI 固件信息（只读 L0，复刻 AIDA64 ACPI Browser 简化版）：BIOS 厂商/版本/发布日期/序列号 + 机箱厂商/型号/系统类型。完整 ACPI 表浏览（FADT/DSDT 反汇编）需 Ring0 工具，属专业向，这里给到 WMI 可读的固件/电源顶层信息。AI 在用户问『BIOS 版本/主板固件/ACPI』时调用。"
    )]
    async fn hw_acpi(&self, _p: Parameters<HwAcpiParams>) -> Result<CallToolResult, McpError> {
        Ok(blocking_call(45, hw::collect_acpi, "ACPI 采集").await)
    }
}

impl Default for AgentServer {
    fn default() -> Self {
        Self::new()
    }
}

// ── 参数 struct（放在 impl 外，宏需要见 `struct` 定义） ─────────────────

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct DiskHealthParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct PartitionUsageParams {
    #[schemars(description = "要查的路径（返回该路径所在卷的用量），如 C:\\Users")]
    path: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct VolumeMetaParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct FileTypeStatsParams {
    #[schemars(description = "要统计的目录绝对路径，如 C:\\Users\\me\\Downloads")]
    path: String,
    #[schemars(description = "返回前 N 大占用扩展名，默认 20")]
    #[serde(default, deserialize_with = "lenient::opt_usize")]
    top_n: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct FileTreeParams {
    #[schemars(description = "目录绝对路径，如 C:\\Users\\me\\Downloads")]
    path: String,
    #[schemars(description = "目录树最大深度（默认 3，上限 10）")]
    #[serde(default, deserialize_with = "lenient::opt_usize")]
    max_depth: Option<usize>,
    #[schemars(description = "最多返回多少节点（默认 100，上限 500）")]
    #[serde(default, deserialize_with = "lenient::opt_usize")]
    max_nodes: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SystemInfoParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ListDirParams {
    #[schemars(description = "目录绝对路径，如 C:\\Users\\me\\Downloads")]
    path: String,
    #[schemars(description = "最多返回多少项，默认 100")]
    #[serde(default, deserialize_with = "lenient::opt_usize")]
    limit: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ReadFileParams {
    #[schemars(description = "文件绝对路径，如 C:\\Users\\me\\Documents\\notes.txt")]
    path: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct FindFilesParams {
    #[schemars(description = "搜索根目录绝对路径，如 C:\\Users\\me")]
    root: String,
    #[schemars(description = "文件名关键字（大小写不敏感），如 report")]
    keyword: String,
    #[schemars(description = "最多返回多少条命中，默认 50")]
    #[serde(default, deserialize_with = "lenient::opt_usize")]
    max_hits: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ListProcessesParams {
    #[schemars(description = "最多返回多少个进程（按内存降序截断），默认 50")]
    #[serde(default, deserialize_with = "lenient::opt_usize")]
    top_n: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ProcessInfoParams {
    #[schemars(description = "进程 PID，如 1234")]
    pid: u32,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ProcessKillParams {
    #[schemars(description = "要结束的进程 PID，如 1234（系统关键进程会被守卫拒绝）")]
    pid: u32,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ProcessStartParams {
    #[schemars(
        description = "要启动的程序的绝对路径（必须 .exe，且在系统目录或已安装程序目录下，如 C:\\Windows\\System32\\notepad.exe）"
    )]
    command: String,
    #[schemars(description = "传给程序的参数列表（可选；原样传入，不拼 shell 字符串）")]
    args: Option<Vec<String>>,
    #[schemars(description = "进程工作目录（可选；必须是已存在的目录，不存在就用默认）")]
    cwd: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct FileRecycleParams {
    #[schemars(
        description = "要移入回收站的文件/目录绝对路径（盘根/系统目录/用户主目录根会被守卫拒绝）"
    )]
    path: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ScheduledTaskManageParams {
    #[schemars(
        description = "计划任务名（先调 sys_boot_items include_scheduled=true 确认名称；系统内置任务会被守卫拒绝修改）"
    )]
    name: String,
    #[schemars(
        description = "动作：enable 启用 / disable 禁用 / query 只读查询状态（不开放 delete/run）"
    )]
    action: String,
    #[schemars(description = "只出计划不执行（默认 false）")]
    #[serde(default, deserialize_with = "lenient::opt_bool")]
    dry_run: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AppListParams {
    #[schemars(description = "按名称/发布者关键字过滤（可选），如 chrome")]
    keyword: Option<String>,
    #[schemars(description = "最多返回多少个程序（默认 50，上限 200）")]
    #[serde(default, deserialize_with = "lenient::opt_usize")]
    limit: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct UninstallAppParams {
    #[schemars(
        description = "要卸载的已安装程序名（DisplayName，先调 app_list 确认，大小写不敏感）"
    )]
    name: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AppLicensesParams {}

// ── 硬件（hw.rs）参数 ─────────────────────────────────────────────

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct HwCpuParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct HwGpuParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct HwMemoryParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct HwMotherboardParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct HwTemperatureParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct HwSuperioParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct HwSensorsParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct HwBatteryParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct HwDiskSmartParams {}

// ── 网络（net.rs）参数 ────────────────────────────────────────────

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct NetStatusParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct NetAdapterDetailParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct NetConnectionsParams {
    #[schemars(description = "按进程 PID 过滤（可选），如 1234")]
    #[serde(default, deserialize_with = "lenient::opt_u32")]
    pid: Option<u32>,
    #[schemars(description = "按关键字过滤（可选）：匹配进程名或远端 IP，大小写不敏感")]
    keyword: Option<String>,
    #[schemars(description = "最多返回多少条（默认 30，上限 100）")]
    #[serde(default, deserialize_with = "lenient::opt_usize")]
    top_n: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct NetSpeedParams {
    #[schemars(description = "采样窗口毫秒（1000-10000，默认 2000）")]
    #[serde(default, deserialize_with = "lenient::opt_u64")]
    interval_ms: Option<u64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct NetShareParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct NetWifiParams {}

// ── 系统服务/驱动/启动（sys.rs）参数 ──────────────────────────────

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SysServicesParams {
    #[schemars(description = "按服务名/显示名关键字过滤（可选），如 sql")]
    keyword: Option<String>,
    #[schemars(description = "是否包含已禁用的服务（默认 false 只看启用/运行中）")]
    #[serde(default, deserialize_with = "lenient::opt_bool")]
    show_disabled: Option<bool>,
    #[schemars(description = "最多返回多少个服务（默认 50，上限 200）")]
    #[serde(default, deserialize_with = "lenient::opt_usize")]
    top_n: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ServiceControlParams {
    #[schemars(description = "服务名（先调 sys_services 确认名字），如 spooler")]
    name: String,
    #[schemars(description = "动作：start 启动 / stop 停止 / restart 重启")]
    action: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SysDriversParams {
    #[schemars(description = "按驱动名关键字过滤（可选），如 nvidia")]
    keyword: Option<String>,
    #[schemars(description = "最多返回多少个驱动（默认 50，上限 200）")]
    #[serde(default, deserialize_with = "lenient::opt_usize")]
    top_n: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SysBootItemsParams {
    #[schemars(description = "是否附带读取计划任务启动项（较慢，默认 false）")]
    #[serde(default, deserialize_with = "lenient::opt_bool")]
    include_scheduled: Option<bool>,
}

// ── 安全/审计（sec.rs）参数 ───────────────────────────────────────

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct EventLogsParams {
    #[schemars(description = "日志名（逗号分隔），如 System 或 System,Application（默认 System）")]
    logs: Option<String>,
    #[schemars(description = "回溯小时数（默认 24，上限 720）")]
    #[serde(default, deserialize_with = "lenient::opt_u32")]
    hours: Option<u32>,
    #[schemars(description = "每个日志最多返回多少条（默认 20，上限 50）")]
    #[serde(default, deserialize_with = "lenient::opt_usize")]
    max_events: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct FirewallRulesParams {
    #[schemars(description = "方向过滤：Inbound / Outbound / Any（默认 Inbound）")]
    direction: Option<String>,
    #[schemars(description = "动作过滤：Allow / Block / Any（默认 Allow）")]
    action: Option<String>,
    #[schemars(description = "最多返回多少条（默认 20，上限 100）")]
    #[serde(default, deserialize_with = "lenient::opt_usize")]
    top_n: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct LoginEventsParams {
    #[schemars(description = "最多返回多少条（默认 20，上限 50）")]
    #[serde(default, deserialize_with = "lenient::opt_usize")]
    max_events: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct DefenderStatusParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct UserAccountsParams {}

// ── 磁盘深入（diskx.rs）参数 ──────────────────────────────────────

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct BiggestFilesParams {
    #[schemars(description = "要搜索的目录绝对路径，如 C:\\Users\\me\\Downloads")]
    path: String,
    #[schemars(description = "最多返回多少个文件（默认 20，上限 100）")]
    #[serde(default, deserialize_with = "lenient::opt_usize")]
    top_n: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct DuplicateFilesParams {
    #[schemars(description = "要搜索的目录绝对路径，如 C:\\Users\\me\\Downloads")]
    path: String,
    #[schemars(description = "最多返回多少组重复（默认 20，上限 100）")]
    #[serde(default, deserialize_with = "lenient::opt_usize")]
    top_n: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct DiskIoUsageParams {
    #[schemars(description = "采样窗口毫秒（默认 1000）")]
    #[serde(default, deserialize_with = "lenient::opt_u64")]
    sample_ms: Option<u64>,
}

// ── 自修复（selfheal.rs）参数 ─────────────────────────────────────────

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct FanSelfhealDiagParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct FanSelfhealFixParams {
    #[schemars(
        description = "修复动作：install_driver（安装 inpoutx64 驱动，需管理员）/ reset_fan <idx>（恢复风扇 idx 主板控制）"
    )]
    action: String,
    #[schemars(
        description = "只出计划不执行（默认 true）。AI 先 dry-run 给用户确认后再传 false 真正执行"
    )]
    #[serde(default, deserialize_with = "lenient::opt_bool")]
    dry_run: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct FanControlParams {
    #[schemars(description = "风扇索引（数字，先 fan_selfheal_diag 查 fans 列表确认）")]
    idx: String,
    #[schemars(description = "目标转速百分比 0-100（越界自动钳制）")]
    pct: f64,
    #[schemars(
        description = "只出计划不执行（默认 true）。AI 先 dry-run 给用户确认后再传 false 真正执行"
    )]
    #[serde(default, deserialize_with = "lenient::opt_bool")]
    dry_run: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ToolbeltListParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ToolbeltRunParams {
    #[schemars(
        description = "工具箱工具名（先调 toolbelt_list 确认名称），如 crystaldiskinfo / wiztree / defraggler"
    )]
    tool: String,
    #[schemars(
        description = "传给工具的 CLI 参数列表（可选，透传不拼 shell；空则用工具默认参数）"
    )]
    args: Option<Vec<String>>,
    #[schemars(
        description = "用户已确认执行（medium/high 风险必须 true，low 风险可省略）。AI 应先把要执行的命令展示给用户，用户同意后再传 true"
    )]
    #[serde(default, deserialize_with = "lenient::opt_bool")]
    confirmed: Option<bool>,
    #[schemars(description = "执行超时秒数（默认 30，上限 300；超过强制终止）")]
    #[serde(default, deserialize_with = "lenient::opt_u64")]
    timeout_secs: Option<u64>,
}

// ── 阶段二：基准 + 压测（bench.rs）参数 ──────────────────────────────────

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct BenchCpuParams {
    #[schemars(description = "每项基准时长秒（默认 3，上限 30）")]
    #[serde(default, deserialize_with = "lenient::opt_u64")]
    secs: Option<u64>,
    #[schemars(description = "线程数（默认全部可用，上限 2 倍）")]
    #[serde(default, deserialize_with = "lenient::opt_usize")]
    threads: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct BenchMemoryParams {
    #[schemars(description = "每项基准时长秒（默认 3，上限 20）")]
    #[serde(default, deserialize_with = "lenient::opt_u64")]
    secs: Option<u64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct BenchDiskParams {
    #[schemars(
        description = "测试目录（默认系统临时目录；必须临时目录或用户指定，绝不碰用户数据）"
    )]
    path: Option<String>,
    #[schemars(description = "测试文件大小 MB（默认 256，16-2048）")]
    #[serde(default, deserialize_with = "lenient::opt_u64")]
    size_mb: Option<u64>,
    #[schemars(description = "只出计划不创建文件（默认 true）；用户确认后可传 false 真正跑")]
    #[serde(default, deserialize_with = "lenient::opt_bool")]
    dry_run: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct StressTestParams {
    #[schemars(description = "压测时长秒（默认 5 演示，上限 600）")]
    #[serde(default, deserialize_with = "lenient::opt_u64")]
    secs: Option<u64>,
    #[schemars(description = "超温熔断阈值℃（默认 95，60-110）")]
    #[serde(default, deserialize_with = "lenient::opt_f64")]
    max_temp: Option<f64>,
    #[schemars(description = "线程数（默认全部可用）")]
    #[serde(default, deserialize_with = "lenient::opt_usize")]
    threads: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct StressTestGpuParams {
    #[schemars(description = "温度熔断守门监控秒数（默认 5，上限 120）")]
    #[serde(default, deserialize_with = "lenient::opt_u64")]
    secs: Option<u64>,
    #[schemars(description = "GPU 超温熔断阈值℃（默认 90，60-110）")]
    #[serde(default, deserialize_with = "lenient::opt_f64")]
    max_temp: Option<f64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct MemTestParams {
    #[schemars(description = "测试缓冲大小 MB（默认 256，64-4096）")]
    #[serde(default, deserialize_with = "lenient::opt_u64")]
    size_mb: Option<u64>,
    #[schemars(description = "测试轮数（默认 1，1-10）")]
    #[serde(default, deserialize_with = "lenient::opt_u32")]
    rounds: Option<u32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct BsodAnalyzeParams {
    #[schemars(description = "最多解析几个转储（默认 10，上限 30）")]
    #[serde(default, deserialize_with = "lenient::opt_usize")]
    max_dumps: Option<usize>,
    #[schemars(description = "是否读根目录 MEMORY.DMP（默认 true）")]
    #[serde(default, deserialize_with = "lenient::opt_bool")]
    include_memory_dmp: Option<bool>,
}

// ── 阶段三：体检报告 / 传感器趋势 / 阈值告警（report.rs）参数 ────────────

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SystemReportParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SensorTrendParams {
    #[schemars(description = "返回最近 N 条趋势（默认 10，上限 50）")]
    #[serde(default, deserialize_with = "lenient::opt_usize")]
    n: Option<usize>,
    #[schemars(description = "输出格式：text（默认，人类可读）/ json（结构化数组，供前端折线图）")]
    format: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SensorAlertParams {
    #[schemars(description = "CPU 温度阈值℃（默认 85，50-110）")]
    #[serde(default, deserialize_with = "lenient::opt_f64")]
    cpu_temp: Option<f64>,
    #[schemars(description = "磁盘温度阈值℃（默认 55，30-90）")]
    #[serde(default, deserialize_with = "lenient::opt_f64")]
    disk_temp: Option<f64>,
    #[schemars(description = "GPU 温度阈值℃（默认 85，50-110）")]
    #[serde(default, deserialize_with = "lenient::opt_f64")]
    gpu_temp: Option<f64>,
    #[schemars(description = "内存占用阈值%（默认 90，50-100）")]
    #[serde(default, deserialize_with = "lenient::opt_f64")]
    mem_percent: Option<f64>,
    #[schemars(description = "磁盘剩余阈值 GB（默认 20，1-500）")]
    #[serde(default, deserialize_with = "lenient::opt_f64")]
    disk_free_gb: Option<f64>,
    #[schemars(description = "把告警写历史 JSONL（默认 false 只读体检）")]
    #[serde(default, deserialize_with = "lenient::opt_bool")]
    persist: Option<bool>,
}

// ── 阶段四：信息补全（hw.rs 扩展）参数 ───────────────────────────────────

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct HwCpuFeaturesParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct HwDramTimingsParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct HwDisplaysParams {}

// ── 阶段五：长尾（bench_gpu / IPMI / ACPI）参数 ────────────────────────────

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct BenchGpuParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct HwIpmiParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct HwAcpiParams {}

// ── 盘点/建议（steam.rs + cleanup.rs）参数 ────────────────────────────────

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SteamGamesParams {
    #[schemars(description = "每个库最多列出多少个游戏（按大小降序，默认 20，上限 100）")]
    #[serde(default, deserialize_with = "lenient::opt_usize")]
    top_n: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct CleanupSuggestionsParams {
    #[schemars(
        description = "要统计的目录绝对路径：**通常直接传盘根**（如 C:\\），本工具为只读统计允许盘根；也可传盘下任意目录（系统目录/用户主目录根仍会被守卫拒绝）"
    )]
    root: String,
    #[schemars(description = "返回按可清理字节降序的 Top N 个项（默认 20，上限 50）")]
    #[serde(default, deserialize_with = "lenient::opt_usize")]
    top_n: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct TopDirectoriesParams {
    #[schemars(description = "要统计的目录绝对路径，如 C:\\Users\\me\\Downloads")]
    path: String,
    #[schemars(description = "返回最大的 N 个子目录（默认 20，上限 100）")]
    #[serde(default, deserialize_with = "lenient::opt_usize")]
    top_n: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ProcessCpuUsageParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct UsbDevicesParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct DiskSmartRawParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct RecycleBinStatsParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct EnvVarsParams {
    name: Option<String>,
}

// ── ServerHandler（#[tool_handler] 自动生成 call_tool / list_tools） ────

#[tool_handler]
impl ServerHandler for AgentServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: Default::default(),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: rmcp::model::Implementation::from_build_env(),
            instructions: Some(
                "DiskPilot 的 AI 可操作工具集：只读磁盘/系统/文件/进程/程序清单查询。\
                 所有路径参数受安全守卫保护（盘根/系统目录/主目录根被拒绝）。\
                 回答磁盘空间、系统状态、目录内容、进程、已装软件问题时先用这些工具拿真实数据。"
                    .into(),
            ),
        }
    }
}
