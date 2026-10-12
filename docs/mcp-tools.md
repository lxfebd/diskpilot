# DiskPilot MCP 工具参考

> agent-server 暴露的 MCP 工具全集。本页由 `tool_*.rs` 的 `#[tool]` 注解与 `params/` 参数定义生成，与运行时 `tools/list` 一致。

除工具外，服务还提供只读资源（`system://profile` / `system://disk-usage` / `system://env`）与任务模板（`system-report` / `cleanup-plan`），见 [server.rs](../crates/agent-server/src/server.rs)。

## 安全模型

- **只读工具（L0）**：查询类，无副作用，AI 可直接调用。
- **写工具（L1/L2）**：回收/卸载/启动/控制类，要求用户确认（`confirmed=true`）或经确认门。
- **路径守卫**：盘根 / 系统目录 / 用户主目录根被拒绝；写操作口径更严。
- **L3 永禁**：BIOS 刷写 / 格式化等（toolbelt_run）永久拒绝。

## 服务/启动域

| 工具 | 参数 | 说明 |
|---|---|---|
| `service_control` | name:String, action:String | 控制 Windows 服务：启动 / 停止 / 重启（**写操作，可逆**）。系统关键服务（内核/系统核心）会被安全守卫拒绝。 |
| `sys_boot_items` | include_scheduled:Option<bool> | 列出开机启动项：注册表 Run 键（含策略强制项与 32 位视图）+ 启动文件夹（只读，不删除不禁用），可附带计划任务。僵尸启动项（文件已不存在）会标红提示。 |
| `sys_drivers` | keyword:Option<String>, top_n:Option<usize> | 列出驱动（设备驱动）：名称/类型/厂商/驱动日期与版本/设备状态（只读，不需要管理员权限）。可按关键字过滤。 |
| `sys_services` | keyword:Option<String>, show_disabled:Option<bool>, top_n:Option<usize> | 列出 Windows 服务：服务名/显示名/状态/启动类型/进程 PID（只读，不需要管理员权限）。可按关键字过滤、可含已禁用服务——服务启动类型与 ImagePath 直接读注册表，准确度高。 |

## 盘点/杂项域

| 工具 | 参数 | 说明 |
|---|---|---|
| `env_vars` | name:Option<String> | 读取环境变量值（只读）：传 name 查指定变量；不传列出常用子集（PATH/TEMP/USERPROFILE/APPDATA 等）。用户问『某个路径在哪/环境变量配没配/命令找不到是不是 PATH 问题』时调用。 |
| `recycle_bin_stats` | — | 查询本机回收站状态（只读，Windows API）：逐卷文件数与总大小 + 合计。用户问『回收站占了多少空间/回收站里有多少东西/能不能清回收站腾空间』时调用。 |
| `sys_defender_status` | — | 读取 Windows Defender 安全中心状态（只读）：服务状态 + 实时保护开关 + 病毒库/引擎版本 + 最近快速扫描时间。用户问『杀毒软件开没开/Windows 安全中心状态/病毒库新不新』时调用；若系统用第三方杀软（Defender 被替换）会如实说明。 |
| `sys_user_accounts` | — | 枚举本机用户账户（只读）：账户名/全名/是否禁用/是否锁定/本地或域账户/SID 尾段，排除内置系统账户（defaultuser0 等）。用户问『这台电脑有哪些用户/有几个账户/禁用账户是谁』时调用；无需管理员即可读取，受限时如实降级。 |
| `usb_devices` | — | 列出 USB 设备：控制器/集线器/外设 + 厂商/服务/状态（只读，PNP 枚举，普通权限，不读设备内容）。用户问『插了什么 USB 设备/U 盘/键鼠/摄像头识别到没有』时调用。 |

## 硬件域（基准/压测）

| 工具 | 参数 | 说明 |
|---|---|---|
| `bench_cpu` | secs:Option<u64>, threads:Option<usize> | CPU 基准（只读 L0，复刻 AIDA64 CPU Queen/FPU Julia）：多线程整数（质数筛）+ 浮点（π 级数）吞吐测试。参数 secs=每项时长秒（默认 3，上限 30）、threads=线程数（默认全部可用）。 |
| `bench_disk` | path:Option<String>, size_mb:Option<u64>, dry_run:Option<bool> | 磁盘基准（只读 L0 为主，复刻 AIDA64 Disk Benchmark）：顺序/随机读 + 顺序/随机写（MB/s）。参数 path=测试目录（默认系统临时目录，**必须用临时目录或用户指定目录，绝不碰用户数据**）、size_mb=测试文件大小（默认 256，16-2048）、dry_run=只出计划不创建文件（默认 true）。 |
| `bench_gpu` | — | GPU 快照（只读 L0，复刻 AIDA64 GPGPU Benchmark 的只读部分）：显卡型号/显存/驱动/分辨率/温度（N 卡）。深度 GPGPU 基准（像素填充率/OpenCL）需 GPU 厂商 SDK，本工具集未实现，如实标注。 |
| `bench_memory` | secs:Option<u64> | 内存基准（只读 L0，复刻 AIDA64 Cache & Memory Benchmark）：读/写/复制带宽（MB/s）+ 延迟（指针追逐 ns）。参数 secs=每项时长秒（默认 3，上限 20）。 |
| `bsod_analyze` | max_dumps:Option<usize>, include_memory_dmp:Option<bool> | 蓝屏转储分析（只读 L0，复刻 BlueScreenView）：扫描 C:\\Windows\\Minidump\\*.dmp 与根目录 MEMORY.DMP，解析每个转储的崩溃时间 / bugcheck 代码 / 4 参数，映射为中文含义（0x124 WHEA=CPU/内存硬件、0x116 VIDEO_TDR=显卡驱动、0x1A MEMORY_MANAGEMENT=内存等 24 个常见代码）。参数 max_dumps=最多解析几个（默认 10，上限 30）、include_memory_dmp=是否读根目录 MEMORY.DMP（默认 true）。 |
| `hw_acpi` | — | ACPI 固件信息（只读 L0，复刻 AIDA64 ACPI Browser 简化版）：BIOS 厂商/版本/发布日期/序列号 + 机箱厂商/型号/系统类型。完整 ACPI 表浏览（FADT/DSDT 反汇编）需 Ring0 工具，属专业向，这里给到 WMI 可读的固件/电源顶层信息。 |
| `hw_ipmi` | — | IPMI 服务器接口探测（只读 L0，复刻 AIDA64 IPMI）：检测主板是否带 BMC/IPMI 接口（服务器/HEDT 板才有，家用台式机通常无——如实返回未检测到，不编造传感器）。检测到时列出接口实例。 |
| `mem_test` | size_mb:Option<u64>, rounds:Option<u32> | 内存稳定性测试（只读 L0，MemTest86 简化版）：进程内分配 size_mb 缓冲，用 全零/全一/AA55/递增/伪随机 5 种 pattern 反复写入并读回校验，发现位翻转/不稳定内存。参数 size_mb=缓冲大小（默认 256，64-4096）、rounds=轮数（默认 1，1-10）。 |
| `stress_cancel` | — | 停止当前压测（只读 L0 控制信号，无参数）：向正在运行的 stress_test / stress_test_gpu 发送停止信号（进程内 CANCEL flag + 跨进程停止文件，两种通道任一命中即退出），压测循环检测到后立即退出并报告「已按停止信号结束」。前端「停止」按钮 / 用户在 AI 对话里说『停』时调用。 |
| `stress_test` | secs:Option<u64>, max_temp:Option<f64>, threads:Option<usize> | 稳定性压测（**写操作 L2**，复刻 AIDA64 System Stability Test）：CPU 多线程满载压测，期间每 1s 采样 CPU 温度，超过 max_temp 阈值自动熔断停机保护硬件。参数 secs=时长秒（默认 5 演示，上限 600）、max_temp=熔断阈值℃（默认 95，60-110）、threads=线程数（默认全部可用）。 |
| `stress_test_gpu` | secs:Option<u64>, max_temp:Option<f64> | GPU 稳定性压测（**写操作 L1**，复刻 FurMark 的温度熔断守门版）：纯后端无 OpenCL/CUDA/WebGL 引擎无法让 GPU 满载（深度压载需厂商 SDK，同 GPGPU 基准结论），本工具承担温度熔断守门——你正在用前端甜甜圈压测页 / 外挂 FurMark / 游戏 / 渲染任务做 GPU 负载时，本工具在 secs 秒内持续采样 GPU 温度（N 卡 nvidia-smi），超 max_temp 即报熔断信号保护硬件。参数 secs=监控秒数（默认 5，上限 120）、max_temp=熔断阈值℃（默认 90，60-110）。 |

