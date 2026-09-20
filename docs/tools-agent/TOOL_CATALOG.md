# TOOL_CATALOG · 工具清单（合入主项目工具墙的索引）

> ⚠️ **快照说明**：本表是 2026-09-10 的索引快照（51 个）。**当前 agent-server 实为 75 个 MCP 工具（64 只读 + 11 写操作）**——AIDA64 复刻（bench_cpu/memory/disk/gpu、stress_test(_gpu)、system_report、sensor_trend/alert、hw_cpu_features/dram_timings/displays/ipmi/acpi、mem_test、bsod_analyze、app_licenses、net_adapter_detail、sys_defender_status、sys_user_accounts 等）未全量入表。完整清单以代码为准：`crates/agent-server/src/tools/mod.rs`；下表补齐写操作后为 51+ 索引。
> 写操作（11 个，需确认门 + 进程/服务/路径守卫多重防线）：
> `process_kill` / `service_control` / `file_recycle` / `process_start` /
> `scheduled_task_manage` / `fan_selfheal_fix` / `uninstall_app` / `fan_control` /
> `toolbelt_run` / `stress_test` / `stress_test_gpu`（2026-09-10 三件套 + R5 卸载助手 + R4 风扇写控 + 工具墙执行 + AIDA64 压测，见 H 节）。返回统一为中文可读文本，直接可作为
> AI 对话素材。参数/描述以代码为准：`crates/agent-server/src/tools/mod.rs`。
> 冒烟验证：`scripts/smoke_test.py`（stdio 全链路，2026-09-12 起 51/51 起步，后逐项追加断言）。

## 总览

### A. 文件/磁盘（8）

| 工具 | 用途 | 参数 | 实现来源 |
|---|---|---|---|
| `disk_health` | 所有分区的总/已用/可用与使用率 | 无 | WinAPI `GetLogicalDrives` + `GetDiskFreeSpaceExW` |
| `disk_partition_usage` | 某路径所在卷的用量（细粒度） | `path` | WinAPI `GetDiskFreeSpaceExW` |
| `disk_volume_meta` | 各分区驱动器类型/文件系统/卷标（换机可移植） | 无 | WinAPI `GetDriveTypeW` + `GetVolumeInformationW` |
| `file_type_stats` | 目录按扩展名聚合的文件数/字节 | `path`, `top_n?` | jwalk 并行遍历 |
| `file_tree` | 目录树（深度+节点数双限） | `path`, `max_depth?`, `max_nodes?` | jwalk 并行遍历 + PathGuard |
| `list_dir` | 目录直接子项（名单/大小，不递归） | `path`, `limit?` | 标准库 `read_dir` + PathGuard |
| `read_file` | 文本文件内容（UTF-8/16 自动，256KB 截断） | `path` | 标准库 + PathGuard |
| `find_files` | 按文件名关键字递归查找 | `root`, `keyword`, `max_hits?` | jwalk 并行遍历 + PathGuard |

### B. 磁盘深入（4）

| 工具 | 用途 | 参数 | 实现来源 |
|---|---|---|---|
| `disk_find_biggest_files` | 某目录下最大的文件（递归，顶 N） | `path`, `top_n?` | jwalk 并行遍历 + PathGuard |
| `disk_find_duplicate_files` | 疑似重复文件（按大小分组 + 头部 64KB 哈希） | `path`, `top_n?` | jwalk + `DefaultHasher` |
| `disk_io_usage` | 每块盘实时 IO 使用率与读写速率 | `sample_ms?` | PowerShell `Win32_PerfFormattedData_PerfDisk_PhysicalDisk` |
| `disk_top_directories` | 某目录的直接子目录占用 Top N（含全部子级大小） | `path`, `top_n?` | jwalk 并行遍历 + PathGuard |

### C. 系统/进程/程序（8）

| 工具 | 用途 | 参数 | 实现来源 |
|---|---|---|---|
| `system_info` | 系统版本/CPU 核心/内存/开机时长 | 无 | WinAPI `GetMemoryStatusEx` + `GetTickCount64` + 注册表 ProductName |
| `list_processes` | 进程列表（PID/名称/内存/路径/父进程） | `top_n?` | WinAPI Toolhelp32 快照 + Psapi + `QueryFullProcessImageNameW` |
| `process_info` | 单个进程详情（内存/可执行文件/父进程） | `pid` | WinAPI `OpenProcess` + Psapi + Toolhelp32 |
| `app_list` | 已安装程序清单（版本/发布者/安装位置/大小） | `keyword?`, `limit?` | WinAPI 注册表 `Uninstall` 键枚举 |
| `sys_services` | Windows 服务：名/状态/启动类型/ImagePath/PID | `keyword?`, `show_disabled?`, `top_n?` | PowerShell `Win32_Service` + 注册表 `CurrentControlSet\Services` |
| `sys_drivers` | 设备驱动：名/类型/厂商/日期/版本/状态 | `keyword?`, `top_n?` | PowerShell `Win32_PnPSignedDriver` |
| `sys_boot_items` | 开机自启项（5 Run 键 + 2 启动文件夹 + 可选计划任务） | `include_scheduled?` | 注册表枚举 + `read_dir` + 僵尸检测 |
| `process_cpu_usage` | 进程 CPU 占用 Top 30（两次 GetProcessTimes 差值采样） | 无 | WinAPI `GetProcessTimes` + Toolhelp32 |

