//! 系统/文件/进程/程序域工具处理器（从 tools/mod.rs 拆分，2026-10-12）。

use crate::tools::params::*;
use crate::tools::{apps, envx, files, hw, process, sec, sys, system};
use crate::tools::{blocking_call, fmt_bytes, tool_error, AgentServer};
use rmcp::{
    handler::server::wrapper::Parameters, model::CallToolResult, tool, tool_router,
    ErrorData as McpError,
};

#[tool_router(router = tool_router_sys, vis = "pub(crate)")]
impl AgentServer {
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
        if let Err(e) = guard.check_read(&path) {
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
        if let Err(e) = guard.check_read(&path) {
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
        if let Err(e) = guard.check_read(&root) {
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
        description = "列出 USB 设备：控制器/集线器/外设 + 厂商/服务/状态（只读，PNP 枚举，普通权限，不读设备内容）。用户问『插了什么 USB 设备/U 盘/键鼠/摄像头识别到没有』时调用。"
    )]
    async fn usb_devices(
        &self,
        _p: Parameters<UsbDevicesParams>,
    ) -> Result<CallToolResult, McpError> {
        Ok(blocking_call(45, hw::collect_usb_devices, "USB 设备枚举").await)
    }

    #[tool(
        description = "读取 Windows Defender 安全中心状态（只读）：服务状态 + 实时保护开关 + 病毒库/引擎版本 + 最近快速扫描时间。用户问『杀毒软件开没开/Windows 安全中心状态/病毒库新不新』时调用；若系统用第三方杀软（Defender 被替换）会如实说明。"
    )]
    async fn sys_defender_status(
        &self,
        _p: Parameters<DefenderStatusParams>,
    ) -> Result<CallToolResult, McpError> {
        Ok(blocking_call(45, sec::collect_defender_status, "Defender 状态读取").await)
    }

    #[tool(
        description = "枚举本机用户账户（只读）：账户名/全名/是否禁用/是否锁定/本地或域账户/SID 尾段，排除内置系统账户（defaultuser0 等）。用户问『这台电脑有哪些用户/有几个账户/禁用账户是谁』时调用；无需管理员即可读取，受限时如实降级。隐私红线：绝不输出任何密码/token 信息。"
    )]
    async fn sys_user_accounts(
        &self,
        _p: Parameters<UserAccountsParams>,
    ) -> Result<CallToolResult, McpError> {
        Ok(blocking_call(45, sec::collect_user_accounts, "用户账户枚举").await)
    }

    #[tool(
        description = "查询本机回收站状态（只读，Windows API）：逐卷文件数与总大小 + 合计。用户问『回收站占了多少空间/回收站里有多少东西/能不能清回收站腾空间』时调用。只读状态，不执行清空。"
    )]
    async fn recycle_bin_stats(
        &self,
        _p: Parameters<RecycleBinStatsParams>,
    ) -> Result<CallToolResult, McpError> {
        Ok(blocking_call(30, envx::collect_recycle_bin_stats, "回收站状态查询").await)
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
}