## 硬件域（扩展）

| 工具 | 参数 | 说明 |
|---|---|---|
| `hw_cpu_features` | — | CPU 指令集与特性（只读 L0，复刻 AIDA64 CPUID）：硬件虚拟化/VT-x/AMD-V、SLAT（EPT/NPT）、VMX/SVM，以及运行时探测确认的指令集（SSE2/SSE4.2/AVX/AVX2/FMA/AES-NI）。AI 在用户问『CPU 支持哪些指令集/虚拟化』时调用。 |
| `hw_displays` | — | 显示器信息（只读 L0，复刻 AIDA64 Monitor）：每台显示器的物理尺寸（cm/英寸）/输入类型/激活状态。分辨率与刷新率见 collect_gpu。 |
| `hw_dram_timings` | — | DRAM 时序（只读 L0，复刻 AIDA64 SPD/DRAM Timings）：每根内存条的容量/标称与运行频率/品牌型号 + 频率一致性诊断（XMP/EXPO 是否生效）。完整 CL 时序需 SPD 读取工具（CPU-Z/Thaiphoon），本工具不编造。 |

## 硬件域（报告/自愈）

| 工具 | 参数 | 说明 |
|---|---|---|
| `fan_control` | idx:String, pct:f64, dry_run:Option<bool> | 直接调速风扇（**写操作 L2**，由主项目权限中心确认门守护）：把指定风扇设为软件控制的固定转速百分比。参数 idx=风扇索引（**字符串**，先用 fan_selfheal_diag 查 fans 列表确认，如 "0"）、pct=目标转速 0-100（数字，越界自动钳制）。 |
| `fan_selfheal_diag` | — | 诊断风扇写控通道（只读 L0）：调用 fancmd diag json，返回驱动是否加载/驱动名/RTC 端口验证/SuperIO 芯片与 env_base/风扇列表/通道可写性，并给出明确的可修复点清单（缺驱动？芯片不可识别？可写但需 reset？）。用户在其他机器上发现『风扇控制不可用』时，先调这个拿诊断，再决定是否让用户授权 fan_selfheal_fix 修复。 |
| `fan_selfheal_fix` | action:String, dry_run:Option<bool> | 修复风扇写控通道（**写操作 L2**，由主项目权限中心确认门 + 管理员权限双重守护）。action 可选：install_driver=安装并启动 inpoutx64 端口驱动（需管理员，复制 DLL 到 System32 + sc create/start，绝不覆盖已存在文件）；reset_fan <idx>=恢复指定风扇为主板自动控制（安全可逆，不需管理员）。 |
| `sensor_alert` | cpu_temp:Option<f64>, disk_temp:Option<f64>, gpu_temp:Option<f64>, mem_percent:Option<f64>, disk_free_gb:Option<f64>, persist:Option<bool> | 传感器阈值告警（只读 L0，persist=true 时写应用自有告警日志）：体检 CPU 温度/磁盘温度/GPU 温度/内存占用/磁盘剩余是否超阈值，输出告警清单（只读采集 + 阈值比较，无系统改动）。参数 cpu_temp=CPU 温度阈值℃（默认 85）、disk_temp=磁盘温度阈值℃（默认 55）、gpu_temp=GPU 阈值℃（默认 85）、mem_percent=内存占用阈值%（默认 90）、disk_free_gb=磁盘剩余阈值 GB（默认 20）、persist=是否把告警写历史到 %LOCALAPPDATA%/diskpilot/agent-server/sensor-alerts.jsonl（默认 false，只写应用自有数据目录）。 |
| `sensor_trend` | n:Option<usize>, format:Option<String> | 传感器趋势（只读 L0，写应用自有历史文件）：追加一条当前传感器快照（含 CPU 最高温）到历史 JSONL（%LOCALAPPDATA%/diskpilot/agent-server/sensor-history.jsonl，追加式 + 自动裁剪 500 条——只写应用自己的数据目录，不改不动用户文件），返回最近 n 条（默认 10，上限 50）。format=json 时返回结构化数组供前端折线图。 |
| `system_report` | — | 一键系统体检报告（只读 L0，复刻 AIDA64 Report）：汇总 CPU/GPU/内存/主板/温度/磁盘健康/系统/电池全部只读采集，输出 Markdown 报告。AI 在用户问『体检/电脑状态总结/报告』时调用，拿它当结论依据。 |

## 硬件域（核心）