### H. 系统控制 + 应用卸载（写操作，7 个）——2026-09-10 新增（R4 写操作三件套 + 既有 2 个 + R5 卸载助手）

> **铁律**：写工具由主项目 agent.rs 桥接层做 `confirmed=true` 硬校验（返回
> `agent:confirm:` 前缀），前端走 `sys.control` / `file.recycle` / `app.uninstall` 权限门（L2 默认关，
> 每次确认）。agent-server 侧另有**路径/进程/服务/任务守卫**——系统关键进程（PID<5）、
> 系统目录中的进程、系统关键服务、系统内置任务、非白名单卸载器一律拒绝。三层防线，缺一不可。

| 工具 | 用途 | 参数 | 实现来源 |
|---|---|---|---|
| `process_kill` | 结束指定 PID 的进程（**写操作**，可逆性由主项目 undo 保障） | `pid` | PowerShell `Stop-Process` + 系统关键 PID/系统目录守卫 |
| `service_control` | 启停/重启 Windows 服务（**写操作，可逆**） | `name`, `action` | PowerShell `Start-Service` / `Stop-Service` / `Restart-Service` + 系统关键服务黑名单 |
| `file_recycle` | 把单个文件/目录移入系统回收站（**写操作，可逆**，绝不直接删除） | `path` | `trash` crate（与主项目 executor 同款，含误报兜底）+ PathGuard |
| `process_start` | 启动已安装程序（**写操作，白名单执行**，启动即返回不等待） | `command`, `args?`, `cwd?` | `std::process::Command spawn` + 系统/已安装程序目录白名单 + 仅 `.exe` 无参白名单 |
| `scheduled_task_manage` | 计划任务启用/禁用/只读查询（**写操作，仅可逆动作**，不开放 delete/run） | `name`, `action`, `dry_run?` | PowerShell `Enable-ScheduledTask` / `Disable-ScheduledTask` + 系统任务路径黑名单 |
| `fan_selfheal_fix` | 修复风扇写控通道（**写操作 L2**，用户确认后执行） | `action`, `dry_run?` | `fancmd` 驱动安装/复位（见 selfheal 模块） |
| `uninstall_app` | 卸载已安装程序：启动其**官方卸载器**（**写操作**，卸载界面由用户操作） | `name` | 注册表 `Uninstall` 键 + UninstallString 白名单解析（见下） |

**安全守卫细节**：
- `process_kill`：PID < 5（System Idle / System / 会话管理）拒绝；可执行文件在系统
  目录（`C:\Windows` 等）拒绝；agent-server 自身 PID 拒绝。
- `service_control`：系统关键服务黑名单（wininit / lsass / services / tcpip /
  stornvme 等）拒绝；目标服务不存在拒绝；已经是目标状态返回「无需操作」。
- `file_recycle`：盘根 / 系统目录 / 用户主目录根拒绝（复用 PathGuard）；路径不存在
  拒绝；回收失败（被占用/无权限）明确报错不静默；只进回收站绝不 `delete`。
- `process_start`：命令必须解析为绝对路径、扩展名必须 `.exe`、目录必须命中白名单
  （`System32` / `SysWOW64` / `Program Files` / `LocalAppData` 等环境变量推导）——
  AI 不能拼任意命令行（参数走 `args` 数组原样传入，不拼 shell 字符串）。
