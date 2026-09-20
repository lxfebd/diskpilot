# agent-server · MCP 工具服务

DiskPilot 的 AI 可操作工具集 —— 一个遵循 Model Context Protocol（MCP）标准协议的
stdio 服务：AI（客户端）通过标准 `tools/list` / `tools/call` 就能真正读取系统状态、
查看磁盘、操作安全边界内的文件，而不是隔着一层 GUI。

协议层用官方 Rust SDK `rmcp`（MCP2026-07-28 规范，兼容 2025-11-25 及更早客户端），
工具实现为 `#[tool_router]` 宏注册的纯函数——新增工具只需加一个函数 + 一个 struct。

## 运行

```bash
cargo run -p agent-server          # 启动后通过 stdin/stdout 走 MCP 协议
```

## 已注册工具

共 **75 个**（2026-09-20）：64 只读 + 11 写操作（含 AIDA64 复刻基准/压测/报告/趋势/告警、信息补全、蓝屏/内存检测）。完整清单与每工具参数见
[`docs/tools-agent/TOOL_CATALOG.md`](../../docs/tools-agent/TOOL_CATALOG.md)。

| 领域 | 工具 | 风险 | 说明 |
|---|---|---|---|
| 磁盘/文件 | `disk_health` `disk_partition_usage` `disk_volume_meta` `file_type_stats` `file_tree` `list_dir` `read_file` `find_files` | L0 只读 | 分区用量/目录树/文件清单/文本读取（256KB 截断）|
| 磁盘深入 | `disk_find_biggest_files` `disk_find_duplicate_files` `disk_io_usage` `disk_top_directories` | L0 只读 | 最大文件/重复文件/实时 IO/子目录占用 |
| 系统/进程 | `system_info` `list_processes` `process_info` `app_list` `sys_services` `sys_drivers` `sys_boot_items` `process_cpu_usage` | L0 只读 | 系统信息/进程/已装程序/服务/驱动/启动项/CPU 采样 |
| 硬件 | `hw_cpu` `hw_gpu` `hw_memory` `hw_motherboard` `hw_temperature` `hw_battery` `hw_disk_smart` `usb_devices` `disk_smart_raw_attributes` `hw_cpu_features` `hw_dram_timings` `hw_displays` `hw_ipmi` `hw_acpi` `hw_sensors` | L0 只读 | CPU/GPU/内存/主板/温度/battery/SMART/USB/指令集/内存时序/显示器/IPMI/ACPI/全量传感器 |
| 网络 | `net_status` `net_connections` `net_speed` `net_share` `net_wifi` `net_adapter_detail` | L0 只读 | 网卡/连接/速率/SMB/WiFi（绝不读密码）/网卡明细（MAC/掩码/网关/DNS）|
| 安全/审计 | `security_event_logs` `security_firewall_rules` `security_login_events` `sys_defender_status` `sys_user_accounts` `app_licenses` | L0 只读 | 事件日志/防火墙规则/登录审计/Defender/用户账户/激活状态 |
| 盘点 | `steam_games` `cleanup_suggestions` `recycle_bin_stats` `env_vars` | L0 只读 | Steam 库/可清理建议/回收站/环境变量 |
| AIDA64 复刻 | `bench_cpu` `bench_memory` `bench_disk` `bench_gpu` `system_report` `sensor_trend` `sensor_alert` `mem_test` `bsod_analyze` | L0 只读 | CPU/内存/磁盘/GPU 基准、体检报告、传感器趋势/告警、内存检测、蓝屏分析 |
| **写操作** | `process_kill` `service_control` `file_recycle` `process_start` `scheduled_task_manage` `fan_selfheal_fix` `fan_control` `uninstall_app` `toolbelt_run` `stress_test` `stress_test_gpu` | **L1/L2 受控** | **结束进程/启停服务/回收站/启动进程/计划任务/风扇自愈与调速/卸载程序/工具墙执行/CPU·GPU 压测——必须确认，见下** |

> 写操作工具默认**不**在 server 侧直接放行：主项目 `agent.rs` 桥接层对这两个工具
> 要求 `confirmed == Some(true)` 才调用（否则返回 `agent:confirm:` 错误）；前端
> `sys.control` 权限门（L2 默认关）做第二道拦截。agent-server 自身还有进程/服务守卫
> （PID<5、系统目录进程、系统关键服务黑名单一律拒绝）——三层防线，缺一不可。

## 安全边界

- 所有路径参数（`list_dir` / `read_file` / `file_type_stats` / `find_files` 等）一律过
  `PathGuard`：拒绝盘根、系统目录（`C:\Windows` 等）、用户主目录根；读文件大小上限
  `read_file` 截断到 256KB。
- 只读工具无需任何确认；写操作工具（`process_kill` / `service_control` / `file_recycle` / `process_start` / `scheduled_task_manage` / `fan_selfheal_fix` / `fan_control` / `uninstall_app` / `toolbelt_run` / `stress_test` / `stress_test_gpu`）必须由
  主项目权限中心确认后放行，server 侧守卫兜底。
- 与主项目四权限级（L0 只读恒开 / L1 受控 / L2 高危 / L3 永禁）对齐设计。