| 工具 | 参数 | 说明 |
|---|---|---|
| `hw_battery` | — | 读取电池信息：电量百分比/电池健康度/充电状态/循环次数/设计容量（只读，不需要管理员权限；台式机返回『未检测到电池』）。用户问『笔记本电池健康吗/续航/要不要换电池』时调用。 |
| `hw_cpu` | — | 读取 CPU 信息：型号/核心/线程数/基频与当前频率/缓存/实时占用率（只读，WMI，不需要管理员权限）。用户问『CPU 是什么/几个核/占用高不高』时调用。 |
| `hw_disk_smart` | — | 读取磁盘健康（SMART）：每块物理磁盘的容量/接口/健康状态/通电时间/温度/磨损与错误计数（只读，Win10 1607+ 普通权限即可，不需要管理员，不使用已移除的 wmic）。用户问『硬盘健康吗/通电多久/要坏了吗』时调用。 |
| `hw_gpu` | — | 读取显卡信息：型号/显存/驱动版本与日期/分辨率/刷新率，NVIDIA 卡附实时温度与占用（只读，不需要管理员权限）。用户问『什么显卡/显存多大/GPU 占用』时调用。 |
| `hw_memory` | — | 读取内存信息：每根内存条容量/频率/厂商/型号 + 总容量 + XMP/EXPO 是否生效诊断（只读，不需要管理员权限）。用户问『内存多大/频率多少/该不该开 XMP』时调用。 |
| `hw_motherboard` | — | 读取主板/整机信息：主板厂商与型号/BIOS 版本与日期/整机品牌型号（只读，不需要管理员权限）。用户问『这是什么主机/主板是什么』时调用。 |
| `hw_sensors` | — | 读取全量硬件传感器快照（AIDA64 同类）：CPU/GPU/主板温度、风扇转速、电压、功耗、频率、负载，按硬件分组输出（只读）。优先走 HWiNFO 共享内存（普通权限即可读全部传感器，无需管理员/Ring0）；HWiNFO 未运行/未启用共享内存时降级 LibreHardwareMonitor 内核（fancmd sensors），仍不可用再回退 ACPI 热区 + GPU + SMART 通道。 |
| `hw_superio` | — | 自研 SuperIO 直读工具（只读 L0，复刻 HWiNFO/LibreHardwareMonitor 的纯端口直读能力）：动态加载本机已装的端口驱动（inpoutx64 优先，普通权限即可，无需管理员/UAC/Ring0），扫描 0x2E/0x4E 双端口识别 ITE/Nuvoton/Winbond SuperIO 芯片，直读环境控制器寄存器返回主板/CPU 温度 + 风扇转速 + PWM 占空比。HWiNFO 没装/没开共享内存且不想依赖 LHM 时用它兜底 CPU/主板传感器。 |
| `hw_temperature` | — | 读取本机温度：优先走 HWiNFO 共享内存（HWiNFO 后台运行时以普通权限发布 CPU/GPU/主板/磁盘全部温度，无需管理员）；HWiNFO 未运行时按顺序尝试自研 SuperIO 直读（inpoutx64 驱动已装则普通权限直读主板 CPU/主板温度，无需 HWiNFO/LHM）、ACPI 热区 + NVIDIA GPU 实时温度 + 磁盘 SMART（只读，传感器读不到会明确说明，不会假装有数据）。用户问『电脑温度/烫不烫』时调用。 |

## 磁盘域

| 工具 | 参数 | 说明 |
|---|---|---|
| `disk_find_biggest_files` | path:String, top_n:Option<usize> | 列出某目录下最大的文件（递归，只读）。安全边界：盘根/系统目录/主目录根会被拒绝。 |
| `disk_find_duplicate_files` | path:String, top_n:Option<usize> | 找出某目录下疑似重复的文件（递归，按大小分组并按头部 64KB 哈希比对；只读。只比对头部 64KB，不是全文件哈希）。 |
| `disk_health` | — | 列出本机所有磁盘的总/已用/可用空间与使用率（只读，实时读取）。用户问磁盘空间/哪块盘快满了/该清哪里之前先调用。 |
| `disk_io_usage` | sample_ms:Option<u64> | 读取磁盘实时 IO 使用率：每块盘的活动时间百分比与读写速率（只读，需要跑一次 PowerShell 采样，约 1 秒）。用户问『磁盘是不是满了/卡了/谁在读写盘』时调用。 |
| `disk_partition_usage` | path:String | 返回某路径所在卷（分区）的用量：总/已用/可用字节与使用率（只读）。比 disk_health 更细粒度——同一盘上不同挂载点用量可能不同。 |
| `disk_smart_raw_attributes` | — | 读取磁盘可靠性计数器明细：每块物理盘的通电时长/温度/磨损/累计读写错误/启停循环（只读，普通权限）。注意：这是 Get-StorageReliabilityCounter 可靠性计数，**不是**标准 SMART 属性表（ID 0x05 重分配扇区等需 smartctl，本工具不装）。 |
| `disk_top_directories` | path:String, top_n:Option<usize> | 统计某目录的**直接子目录**占用 Top N（含各自全部子级大小，只读，单遍并行遍历）。用户问『哪个文件夹占了空间/什么目录最大』时比单文件视角更实用。 |
| `disk_volume_meta` | — | 列出每个分区的类型元数据（只读）：本地磁盘/可移动/网络/光驱 + 文件系统（NTFS/exFAT/FAT32）+ 卷标。AI 回答『这个盘是什么盘/能不能插拔』时用。 |
| `file_tree` | path:String, max_depth:Option<usize>, max_nodes:Option<usize> | 列出目录树（深度限制，带文件大小；只读）。AI 想一眼看清『某目录下结构长什么样』时用，比 list_dir 更宏观。 |
| `file_type_stats` | path:String, top_n:Option<usize> | 递归统计某目录下按扩展名聚合的文件数与占用字节（只读）。AI 分析『什么文件占了空间』时用：返回 .mp4/.dll/.log 等各占多少。 |

## 系统/文件/进程域

| 工具 | 参数 | 说明 |
|---|---|---|
| `file_recycle` | path:String | 把单个文件/目录移入系统回收站（**写操作，可逆**）。安全守卫：盘根 / 系统目录 / 用户主目录根会被拒绝。 |
| `find_files` | root:String, keyword:String, max_hits:Option<usize> | 在指定目录下按文件名关键字递归查找文件/目录（只读，大小写不敏感，带深度/数量上限）。AI 找『某个文件在哪』时用。 |
| `list_dir` | path:String, limit:Option<usize> | 列出某目录的直接子项（不递归）：名称/是否目录/文件大小。AI 核实『目录里到底有什么』时用。 |
| `list_processes` | top_n:Option<usize> | 列出本机进程（只读）：PID/名称/内存占用/可执行文件路径，按内存占用降序。用户问『有什么程序在跑/哪个进程占内存/某进程存在吗』时调用。 |
| `process_info` | pid:u32 | 查询单个进程的详情（只读）：PID/名称/内存/可执行文件路径/父进程 PID。AI 回答『这个进程是什么/谁拉起的』时用，找不到返回提示。 |
| `process_kill` | pid:u32 | 结束指定 PID 的进程（**写操作**）。安全守卫：系统关键进程（PID < 5）、系统目录中的进程会被拒绝。 |
| `process_start` | command:String, args:Option<Vec<String>>, cwd:Option<String> | 启动一个已安装程序（**写操作，白名单执行**）。安全守卫：只允许系统目录或已安装程序目录（System32 / Program Files / LocalAppData 等）下的 .exe，命令必须存在，参数原样传入（不拼 shell 字符串）。 |
| `read_file` | path:String | 读取文本文件内容（只读，UTF-8/UTF-16 自动识别，超 256KB 截断）。AI 核实某个配置文件/脚本/日志内容时用。 |
| `scheduled_task_manage` | name:String, action:String, dry_run:Option<bool> | 管理计划任务（**写操作，仅可逆动作**）：action 取 enable（启用）/ disable（禁用）/ query（只读查询当前状态）。安全守卫：系统内置任务（任务路径在 \\Microsoft\\ / \\Windows\\ / \\System32\\ 下）一律拒绝修改（query 仍放行）；不开放 delete / run；目标任务不存在则拒绝。 |
| `system_info` | — | 读取本机系统信息与实时状态：系统版本/CPU 逻辑核心/内存总量与使用率/开机时长（只读）。用户问『电脑什么配置/现在卡不卡/帮我看看电脑状态』时调用。 |