- `scheduled_task_manage`：任务路径以 `\Microsoft\` / `\Windows\` / `\System32\` 开头
  的系统内置任务拒绝修改（query 只读仍放行）；通配 `\*` 拒绝；`delete` / `run` 动作
  不存在（只 enable / disable / query）；enable/disable 可逆（禁用后可用 enable 恢复）。
- `uninstall_app`：按 `DisplayName` 匹配已安装程序；`UninstallString` 拆出 exe 与参数
  （支持引号包裹路径）；exe 必须命中已安装目录白名单（`Program Files` /
  `Program Files (x86)` / `LocalAppData` 等环境变量推导）且文件存在；参数必须全部落在
  静默参数白名单（`/s` `/silent` `/quiet` `/qn` `/verysilent` `/suppressmsgbaxes` /
  `/norestart` `/usecurrentuser` `/allusers` `/currentuser` `/qb`），拒绝 `runas` /
  `delete` / `format` / `rem` 等破坏性参数；卸载器分离启动（界面由用户操作，不自动确认）。
- 真实可逆往返已实测：`AdskLicensingService` stop→start 完整恢复（2026-09-10）；
  `file_recycle` 真实文件进回收站已实测（2026-09-10）。

### D. 硬件（10）

| 工具 | 用途 | 参数 | 实现来源 |
|---|---|---|---|
| `hw_cpu` | CPU 型号/核心/线程/频率/缓存/实时占用 | 无 | PowerShell CIM `Win32_Processor` |
| `hw_gpu` | 显卡型号/显存/驱动/分辨率/刷新率 + N 卡实时 | 无 | WMI `Win32_VideoController` + `nvidia-smi` |
| `hw_memory` | 内存条容量/频率/厂商 + 总容量 + XMP 诊断 | 无 | WMI `Win32_PhysicalMemory` |
| `hw_motherboard` | 主板/BIOS/整机品牌 | 无 | WMI `Win32_BaseBoard`/`Win32_BIOS`/`Win32_ComputerSystem` |
| `hw_temperature` | ACPI 热区温度 + GPU 温度 | 无 | WMI `MSAcpi_ThermalZoneTemperature` + `nvidia-smi` |
| `hw_sensors` | **全量传感器快照（AIDA64 同类）**：CPU/GPU/主板温度、风扇转速、电压、功耗、频率、负载 | 无 | `fancmd sensors`（LibreHardwareMonitor 0.9.4 内核），缺驱动降级 ACPI/SMART |
| `hw_battery` | 电量/健康度/充电状态/循环次数 | 无 | WMI `Win32_Battery`/`Win32_SystemBattery` |
| `hw_disk_smart` | 磁盘健康/SMART：通电时长/温度/磨损/错误 | 无 | PowerShell `Get-PhysicalDisk` + `Get-StorageReliabilityCounter` |
| `usb_devices` | USB 设备列表（控制器/集线器/外设 + 厂商/服务/状态） | 无 | PNP 枚举（普通权限，不读设备内容） |
| `disk_smart_raw_attributes` | 磁盘可靠性计数器明细（通电时长/温度/磨损/读写错误/启停） | 无 | PowerShell `Get-StorageReliabilityCounter` |

### E. 网络（5）

| 工具 | 用途 | 参数 | 实现来源 |
|---|---|---|---|
| `net_status` | 网卡名/状态/IP/链接速率（虚拟标出） | 无 | WinAPI `GetAdaptersAddresses` + PS `Get-NetIPConfiguration` 兜底 |
| `net_connections` | TCP/UDP 连接：本地/远端/状态/所属进程 | `pid?`, `keyword?`, `top_n?` | WinAPI `GetExtendedTcpTable`/`GetExtendedUdpTable` |
| `net_speed` | 实测收发速率（采样窗口） | `interval_ms?` | PowerShell `Get-NetAdapterStatistics` 双采样 |
| `net_share` | SMB 共享列表（会话/打开文件需管理员） | 无 | PowerShell `Get-SmbShare` |
| `net_wifi` | 无线网卡状态 + 已保存 WiFi 名（绝不读密码） | 无 | WinAPI `WlanOpenHandle`/`WlanEnumInterfaces` + PS 兜底 |

### F. 安全/审计（3）

| 工具 | 用途 | 参数 | 实现来源 |
|---|---|---|---|
| `security_event_logs` | 系统事件日志（错误/警告），按日志名/小时 | `logs?`, `hours?`, `max_events?` | PowerShell `Get-WinEvent` |
| `security_firewall_rules` | 防火墙规则（方向/动作过滤） | `direction?`, `action?`, `top_n?` | PowerShell `Get-NetFirewallRule` + Port/AddressFilter |
| `security_login_events` | 登录/注销（4624/4625）+ PS 脚本块 4104 | `max_events?` | PowerShell `Get-WinEvent` |

### G. 盘点/建议（4）

| 工具 | 用途 | 参数 | 实现来源 |
|---|---|---|---|
| `steam_games` | Steam 游戏库盘点：库根/游戏数/总占用 + 各游戏大小/上次游玩/是否幽灵/建议清理 | `top_n?` | 注册表定位 Steam + 读 ACF（复用 `diskpilot-steam-inspector`） |
| `cleanup_suggestions` | 按软件分类给出可清理建议（scaffold 缓存/临时目录占用，**只出建议不删除**） | `root`, `top_n?` | jwalk + glob 匹配（复用 `diskpilot-scaffold`） |
| `recycle_bin_stats` | 回收站内容盘点（总占用/文件数/最近删除） | `top_n?` | WinAPI `SHQueryRecycleBin` + 枚举 |
| `env_vars` | 环境变量查询（全部或单个） | `name?` | 标准库 `std::env::vars()` |

## 逐工具说明

### disk_health
- **何时调**：用户问磁盘空间 / 哪块盘快满了 / 该清哪里之前先调用。
- **返回**：`磁盘概览（N 个分区）：- C:：总 931.1 GB / 已用 330.4 GB（35.5%）/ 可用 600.7 GB ...`
- **来源**：纯 WinAPI 实时读取，不依赖主项目 scan 缓存，独立可用。

### disk_partition_usage
- **何时调**：需要「这个路径所在卷」的用量时（同一物理盘不同挂载点用量不同）。
- **参数**：`path`（必填）——任意存在路径，返回其所在卷的用量。

### disk_volume_meta
- **何时调**：AI 需要知道某块盘是「本地磁盘 / 网络驱动器 / 光盘 / 可移动」，或文件系统是不是 NTFS、卷标叫什么。
- **返回**：每个分区的驱动器类型、文件系统名、卷标。类型由 Win32 `DRIVE_*` 枚举映射为中文。
- **可移植性**：不依赖盘符假设，用 `GetLogicalDrives` 枚举实际存在的盘，换机即适配。
- **注**：SSD/HDD 物理介质检测需 `DeviceIoControl` + `STORAGE_DEVICE_DESCRIPTOR`，windows-sys 0.59 的 Win32 面未导出，见 DESIGN 七章。

### file_type_stats
- **何时调**：AI 分析「什么文件占了空间」。返回各扩展名（.mp4/.dll/.log…）的文件数与总字节，按占用降序。
- **参数**：`path`（目录绝对路径）、`top_n`（默认 20，上限 100）。
- **性能**：jwalk 并行遍历，多核机器比串行快数倍。
- **硬上限**：200,000 文件 / 20 层深度，超限结果标记 `truncated`，绝不无界遍历。

### file_tree
- **何时调**：AI 回答「哪里占空间」的结构化补充，展示一棵可读的目录树。
- **参数**：`path`、`max_depth`（默认 4，钳到 1..=10）、`max_nodes`（默认 40，钳到 1..=500）；超限标记 `truncated`。
- **性能**：jwalk 并行遍历，跳过符号链接防环。

### list_dir
- **何时调**：AI 核实「目录里到底有什么」。
- **安全边界**：PathGuard 拒绝盘根（`C:\`）、系统目录（`C:\Windows` 等）、用户主目录根。返回 `isError=true` + 中文原因。

### read_file
- **何时调**：核实某个配置文件 / 脚本 / 日志内容。
- **限制**：只接受文件拒绝目录；UTF-8/UTF-16 BOM 自动识别；超 256KB 截断并提示。

### find_files
- **何时调**：AI 找「某个文件在哪」。
- **参数**：`root`（搜索根）、`keyword`（大小写不敏感）、`max_hits`（默认 50）。
- **性能**：jwalk 并行遍历，跳过符号链接防环；500,000 命中上限防爆。

### disk_find_biggest_files
- **何时调**：用户问「什么文件占了空间 / 哪个文件最大」。
- **参数**：`path`（必填，目录）、`top_n`（默认 20，钳到 5..=100）。
- **性能**：jwalk 并行遍历，500,000 文件上限；按字节降序取前 N。
- **安全**：PathGuard 拒绝盘根/系统目录/主目录根。

### disk_find_duplicate_files
- **何时调**：用户问「哪些文件重复了 / 有什么可删的重复文件」。
- **参数**：`path`（必填，目录）、`top_n`（默认 20，钳到 5..=100）。
- **原理**：按大小分组 → 同尺寸 ≥2 的组做头部 64KB 哈希比对（`DefaultHasher`）。**只比对头部 64KB，不是全文件哈希**——返回会明确标注，避免 AI 据此判全等。
- **性能**：200,000 文件上限，组员数超过 top_n 时跳过深度比对并标注「未深度比对」。

### disk_io_usage
- **何时调**：用户问「磁盘是不是满了 / 卡了 / 谁在读写盘」。
- **参数**：`sample_ms`（默认 1000）。运行一次 PowerShell 采样，约 1 秒。
- **来源**：`Win32_PerfFormattedData_PerfDisk_PhysicalDisk`，过滤 `_Total` 与逻辑盘行，输出每盘读/写 B/s 与占用率。

### system_info
- **何时调**：用户问「电脑什么配置 / 现在卡不卡 / 看看电脑状态」。
- **返回**：系统产品名（注册表 `HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion` ProductName，真实值）、CPU 逻辑核心、内存总量/已用/使用率、开机时长。
- **注意**：绝不用 `GetVersionExW` 做展示（Win8.1+ 被兼容层遮蔽为 6.2.9200），注册表读取失败才兜底并标注。

### list_processes
- **何时调**：用户问「有什么程序在跑 / 某进程存在吗」。
- **现状**：PID + 名称 + **内存占用（Psapi WorkingSet，按内存降序）+ 可执行文件路径（`QueryFullProcessImageNameW`）+ 父进程 PID**。
- **参数**：`top_n`（默认 50，上限 500），按内存降序取前 N。

### process_info
- **何时调**：AI 需要「某个 PID 的进程详情」——内存、exe 路径、父进程。
- **参数**：`pid`（必填）。未找到返回 `isError`。

### app_list
- **何时调**：用户问「装了什么软件 / 这个软件能卸载吗 / 查一下未安装来源」的只读前置。
- **参数**：`keyword`（按名称+发布者过滤）、`limit`（默认 50）。
- **来源**：枚举 `HKLM\SOFTWARE\...( \WOW6432Node)\...\Uninstall` + `HKCU`，滤掉 `SystemComponent` 与 `ParentKeyName` 组件；`EstimatedSize`（KB）换算为 MB。

### sys_services
- **何时调**：用户问「什么服务在跑 / 某服务是干嘛的 / 怎么改启动方式」的只读前置。
- **参数**：`keyword`（按服务名/显示名过滤）、`show_disabled`（是否含已禁用，默认 false）、`top_n`（默认 50，上限 200）。
- **实现**：`Win32_Service` 一次性拿状态映射 + 注册表 `HKLM\SYSTEM\CurrentControlSet\Services` 枚举 ImagePath/Start/Type。系统服务的 ImagePath WMI 为空，**必须读注册表补齐**；Start DWORD 0=Boot/1=System/2=Automatic/3=Manual/4=Disabled 映射中文；Type 1/2=内核驱动、32/36=Win32 进程。
- **性能**：注册表键枚举 + 值读取，955 条服务在冒烟机约 2 秒。

### sys_drivers
- **何时调**：用户问「装了什么驱动 / 某驱动是干嘛的 / 驱动是不是太旧」。
- **参数**：`keyword`（按驱动名过滤）、`top_n`（默认 50，上限 200）。
- **实现**：`Win32_PnPSignedDriver`（60s 超时）；微软驱动的日期年份 <2020 且版本以 `10.0` 开头时标注「微软驱动日期可能失真」。
- **说明**：NVIDIA/Intel 等第三方驱动日期来自厂商签名，可信度高；系统内置驱动日期常失真。

### sys_boot_items
- **何时调**：用户问「开机自动启动了什么 / 怎么精简开机项」。
- **参数**：`include_scheduled`（是否附带计划任务，默认 false——199 条任务很慢）。
- **覆盖**：5 个注册表 Run 键（含 `HKCU/HKLM\...\Policies\Explorer\Run` 策略强制项、`WOW6432Node` 32 位视图）+ 用户/机器 2 个启动文件夹 + 可选 `Get-ScheduledTask`。
- **僵尸检测**：解析命令第一 token → 展开 `%VAR%` → `Path::exists()`，不存在标「僵尸启动项」。
- **红线**：绝不修改/禁用/删除任何启动项，纯枚举。

### hw_cpu
- **何时调**：用户问「CPU 是什么 / 几个核 / 占用高不高」。
- **返回**：型号、核心/线程、标称与实时频率、L2/L3 缓存、厂商、实时占用率。
- **坑处理**：L2CacheSize WMI bug（≤0 或 <16 视为垃圾值 →「未知」，不把 3 KB 当真）；多路 CPU 取第 1 路并标注共 N 路。

### hw_gpu
- **何时调**：用户问「什么显卡 / 显存多大 / GPU 占用」。
- **返回**：每张卡的型号、显存、驱动版本/日期、分辨率/刷新率；NVIDIA 卡附实时温度与占用（`nvidia-smi`）。
- **坑处理**：AdapterRAM 32 位溢出（≥4GB 显示「≥4 GB（WMI 32 位溢出，实际可能更大）」）；`==0` 显示「共享系统内存 / 未知」。

### hw_memory
- **何时调**：用户问「内存多大 / 频率多少 / 该不该开 XMP」。
- **返回**：每根内存条容量/频率/厂商/型号 + 总容量 + XMP/EXPO 诊断（`ConfiguredClockSpeed < Speed` →「可能未生效」）。
- **坑处理**：DDR5 的 `Speed` 是有效频率一半，输出标注「速率单位为 MT/s（DDR4 及以下，无 2 倍折算）」；不做 CL 时序（WMI 拿不到）。

### hw_motherboard
- **何时调**：用户问「这是什么主机 / 主板是什么」。
- **返回**：主板厂商/型号/序列号（`Default string` 占位串 →「未知」）、BIOS 厂商/版本/发布日期（原样传递）、整机品牌。
- **来源**：`Win32_BaseBoard`/`Win32_BIOS`/`Win32_ComputerSystem`。

### hw_temperature
- **何时调**：用户问「电脑温度 / 烫不烫」。
- **实现（四级通道）**：**① HWiNFO 共享内存**（普通权限可读 CPU/GPU/主板/磁盘全部温度，无需管理员）→ **② 自研 SuperIO 直读**（superio.rs：inpoutx64 驱动已装则普通权限直读 ITE/Nuvoton/Winbond 环境寄存器，CPU/主板/辅助温度，无需 HWiNFO/LHM）→ **③ ACPI 热区** `MSAcpi_ThermalZoneTemperature`（`/10 - 273.15` 转摄氏，`(0.0..=120.0)` 过滤；热区未就绪的值 2731/0 被过滤）+ NVIDIA GPU 实时温度 → **④ 磁盘 SMART**。
- **降级**：全部过滤掉时明确说「温度传感器读不到（ACPI 热区未暴露）」，附通用建议，**绝不返回空列表假装成功**。输出首行会标注实际数据源（HWiNFO 共享内存 / 自研 SuperIO / ACPI + GPU + SMART）。

### hw_sensors（2026-09-12 新增 AIDA64 同类全量快照；2026-09-17 接入 HWiNFO + 自研 SuperIO）
- **何时调**：用户问「CPU 温度多少 / 显卡温度 / 风扇转速 / 电压 / 功耗 / 全机传感器总览」。比 `hw_temperature` 全、比 AIDA64 开箱即用。
- **实现（四级通道）**：**① HWiNFO 共享内存**（`HWiNFO_SENS_SM2`，普通权限即可读全部传感器，无 Ring0/管理员依赖）→ **② 自研 SuperIO 直读**（superio.rs：`LoadLibraryW` 动态加载 inpoutx64/inpout32/WinRing0/WinIo，多芯片 × 双端口枚举 ITE/Nuvoton/Winbond，直读环境寄存器——主板/CPU/辅助温度 + 4-6 路风扇转速 + PWM 占空比，普通权限，无 LHM 依赖）→ **③ `fancmd sensors`**（LibreHardwareMonitor 0.9.4 内核，与 FanControl / AIDA64 同类硬件枚举；CPU 核心温度需 Ring0 驱动+管理员）→ **④ 系统自带通道** `hw_temperature`（ACPI + SMART + nvidia-smi/ADL GPU）。
- **HWiNFO 启用方法（激活 ① 通道）**：HWiNFO 安装并启动后，打开 `设置（Settings）` → `主设置（Main Settings）` → 勾选 `启用共享内存支持（Enable Shared Memory Support）`，确定即可，**不需要提权、不安装驱动、不加载 Ring0 模块**。免费版 HWiNFO 的共享内存在运行约 12 小时后停止更新，重启 HWiNFO 即可。HWiNFO 未运行时工具自动走 ②③④，行为与 2026-09-12 一致。
- **自研 SuperIO（②）是本项目自己的通道**：不依赖任何外部程序（HWiNFO/LHM 都是可选的），只需 inpoutx64 内核驱动在位（`sc query inpoutx64` 显示 RUNNING）且 DLL 在候选目录（工具目录 / DISKPILOT_TOOLS_DIR / exe 目录）。驱动缺失时输出明确诊断引导 AI 用 `fan_selfheal_fix action=install_driver` 安装——装一次后普通权限永久可用。2026-09-17 本机实测（ITE 0x8689/端口 0x2E）：主板 38℃ / CPU 42℃ / 辅助 68℃ + 4 路风扇 630-2336 RPM。
- **管理员下实测（2026-09-12，i7-13700KF）**：CPU 每核温度全量（Core Max=80℃、Core Average=72.4℃、16 核各自温度）、频率（睿频 5327MHz）、电压（CPU Core=1.39V）、功耗（Package=181.3W）；GPU 温度/功耗（RTX 5090 56℃/86.9W）；5 块盘 SMART 温度（NVMe 49-59℃、机械 32℃）。**这就是 AIDA64 的完整覆盖**。
- **权限说明（关键：温度权限边界）**：
  - **HWiNFO 通道（①）不需要管理员**：HWiNFO 以普通权限运行并把传感器写入共享内存段，DiskPilot 用 `OpenFileMappingW` + `MapViewOfFile` 只读映射，**非管理员进程也能读到 CPU/GPU/主板/磁盘温度**。
  - **自研 SuperIO 通道（②）不需要管理员**：只要 inpoutx64 内核驱动由系统以服务方式加载（RUNNING），普通权限进程 `LoadLibraryW` 其 DLL + 端口读写即可（端口读写由内核驱动代理，无需用户令牌）。2026-09-17 实测 `elevated=False` 下直读成功。
  - **LHM 通道（③）需要管理员**：LHM 读 CPU 核心温度走 MSR，需 Ring0 驱动 + 管理员令牌。仅当 HWiNFO + SuperIO 都不可用且进程非管理员时，CPU 温度返回 null，工具会输出**明确诊断**。
  - **写操作仍走 L2 确认门**：`fan_control` / `fan_selfheal_fix` / `control_service` 等写操作不受影响，仍按主项目权限中心 L2 + 确认门双重守护——本专项只让**读**更完整，不放开**写**。
- **只读**：不写任何寄存器、不调速、不加载驱动（SuperIO 只 LoadLibrary 已装 DLL、不安装）、纯快照查询（L0）。共享内存只读映射（`FILE_MAP_READ`），不创建/修改任何映射段。

### hw_superio（2026-09-17 新增 自研 SuperIO 直读）
- **何时调**：用户问「主板读到的温度/风扇转速是什么 / SuperIO 芯片是什么 / HWiNFO 没装时还能读温度吗」。HW_temperature/HW_sensors 已自动走该通道，此工具用于**单独查看自研直读链路**。
- **实现**：`LoadLibraryW` 动态加载端口驱动（inpoutx64 → inpout32 → WinRing0x64 → WinRing0 → WinIo64 → WinIo32，候选目录 = exe 目录 / 上溯 Tools 布局 / System32 / `DISKPILOT_TOOLS_DIR`）→ RTC 金标准验证端口真实读写 → 0x2E/0x4E 双端口 × ITE/Nuvoton/Winbond 三套 enter 序列枚举芯片 → LD#4 环境控制器基址 → 直读 0x29-0x2B 温度 + 16 位 TACH 风扇转速（rpm = 1.35e6/(tach×2)）+ PWM 占空比。
- **实测（2026-09-17，ITE 0x8689）**：驱动 inpoutx64.dll / 芯片 0x8689 / 端口 0x2E / 环境基址 0x0A40；主板 38℃ / CPU 42℃ / 辅助 68℃；风扇 4 路 630-2336 RPM；PWM 6 通道占空比。
- **红线**：**纯只读**——不写任何寄存器、不调速、不进入写模式。读写只针对 SuperIO 环境寄存器（地址口 + 数据口两条端口），绝不碰盘/网络/其他设备。（L0）


### hw_battery
- **何时调**：用户问「笔记本电池健康吗 / 续航 / 要不要换电池」。
- **返回**：电量百分比、健康度（`Win32_SystemBattery.EstimatedChargeRemaining`）、充电状态、循环次数（`Win32_Battery.CycleCount` 嵌套数组）、设计容量。
- **降级**：台式机/无电池 →「未检测到电池（台式机或电池不可用）」。

### hw_disk_smart
- **何时调**：用户问「硬盘健康吗 / 通电多久 / 要坏了吗」。
- **实现**：`Get-PhysicalDisk` + `Get-StorageReliabilityCounter`（Win10 1607+，**普通权限即可，不需要管理员，不需要 DeviceIoControl**）。字段：PowerOnHours、Temperature、TemperatureMax、Wear、ReadErrorsTotal/WriteErrorsTotal、StartStopCycleCount、LoadUnloadCycleCount、HealthStatus。
- **坑处理**：温度单位启发式 `if t > 60 { (t-32)*5/9 } else { t }`（控制器混报摄氏/华氏）；NVMe 的 Wear/PowerOnHours 常见 null → 明确标注「未上报（NVMe 控制器常见行为，不代表盘体健康有问题）」。
- **红线**：`wmic` 已在 Win11 24H2 移除，绝不用。

### net_status
- **何时调**：用户问「电脑联网了吗 / 网卡是什么 / IP 多少 / 网速多少」。
- **实现**：`GetAdaptersAddresses`（AF_UNSPEC，原生，不需要 PS）→ 网卡名/状态/IP/链接速率；虚拟适配器（Hyper-V/WSL/VPN）标注「（虚拟）」；PS `Get-NetIPConfiguration` 兜底补全 IP。
- **坑处理**：`GetAdaptersAddresses` 首次调用用 size=0 探测（ERROR_BUFFER_OVERFLOW → 按返回大小分配）；`TransmitLinkSpeed`/`ReceiveLinkSpeed` 单位为 bytes/sec（×8 得 bps）。

### net_connections
- **何时调**：用户问「谁在连网 / 哪个程序在联网 / 某某连接是什么」。
- **参数**：`pid`（按进程过滤）、`keyword`（按进程名/远端 IP 过滤，大小写不敏感）、`top_n`（默认 30，上限 100，钳到 ≥1）。
- **实现**：`GetExtendedTcpTable`（AF_INET+AF_INET6）+ `GetExtendedUdpTable`（`TCP_TABLE_OWNER_PID_ALL`）；网络字节序 → `u32::from_be`；PID→进程名 join 进程表；`MIB_TCP_STATE` 映射中文。
- **默认过滤**：TimeWait + 本地回环对端（避免噪音），返回开头说明「已默认过滤 TimeWait 与回环对端（跳过 N 条）」；`keyword` 为空只返回**已建立**连接，给 keyword/PID 或 `top_n` 大时放宽。

### net_speed
- **何时调**：用户问「当前网速快不快 / 下载多少兆」。
- **参数**：`interval_ms`（1000-10000，默认 2000）。运行一次 PS 双采样，约 1-10 秒。
- **实现**：`Get-NetAdapterStatistics` 前后两次 ReceivedBytes/SentBytes 差值 ÷ 窗口 → KB/s；附带累计收/发总量；虚拟网卡标注「（虚拟，无真实流量）」。

### net_share
- **何时调**：用户问「电脑共享了什么文件夹 / 开了哪些共享」。
- **实现**：`Get-SmbShare` → 共享名/路径/说明/是否特殊共享（ADMIN$/C$/IPC$ 标「特殊共享，系统保留名」）/作用域（LOCALHOST=仅本机 / ALL=局域网可见）。
- **降级**：会话/打开文件统计（`Get-SmbSession`/`Get-SmbOpenFile`）需管理员 →「需要管理员权限（拒绝访问）」，只返回共享列表本身。

### net_wifi
- **何时调**：用户问「WiFi 连的是什么 / 保存了哪些 WiFi」。
- **实现**：`WlanOpenHandle(2)`/`WlanEnumInterfaces`/`WlanGetProfileList` → 网卡接口列表 + 已保存的 WiFi profile 名；PS `netsh wlan show interfaces` 兜底。
- **红线**：**绝不读取 WiFi 密码**（profile 密码需 `WlanGetProfile` + 明文 XML，属于敏感信息，违反本工具集只读+隐私原则）；无无线网卡 →「本机没有无线网卡（无 WiFi 适配器）」。

### security_event_logs
- **何时调**：用户问「最近系统出了什么错误 / 蓝屏 / 警告 / 日志」。
- **参数**：`logs`（逗号分隔日志名，默认 System）、`hours`（回溯小时，默认 24，上限 720）、`max_events`（每日志上限，默认 20，上限 50）。
- **实现**：`Get-WinEvent -FilterHashtable @{LogName=...;Level=2,3;StartTime=...}`；级别映射中文（1=严重/2=错误/3=警告/4=信息）；`trunc_msg` 280 字符截断。
- **坑处理**：前置探测每个日志（`Get-WinEvent -LogName $log -MaxEvents 1`）——`-FilterHashtable` 对无权限日志**静默返回空**而非报错；`-Enabled` 是仅接受字符串 `True` 的开关。
- **降级**：Security 无管理员权限 →「安全日志：需要管理员权限，无法读取（登录失败审计不可用）」。

### security_firewall_rules
- **何时调**：用户问「防火墙开了什么 / 某程序被放行了吗」。
- **参数**：`direction`（Inbound/Outbound/Any，默认 Inbound）、`action`（Allow/Block/Any，默认 Allow）、`top_n`（默认 20，上限 100）。
- **实现**：`Get-NetFirewallRule` + `Get-NetFirewallPortFilter` + `Get-NetFirewallAddressFilter` 联查；输出规则名/端口/协议/远端地址/配置文件；`-Enabled True` + `-MaxEvents`。
- **红线**：只输出规则元数据，**绝不返回规则内部动作细节**（如程序路径），保持只读纯审计。

### security_login_events
- **何时调**：用户问「谁登录过这台电脑 / 有没有异常登录 / 有没有人跑过脚本」。
- **参数**：`max_events`（默认 20，上限 50）。
- **实现**：Security 4624/4625（登录/失败）+ PowerShell/Operational 4104（脚本块记录，普通权限可读，脚本攻击链高价值）。
- **降级**：Security 无管理员权限 →「[Security] 登录事件：需要管理员权限，无法读取」；PS 4104 普通权限即可读，会正常返回。

### disk_top_directories
- **何时调**：用户问「哪个文件夹占了空间 / 什么目录最大」——比单文件视角更实用（直接子目录含全部子级大小）。
- **参数**：`path`、`top_n`（默认 20，钳 1..=50）。
- **实现**：jwalk 单遍并行遍历，按 root 下第一层子目录聚合字节，降序输出 Top N。
- **安全边界**：盘根/系统目录/主目录根被 PathGuard 拒绝。

### process_cpu_usage
- **何时调**：用户问「哪个进程吃 CPU / 为什么卡 / 谁在烧 CPU」——与 `list_processes`（内存视角）互补。
- **参数**：无。
- **实现**：内部两次 `GetProcessTimes` + 200ms 采样窗口差值，输出单核百分比（多核满载 = 核数 × 100%）。

### usb_devices
- **何时调**：用户问「插了什么 USB 设备 / U 盘 / 键鼠 / 摄像头识别到没有」。
- **参数**：无。
- **实现**：PNP 枚举（控制器/集线器/外设 + 厂商/服务/状态），普通权限，**不读设备内容**。

### disk_smart_raw_attributes
- **何时调**：用户问「硬盘具体磨损多少 / 通电多久的明细」。
- **参数**：无。
- **实现**：PowerShell `Get-StorageReliabilityCounter` 可靠性计数（通电时长/温度/磨损/累计读写错误/启停循环），普通权限。
- **注意**：这是可靠性计数器，**不是**标准 SMART 属性表（ID 0x05 重分配扇区等需 smartctl，本工具不装）。

### steam_games
- **何时调**：用户问「装了哪些 Steam 游戏 / 哪个游戏占地 / Steam 该清谁」。
- **参数**：`top_n`（每库最多列多少个游戏，默认 20，钳 1..=100）。
- **实现**：注册表定位 Steam 根 + 读 ACF 清单（复用 `diskpilot-steam-inspector`）；建议逻辑来自其内置规则（≥30GB 且 ≥6 个月未玩 / ≥50GB 且 ≥3 个月未玩，Steam 本体/SteamVR 等系统组件绝不建议）。
- **降级**：找不到 Steam → 中文兜底说明（非 Err）。

### cleanup_suggestions
- **何时调**：用户问「C 盘/某盘有什么可清的 / 微信 QQ 缓存占了多少」。
- **参数**：`root`（目标目录，通常盘根）、`top_n`（默认 20，钳 1..=50）。
- **实现**：读 scaffolds TOML（`DISKPILOT_SCAFFOLDS` 环境变量 → 仓库 `scaffolds/`），单遍并行 walk + glob 匹配统计各 scope 命中的文件数/字节，降序输出。
- **红线**：**只出建议清单，绝不删除任何文件**（执行需用户确认走主项目清理流程）；未命中已知软件会明确说明。

## 与主项目图吧工具墙的关系

- 图吧 30 个 manifest 是「本地 exe 说明书」，AI 只能 GUI 启动给人看；本目录 51 个工具是
  「能力」，AI 可直接 `tools/call` 拿结构化结果。两者不冲突：合入后工具墙出现
  「AI 可操作」分区（见 DESIGN 六章合并方案）。
- 安全分级沿用主项目四级权限：43 个只读全部 L0（恒开）；8 个写操作
  （`process_kill` / `service_control` / `file_recycle` / `process_start` /
  `scheduled_task_manage` / `fan_selfheal_fix` / `uninstall_app` / `fan_control`）走 L2 + 确认门，server 端另做
  进程/服务/路径/任务/卸载器守卫。
- **可移植性**：路径守卫全部从环境变量（`SystemRoot`/`windir`/`ProgramFiles`/`ProgramData`/`USERPROFILE`）推导而非硬编码盘符，装在任意盘符的 Windows 都能识别；硬件查询全走 CIM/`Get-PhysicalDisk`（不需要管理员权限、不触发 UAC），NET 层用原生 WinAPI + PS 兜底；中文系统 PowerShell 输出经 UTF-8 强注入 + GBK 兜底解码，换机不乱码。
- **红线**：本工具集不读 WiFi 密码、不删除/禁用/篡改任何服务/驱动/启动项/防火墙规则，全部只读。