## 网络/盘点域

| 工具 | 参数 | 说明 |
|---|---|---|
| `cleanup_suggestions` | root:String, top_n:Option<usize> | 按软件分类给出某目录（**必须传盘根，如 C:\\**）的可清理建议：统计各已知软件（scaffold）的缓存/临时目录占用（只读，单遍并行遍历 + glob 匹配）。**只出建议，绝不删除任何文件**（执行需用户确认走主项目清理流程）。 |
| `net_adapter_detail` | — | 读取每张网卡的详细配置：状态/速率/逐 IP 地址（IPv4 带子网掩码、IPv6 带前缀长度）/MAC 地址/网关/DNS 服务器（只读，不需要管理员权限）。用户问『我的 IP 是什么/子网掩码是多少/MAC 地址/网关 DNS』时调用。 |
| `net_connections` | pid:Option<u32>, keyword:Option<String>, top_n:Option<usize> | 读取网络连接列表：本地/远端地址与端口/状态/所属进程（只读，不需要管理员权限）。默认只看已建立的连接；可按 PID 或关键字（进程名/IP）过滤，也可查看全部。 |
| `net_share` | — | 读取本机共享文件夹（SMB 共享）列表：共享名/路径/权限类型（只读；会话/打开文件统计需要管理员权限，非管理员会明确降级说明）。用户问『电脑共享了什么文件夹/开了哪些共享』时调用。 |
| `net_speed` | interval_ms:Option<u64> | 实测网络实时速率：采集 interval_ms 毫秒（1000-10000，默认 2000）窗口内的收发流量并换算为 KB/s（只读，需要跑一次 PowerShell 采样，约 2-10 秒）。用户问『当前网速快不快/下载多少兆』时调用。 |
| `net_status` | — | 读取本机网络状态：每个网卡名称/连接状态/IP/链接速率/是否虚拟（只读，不需要管理员权限）。用户问『电脑联网了吗/网卡是什么/IP 多少/网速多少』时调用。 |
| `net_wifi` | — | 读取无线网络信息：本机无线网卡状态 + 已保存的 WiFi 网络名称列表（绝不读取密码，只读）。用户问『WiFi 连的是什么/保存了哪些 WiFi』时调用；无无线网卡会明确说明。 |
| `process_cpu_usage` | — | 读取进程 CPU 占用 Top 30（同步采样窗口 200ms：两次 GetProcessTimes 差值，只读）。占用率为单核百分比（多核满载 = 核数 × 100%）。 |
| `security_event_logs` | logs:Option<String>, hours:Option<u32>, max_events:Option<usize> | 读取系统事件日志：指定日志名（逗号分隔，默认 System）、最近 hours 小时、最多 max_events 条（只读）。安全日志无管理员权限会降级说明。 |
| `security_firewall_rules` | direction:Option<String>, action:Option<String>, top_n:Option<usize> | 列出防火墙规则：方向（默认 Inbound 入站）与动作（默认 Allow 允许）过滤，最多 top_n 条（只读，不需要管理员权限）。用户问『防火墙开了什么/某程序被放行了吗』时调用。 |
| `security_login_events` | max_events:Option<usize> | 读取最近的登录/注销事件（安全日志 4624/4625 + PowerShell 脚本块日志 4104，只读）。需要管理员权限才读得到安全日志，无权限会明确降级。 |
| `steam_games` | top_n:Option<usize> | 盘点本机 Steam 游戏库：库根/每库游戏数/总占用 + 各游戏（appid/名称/安装目录/大小/上次游玩/是否幽灵安装/是否建议清理及原因）+ 卸载残留（有目录无 ACF，带完整路径与大小）。只读（注册表定位 Steam + 读 ACF 清单）。 |
| `toolbelt_list` | — | 列出图吧工具箱集成工具清单（只读 L0）：工具名/分类/CLI|GUI/权限级/风险/用途。AI 判断『这个场景该用工具箱哪个工具』时调用，或先于 toolbelt_run 查询可用工具。 |
| `toolbelt_run` | tool:String, args:Option<Vec<String>>, confirmed:Option<bool>, timeout_secs:Option<u64> | 执行图吧工具箱的 CLI 工具（**写操作 L2**，三层防线：L3 永久禁止如 FPT64 刷 BIOS；medium/high 风险必须 confirmed=true 用户确认；cli.run 权限未开启拒绝）。参数 tool=工具名（先 toolbelt_list 确认名称，如 crystaldiskinfo/wiztree）、args=参数列表（透传不拼 shell，空则用工具默认）、confirmed=用户已确认（medium+ 风险必须 true）、timeout_secs=超时秒（默认 30）。 |

## 软件域

| 工具 | 参数 | 说明 |
|---|---|---|
| `app_licenses` | — | 查询软件许可证：Windows 激活状态（slmgr /xpr）+ Office 激活状态（只读，不修改任何系统状态）。用户问『这个系统正版吗/激活了吗/Office 什么版本』时调用。 |
| `app_list` | keyword:Option<String>, limit:Option<usize> | 列出本机已安装程序（只读，来自注册表 Uninstall 键）：名称/版本/发布者/安装位置/预估大小。AI 回答『这台电脑装了什么软件/这个软件能卸载吗』时调用。 |
| `uninstall_app` | name:String | 卸载已安装程序（**写操作**，启动其官方卸载器）。安全守卫：只允许已安装程序目录（Program Files / LocalAppData 等）下 UninstallString 指向的卸载器，卸载参数只允许静默白名单（/S /silent /quiet /qn 等），拒绝 runas/delete 等危险参数。 |

---

## 完整描述速查

### `app_licenses`

查询软件许可证：Windows 激活状态（slmgr /xpr）+ Office 激活状态（只读，不修改任何系统状态）。用户问『这个系统正版吗/激活了吗/Office 什么版本』时调用。Windows 激活查询可能需要管理员权限，无权限时如实返回。

### `app_list`

列出本机已安装程序（只读，来自注册表 Uninstall 键）：名称/版本/发布者/安装位置/预估大小。AI 回答『这台电脑装了什么软件/这个软件能卸载吗』时调用。

### `bench_cpu`

CPU 基准（只读 L0，复刻 AIDA64 CPU Queen/FPU Julia）：多线程整数（质数筛）+ 浮点（π 级数）吞吐测试。参数 secs=每项时长秒（默认 3，上限 30）、threads=线程数（默认全部可用）。纯计算不写盘，几秒出结果。AI 在用户问『CPU 性能/跑分』时调用。

### `bench_disk`

磁盘基准（只读 L0 为主，复刻 AIDA64 Disk Benchmark）：顺序/随机读 + 顺序/随机写（MB/s）。参数 path=测试目录（默认系统临时目录，**必须用临时目录或用户指定目录，绝不碰用户数据**）、size_mb=测试文件大小（默认 256，16-2048）、dry_run=只出计划不创建文件（默认 true）。dry_run=true 先给 AI 看计划；用户同意后再传 dry_run=false 真正跑。测完临时文件立即删除。AI 在用户问『磁盘速度/读写性能』时调用。

### `bench_gpu`

GPU 快照（只读 L0，复刻 AIDA64 GPGPU Benchmark 的只读部分）：显卡型号/显存/驱动/分辨率/温度（N 卡）。深度 GPGPU 基准（像素填充率/OpenCL）需 GPU 厂商 SDK，本工具集未实现，如实标注。AI 在用户问『显卡参数/驱动/温度』或要求 GPU 基准时先调用它。

### `bench_memory`

内存基准（只读 L0，复刻 AIDA64 Cache & Memory Benchmark）：读/写/复制带宽（MB/s）+ 延迟（指针追逐 ns）。参数 secs=每项时长秒（默认 3，上限 20）。纯只读（写基准写自身堆内存），几秒出结果。AI 在用户问『内存性能/带宽/延迟』时调用。

### `bsod_analyze`

蓝屏转储分析（只读 L0，复刻 BlueScreenView）：扫描 C:\\Windows\\Minidump\\*.dmp 与根目录 MEMORY.DMP，解析每个转储的崩溃时间 / bugcheck 代码 / 4 参数，映射为中文含义（0x124 WHEA=CPU/内存硬件、0x116 VIDEO_TDR=显卡驱动、0x1A MEMORY_MANAGEMENT=内存等 24 个常见代码）。参数 max_dumps=最多解析几个（默认 10，上限 30）、include_memory_dmp=是否读根目录 MEMORY.DMP（默认 true）。无转储时返回『未记录到蓝屏』正面结论。用于『为什么蓝屏了/蓝屏代码是什么意思』场景。纯只读不改删文件。

### `cleanup_suggestions`

按软件分类给出某目录（**必须传盘根，如 C:\\**）的可清理建议：统计各已知软件（scaffold）的缓存/临时目录占用（只读，单遍并行遍历 + glob 匹配）。**只出建议，绝不删除任何文件**（执行需用户确认走主项目清理流程）。用户问『C 盘/某盘有什么可清的/微信 QQ 缓存占了多少』时调用；未命中已知软件会明确说明。

### `disk_find_biggest_files`

列出某目录下最大的文件（递归，只读）。安全边界：盘根/系统目录/主目录根会被拒绝。用户问『什么文件占了空间/哪个文件最大』时调用。

### `disk_find_duplicate_files`

找出某目录下疑似重复的文件（递归，按大小分组并按头部 64KB 哈希比对；只读。只比对头部 64KB，不是全文件哈希）。安全边界：盘根/系统目录/主目录根会被拒绝。用户问『哪些文件重复了/有什么可删的重复文件』时调用。

### `disk_health`

列出本机所有磁盘的总/已用/可用空间与使用率（只读，实时读取）。用户问磁盘空间/哪块盘快满了/该清哪里之前先调用。

### `disk_io_usage`

读取磁盘实时 IO 使用率：每块盘的活动时间百分比与读写速率（只读，需要跑一次 PowerShell 采样，约 1 秒）。用户问『磁盘是不是满了/卡了/谁在读写盘』时调用。

### `disk_partition_usage`

返回某路径所在卷（分区）的用量：总/已用/可用字节与使用率（只读）。比 disk_health 更细粒度——同一盘上不同挂载点用量可能不同。

### `disk_smart_raw_attributes`

读取磁盘可靠性计数器明细：每块物理盘的通电时长/温度/磨损/累计读写错误/启停循环（只读，普通权限）。注意：这是 Get-StorageReliabilityCounter 可靠性计数，**不是**标准 SMART 属性表（ID 0x05 重分配扇区等需 smartctl，本工具不装）。用户问『硬盘具体磨损多少/通电多久的明细』时调用。

### `disk_top_directories`

统计某目录的**直接子目录**占用 Top N（含各自全部子级大小，只读，单遍并行遍历）。用户问『哪个文件夹占了空间/什么目录最大』时比单文件视角更实用。安全边界：盘根/系统目录/主目录根会被拒绝。

### `disk_volume_meta`

列出每个分区的类型元数据（只读）：本地磁盘/可移动/网络/光驱 + 文件系统（NTFS/exFAT/FAT32）+ 卷标。AI 回答『这个盘是什么盘/能不能插拔』时用。

### `env_vars`

读取环境变量值（只读）：传 name 查指定变量；不传列出常用子集（PATH/TEMP/USERPROFILE/APPDATA 等）。用户问『某个路径在哪/环境变量配没配/命令找不到是不是 PATH 问题』时调用。

### `fan_control`

直接调速风扇（**写操作 L2**，由主项目权限中心确认门守护）：把指定风扇设为软件控制的固定转速百分比。参数 idx=风扇索引（**字符串**，先用 fan_selfheal_diag 查 fans 列表确认，如 "0"）、pct=目标转速 0-100（数字，越界自动钳制）。执行前自动 diag 校验写通道（驱动未加载/芯片不可识别时拒绝）。dry_run=true 只出计划不执行。软件接管后可随时用 fan_selfheal_fix action=reset_fan <idx> 恢复主板自动控制。AI 应先 fan_selfheal_diag 看可写性与风扇列表，dry-run 给用户确认后再传 dry_run=false 真正调速。

### `fan_selfheal_diag`

诊断风扇写控通道（只读 L0）：调用 fancmd diag json，返回驱动是否加载/驱动名/RTC 端口验证/SuperIO 芯片与 env_base/风扇列表/通道可写性，并给出明确的可修复点清单（缺驱动？芯片不可识别？可写但需 reset？）。用户在其他机器上发现『风扇控制不可用』时，先调这个拿诊断，再决定是否让用户授权 fan_selfheal_fix 修复。

### `fan_selfheal_fix`

修复风扇写控通道（**写操作 L2**，由主项目权限中心确认门 + 管理员权限双重守护）。action 可选：install_driver=安装并启动 inpoutx64 端口驱动（需管理员，复制 DLL 到 System32 + sc create/start，绝不覆盖已存在文件）；reset_fan <idx>=恢复指定风扇为主板自动控制（安全可逆，不需管理员）。dry_run=true 只出计划不执行。AI 应先跑 fan_selfheal_diag 确认根因，再用本工具修复，并明确告知用户要做的操作、请用户确认。

### `file_recycle`

把单个文件/目录移入系统回收站（**写操作，可逆**）。安全守卫：盘根 / 系统目录 / 用户主目录根会被拒绝。**此工具会改变系统状态，主项目侧会先弹确认门，用户确认后才真正执行**。AI 在用户明确要求『把某个文件/文件夹扔进回收站』（如确认是垃圾的临时文件、重复文件）时调用；只进回收站绝不直接删除，误删可从系统回收站还原。

### `file_tree`

列出目录树（深度限制，带文件大小；只读）。AI 想一眼看清『某目录下结构长什么样』时用，比 list_dir 更宏观。安全边界：盘根/系统目录/主目录根会被拒绝。

### `file_type_stats`

递归统计某目录下按扩展名聚合的文件数与占用字节（只读）。AI 分析『什么文件占了空间』时用：返回 .mp4/.dll/.log 等各占多少。带硬上限（20 万文件/20 层深度），超限会标记 truncated。

### `find_files`

在指定目录下按文件名关键字递归查找文件/目录（只读，大小写不敏感，带深度/数量上限）。AI 找『某个文件在哪』时用。安全边界：盘根/系统目录/主目录根会被拒绝。

### `hw_acpi`

ACPI 固件信息（只读 L0，复刻 AIDA64 ACPI Browser 简化版）：BIOS 厂商/版本/发布日期/序列号 + 机箱厂商/型号/系统类型。完整 ACPI 表浏览（FADT/DSDT 反汇编）需 Ring0 工具，属专业向，这里给到 WMI 可读的固件/电源顶层信息。AI 在用户问『BIOS 版本/主板固件/ACPI』时调用。

### `hw_battery`

读取电池信息：电量百分比/电池健康度/充电状态/循环次数/设计容量（只读，不需要管理员权限；台式机返回『未检测到电池』）。用户问『笔记本电池健康吗/续航/要不要换电池』时调用。

### `hw_cpu`

读取 CPU 信息：型号/核心/线程数/基频与当前频率/缓存/实时占用率（只读，WMI，不需要管理员权限）。用户问『CPU 是什么/几个核/占用高不高』时调用。

### `hw_cpu_features`

CPU 指令集与特性（只读 L0，复刻 AIDA64 CPUID）：硬件虚拟化/VT-x/AMD-V、SLAT（EPT/NPT）、VMX/SVM，以及运行时探测确认的指令集（SSE2/SSE4.2/AVX/AVX2/FMA/AES-NI）。AI 在用户问『CPU 支持哪些指令集/虚拟化』时调用。

### `hw_disk_smart`

读取磁盘健康（SMART）：每块物理磁盘的容量/接口/健康状态/通电时间/温度/磨损与错误计数（只读，Win10 1607+ 普通权限即可，不需要管理员，不使用已移除的 wmic）。用户问『硬盘健康吗/通电多久/要坏了吗』时调用。

### `hw_displays`

显示器信息（只读 L0，复刻 AIDA64 Monitor）：每台显示器的物理尺寸（cm/英寸）/输入类型/激活状态。分辨率与刷新率见 collect_gpu。AI 在用户问『显示器型号/尺寸/支持什么输入』时调用。

### `hw_dram_timings`

DRAM 时序（只读 L0，复刻 AIDA64 SPD/DRAM Timings）：每根内存条的容量/标称与运行频率/品牌型号 + 频率一致性诊断（XMP/EXPO 是否生效）。完整 CL 时序需 SPD 读取工具（CPU-Z/Thaiphoon），本工具不编造。AI 在用户问『内存频率/时序/XMP 生效没』时调用。

### `hw_gpu`

读取显卡信息：型号/显存/驱动版本与日期/分辨率/刷新率，NVIDIA 卡附实时温度与占用（只读，不需要管理员权限）。用户问『什么显卡/显存多大/GPU 占用』时调用。

### `hw_ipmi`

IPMI 服务器接口探测（只读 L0，复刻 AIDA64 IPMI）：检测主板是否带 BMC/IPMI 接口（服务器/HEDT 板才有，家用台式机通常无——如实返回未检测到，不编造传感器）。检测到时列出接口实例。AI 在用户问『这板子支不支持 IPMI/远程管理』时调用。

### `hw_memory`

读取内存信息：每根内存条容量/频率/厂商/型号 + 总容量 + XMP/EXPO 是否生效诊断（只读，不需要管理员权限）。用户问『内存多大/频率多少/该不该开 XMP』时调用。

### `hw_motherboard`

读取主板/整机信息：主板厂商与型号/BIOS 版本与日期/整机品牌型号（只读，不需要管理员权限）。用户问『这是什么主机/主板是什么』时调用。

### `hw_sensors`

读取全量硬件传感器快照（AIDA64 同类）：CPU/GPU/主板温度、风扇转速、电压、功耗、频率、负载，按硬件分组输出（只读）。优先走 HWiNFO 共享内存（普通权限即可读全部传感器，无需管理员/Ring0）；HWiNFO 未运行/未启用共享内存时降级 LibreHardwareMonitor 内核（fancmd sensors），仍不可用再回退 ACPI 热区 + GPU + SMART 通道。用户问『温度/风扇/电压/功耗/整机健康』时调用，比 hw_temperature 更全。

### `hw_superio`

自研 SuperIO 直读工具（只读 L0，复刻 HWiNFO/LibreHardwareMonitor 的纯端口直读能力）：动态加载本机已装的端口驱动（inpoutx64 优先，普通权限即可，无需管理员/UAC/Ring0），扫描 0x2E/0x4E 双端口识别 ITE/Nuvoton/Winbond SuperIO 芯片，直读环境控制器寄存器返回主板/CPU 温度 + 风扇转速 + PWM 占空比。HWiNFO 没装/没开共享内存且不想依赖 LHM 时用它兜底 CPU/主板传感器。用户问『主板读到的温度/风扇转速是什么』或 HWiNFO 不可用时调用。纯只读，不写任何寄存器、不调速。

### `hw_temperature`

读取本机温度：优先走 HWiNFO 共享内存（HWiNFO 后台运行时以普通权限发布 CPU/GPU/主板/磁盘全部温度，无需管理员）；HWiNFO 未运行时按顺序尝试自研 SuperIO 直读（inpoutx64 驱动已装则普通权限直读主板 CPU/主板温度，无需 HWiNFO/LHM）、ACPI 热区 + NVIDIA GPU 实时温度 + 磁盘 SMART（只读，传感器读不到会明确说明，不会假装有数据）。用户问『电脑温度/烫不烫』时调用。

### `list_dir`

列出某目录的直接子项（不递归）：名称/是否目录/文件大小。AI 核实『目录里到底有什么』时用。安全边界：盘根/系统目录/主目录根会被拒绝。

### `list_processes`

列出本机进程（只读）：PID/名称/内存占用/可执行文件路径，按内存占用降序。用户问『有什么程序在跑/哪个进程占内存/某进程存在吗』时调用。

### `mem_test`

内存稳定性测试（只读 L0，MemTest86 简化版）：进程内分配 size_mb 缓冲，用 全零/全一/AA55/递增/伪随机 5 种 pattern 反复写入并读回校验，发现位翻转/不稳定内存。参数 size_mb=缓冲大小（默认 256，64-4096）、rounds=轮数（默认 1，1-10）。纯进程堆读写，无磁盘副作用。用于『内存条稳不稳/超频是否稳定』场景；注意只能覆盖进程寻址内存，无法替代 pre-boot 版 MemTest86 的硬件寻址层。

### `net_adapter_detail`

读取每张网卡的详细配置：状态/速率/逐 IP 地址（IPv4 带子网掩码、IPv6 带前缀长度）/MAC 地址/网关/DNS 服务器（只读，不需要管理员权限）。用户问『我的 IP 是什么/子网掩码是多少/MAC 地址/网关 DNS』时调用。

### `net_connections`

读取网络连接列表：本地/远端地址与端口/状态/所属进程（只读，不需要管理员权限）。默认只看已建立的连接；可按 PID 或关键字（进程名/IP）过滤，也可查看全部。用户问『谁在连网/哪个程序在联网/某某连接是什么』时调用。

### `net_share`

读取本机共享文件夹（SMB 共享）列表：共享名/路径/权限类型（只读；会话/打开文件统计需要管理员权限，非管理员会明确降级说明）。用户问『电脑共享了什么文件夹/开了哪些共享』时调用。

### `net_speed`

实测网络实时速率：采集 interval_ms 毫秒（1000-10000，默认 2000）窗口内的收发流量并换算为 KB/s（只读，需要跑一次 PowerShell 采样，约 2-10 秒）。用户问『当前网速快不快/下载多少兆』时调用。

### `net_status`

读取本机网络状态：每个网卡名称/连接状态/IP/链接速率/是否虚拟（只读，不需要管理员权限）。用户问『电脑联网了吗/网卡是什么/IP 多少/网速多少』时调用。

### `net_wifi`

读取无线网络信息：本机无线网卡状态 + 已保存的 WiFi 网络名称列表（绝不读取密码，只读）。用户问『WiFi 连的是什么/保存了哪些 WiFi』时调用；无无线网卡会明确说明。

### `process_cpu_usage`

读取进程 CPU 占用 Top 30（同步采样窗口 200ms：两次 GetProcessTimes 差值，只读）。占用率为单核百分比（多核满载 = 核数 × 100%）。用户问『哪个进程吃 CPU/为什么卡/谁在烧 CPU』时调用，与 list_processes（内存视角）互补。

### `process_info`

查询单个进程的详情（只读）：PID/名称/内存/可执行文件路径/父进程 PID。AI 回答『这个进程是什么/谁拉起的』时用，找不到返回提示。

### `process_kill`

结束指定 PID 的进程（**写操作**）。安全守卫：系统关键进程（PID < 5）、系统目录中的进程会被拒绝。**此工具会改变系统状态，主项目侧会先弹确认门，用户确认后才真正执行**。AI 在用户明确要求『把某进程/某程序结束掉』时才调用；结束后建议用 list_processes 复查。

### `process_start`

启动一个已安装程序（**写操作，白名单执行**）。安全守卫：只允许系统目录或已安装程序目录（System32 / Program Files / LocalAppData 等）下的 .exe，命令必须存在，参数原样传入（不拼 shell 字符串）。**此工具会改变系统状态，主项目侧会先弹确认门，用户确认后才真正执行**。用户明确要求『打开/启动某程序』（如记事本、计算器、某已装软件）时调用；启动即返回（不等待进程退出），结束后可用 list_processes 复查。

### `read_file`

读取文本文件内容（只读，UTF-8/UTF-16 自动识别，超 256KB 截断）。AI 核实某个配置文件/脚本/日志内容时用。只接受文件，拒绝目录。

### `recycle_bin_stats`

查询本机回收站状态（只读，Windows API）：逐卷文件数与总大小 + 合计。用户问『回收站占了多少空间/回收站里有多少东西/能不能清回收站腾空间』时调用。只读状态，不执行清空。

### `scheduled_task_manage`

管理计划任务（**写操作，仅可逆动作**）：action 取 enable（启用）/ disable（禁用）/ query（只读查询当前状态）。安全守卫：系统内置任务（任务路径在 \\Microsoft\\ / \\Windows\\ / \\System32\\ 下）一律拒绝修改（query 仍放行）；不开放 delete / run；目标任务不存在则拒绝。**此工具会改变系统状态，主项目侧会先弹确认门，用户确认后才真正执行**。用户明确要求『禁用/启用某个计划任务』时调用；dry_run=true 只出计划不执行。

### `security_event_logs`

读取系统事件日志：指定日志名（逗号分隔，默认 System）、最近 hours 小时、最多 max_events 条（只读）。安全日志无管理员权限会降级说明。用户问『最近系统出了什么错误/蓝屏/警告/日志』时调用。

### `security_firewall_rules`

列出防火墙规则：方向（默认 Inbound 入站）与动作（默认 Allow 允许）过滤，最多 top_n 条（只读，不需要管理员权限）。用户问『防火墙开了什么/某程序被放行了吗』时调用。

### `security_login_events`

读取最近的登录/注销事件（安全日志 4624/4625 + PowerShell 脚本块日志 4104，只读）。需要管理员权限才读得到安全日志，无权限会明确降级。用户问『谁登录过这台电脑/有没有异常登录/有没有人跑过脚本』时调用。

### `sensor_alert`

传感器阈值告警（只读 L0，persist=true 时写应用自有告警日志）：体检 CPU 温度/磁盘温度/GPU 温度/内存占用/磁盘剩余是否超阈值，输出告警清单（只读采集 + 阈值比较，无系统改动）。参数 cpu_temp=CPU 温度阈值℃（默认 85）、disk_temp=磁盘温度阈值℃（默认 55）、gpu_temp=GPU 阈值℃（默认 85）、mem_percent=内存占用阈值%（默认 90）、disk_free_gb=磁盘剩余阈值 GB（默认 20）、persist=是否把告警写历史到 %LOCALAPPDATA%/diskpilot/agent-server/sensor-alerts.jsonl（默认 false，只写应用自有数据目录）。AI 在用户问『电脑状态健康吗/该注意什么』时调用。

### `sensor_trend`

传感器趋势（只读 L0，写应用自有历史文件）：追加一条当前传感器快照（含 CPU 最高温）到历史 JSONL（%LOCALAPPDATA%/diskpilot/agent-server/sensor-history.jsonl，追加式 + 自动裁剪 500 条——只写应用自己的数据目录，不改不动用户文件），返回最近 n 条（默认 10，上限 50）。format=json 时返回结构化数组供前端折线图。AI 在用户问『温度趋势/散热表现』时调用（多次调用可见随时间变化）。

### `service_control`

控制 Windows 服务：启动 / 停止 / 重启（**写操作，可逆**）。系统关键服务（内核/系统核心）会被安全守卫拒绝。**此工具会改变系统状态，主项目侧会先弹确认门，用户确认后才真正执行**。用户明确要求『帮我停掉某服务/启动某服务/重启某服务』时调用；参数 action 取 start / stop / restart。

### `steam_games`

盘点本机 Steam 游戏库：库根/每库游戏数/总占用 + 各游戏（appid/名称/安装目录/大小/上次游玩/是否幽灵安装/是否建议清理及原因），以及**卸载残留**（有目录无 ACF：游戏列表里完全看不到的真实占用，每处带完整路径与大小）。只读（注册表定位 Steam + 读 ACF 清单）。用户问『装了哪些 Steam 游戏/哪个游戏占地/Steam 该清谁』『Steam 卸载没删干净吗、有没有残留』时调用；用户明确要求清掉某一处残留时，按给出的路径用 `file_recycle` 移入回收站（写操作，先过用户确认门）。

### `stress_cancel`

停止当前压测（只读 L0 控制信号，无参数）：向正在运行的 stress_test / stress_test_gpu 发送停止信号（进程内 CANCEL flag + 跨进程停止文件，两种通道任一命中即退出），压测循环检测到后立即退出并报告「已按停止信号结束」。前端「停止」按钮 / 用户在 AI 对话里说『停』时调用。无压测运行时调用是安全空操作（返回『当前无压测在跑』）。

### `stress_test`

稳定性压测（**写操作 L2**，复刻 AIDA64 System Stability Test）：CPU 多线程满载压测，期间每 1s 采样 CPU 温度，超过 max_temp 阈值自动熔断停机保护硬件。参数 secs=时长秒（默认 5 演示，上限 600）、max_temp=熔断阈值℃（默认 95，60-110）、threads=线程数（默认全部可用）。纯计算不写盘。**满负荷压测会大幅升温/占 CPU，AI 必须先告知用户风险并等用户确认（主项目确认门），用户同意后才调用**。压测结束时给出峰值温度与是否熔断。

### `stress_test_gpu`

GPU 稳定性压测（**写操作 L1**，复刻 FurMark 的温度熔断守门版）：纯后端无 OpenCL/CUDA/WebGL 引擎无法让 GPU 满载（深度压载需厂商 SDK，同 GPGPU 基准结论），本工具承担温度熔断守门——你正在用前端甜甜圈压测页 / 外挂 FurMark / 游戏 / 渲染任务做 GPU 负载时，本工具在 secs 秒内持续采样 GPU 温度（N 卡 nvidia-smi），超 max_temp 即报熔断信号保护硬件。参数 secs=监控秒数（默认 5，上限 120）、max_temp=熔断阈值℃（默认 90，60-110）。无 N 卡/无 nvidia-smi 时如实降级「无温度护栏」。hw.stress 权限门，需用户确认。

### `sys_boot_items`

列出开机启动项：注册表 Run 键（含策略强制项与 32 位视图）+ 启动文件夹（只读，不删除不禁用），可附带计划任务。僵尸启动项（文件已不存在）会标红提示。用户问『开机自动启动了什么/怎么精简开机项』时调用。

### `sys_defender_status`

读取 Windows Defender 安全中心状态（只读）：服务状态 + 实时保护开关 + 病毒库/引擎版本 + 最近快速扫描时间。用户问『杀毒软件开没开/Windows 安全中心状态/病毒库新不新』时调用；若系统用第三方杀软（Defender 被替换）会如实说明。

### `sys_drivers`

列出驱动（设备驱动）：名称/类型/厂商/驱动日期与版本/设备状态（只读，不需要管理员权限）。可按关键字过滤。用户问『装了什么驱动/某驱动是干嘛的/驱动是不是太旧』时调用。

### `sys_services`

列出 Windows 服务：服务名/显示名/状态/启动类型/进程 PID（只读，不需要管理员权限）。可按关键字过滤、可含已禁用服务——服务启动类型与 ImagePath 直接读注册表，准确度高。用户问『什么服务在跑/某服务是干嘛的/怎么启动方式』时调用。

### `sys_user_accounts`

枚举本机用户账户（只读）：账户名/全名/是否禁用/是否锁定/本地或域账户/SID 尾段，排除内置系统账户（defaultuser0 等）。用户问『这台电脑有哪些用户/有几个账户/禁用账户是谁』时调用；无需管理员即可读取，受限时如实降级。隐私红线：绝不输出任何密码/token 信息。

### `system_info`

读取本机系统信息与实时状态：系统版本/CPU 逻辑核心/内存总量与使用率/开机时长（只读）。用户问『电脑什么配置/现在卡不卡/帮我看看电脑状态』时调用。

### `system_report`

一键系统体检报告（只读 L0，复刻 AIDA64 Report）：汇总 CPU/GPU/内存/主板/温度/磁盘健康/系统/电池全部只读采集，输出 Markdown 报告。AI 在用户问『体检/电脑状态总结/报告』时调用，拿它当结论依据。

### `toolbelt_list`

列出图吧工具箱集成工具清单（只读 L0）：工具名/分类/CLI|GUI/权限级/风险/用途。AI 判断『这个场景该用工具箱哪个工具』时调用，或先于 toolbelt_run 查询可用工具。CLI 工具（A 类）可被 AI 直接执行，GUI 工具（B 类）只能启动提示用户。

### `toolbelt_run`

执行图吧工具箱的 CLI 工具（**写操作 L2**，三层防线：L3 永久禁止如 FPT64 刷 BIOS；medium/high 风险必须 confirmed=true 用户确认；cli.run 权限未开启拒绝）。参数 tool=工具名（先 toolbelt_list 确认名称，如 crystaldiskinfo/wiztree）、args=参数列表（透传不拼 shell，空则用工具默认）、confirmed=用户已确认（medium+ 风险必须 true）、timeout_secs=超时秒（默认 30）。**此工具会启动外部程序，主项目侧会先弹确认门**。AI 在用户明确要求『用 XX 工具跑一下』时调用；只读工具（风险 low 如 CrystalDiskInfo/WizTree）可不确认，写类（风险 medium/high）必须先展示命令给用户确认再传 confirmed=true。

### `uninstall_app`

卸载已安装程序（**写操作**，启动其官方卸载器）。安全守卫：只允许已安装程序目录（Program Files / LocalAppData 等）下 UninstallString 指向的卸载器，卸载参数只允许静默白名单（/S /silent /quiet /qn 等），拒绝 runas/delete 等危险参数。**此工具会改变系统状态，主项目侧会先弹确认门，用户确认后才真正执行**。AI 在用户明确要求『卸载某软件』时先调 app_list 确认名称再调用；卸载器以分离方式启动（有自己界面/进度），完成后建议 app_list 复查。

### `usb_devices`

列出 USB 设备：控制器/集线器/外设 + 厂商/服务/状态（只读，PNP 枚举，普通权限，不读设备内容）。用户问『插了什么 USB 设备/U 盘/键鼠/摄像头识别到没有』时调用。
