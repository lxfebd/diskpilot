# DESIGN · tools-agent 架构与技术设计

> 交接文档：给合入主项目的那一侧（AI / 维护者）看的设计说明。
> 现状基准：2026-09-09，41 工具冒烟 43/43 PASS（41 工具 + 安全守卫 ×2）。

## 一、为什么做这个（背景）

DiskPilot 主项目内置「图吧工具箱」工具墙：30 个工具 manifest，本质是**本地 exe 的
说明书**——AI 只能识别 manifest 里的参数表，然后 `toolbelt_launch` 把它 GUI 启动给人看。
这是 GUI 时代的产物：工具是人手操作的对象，不是 AI 可调用的能力。

本项目要解决的是：**让 AI 能真正操作这台电脑**。按照第一性原则拆解：

- 工具 = **能力**（读磁盘 / 列进程 / 读文件 / 查系统），不是 exe。
- 同一个能力既可以给 AI 调（MCP `tools/call`），也可以日后挂回主项目工具墙（复用
  TOOL_CATALOG 索引），两头都是消费者。
- **不重复造轮子**：协议层不自己写 JSON-RPC，用官方 Rust MCP SDK（`rmcp` 0.4.1，
  模型上下文协议官方实现）；工具逻辑优先复用主项目 crate 已有纯函数（合入后直接依赖
  `diskpilot-scanner` / `diskpilot-toolbelt`），不复制粘贴。

## 二、技术选型

| 决策 | 选择 | 理由 |
|---|---|---|
| 协议 | MCP（Model Context Protocol） | AI 时代工具调用的事实标准：`tools/list` / `tools/call` 三个标准 channel；Claude / 自研 agent 都能直接接 |
| SDK | `rmcp` 0.4（features: server + schemars + transport-io） | 官方 Rust 实现，`#[tool_router]` / `#[tool_handler]` / `#[tool(name, description)]` 声明式注册工具，自带 JSON Schema 生成（schemars） |
| 传输 | stdio（`rmcp::transport::io::stdio()`） | 最小接入成本：主项目 `Command` spawn 子进程即可，无需端口/权限 |
| Windows API | `windows-sys` 0.59（与主项目同版本） | 零运行时依赖、纯 FFI 绑定；按工具用到的能力开 feature |
| 异步 | tokio + tracing | rmcp 的 ServerHandler 是 async trait；tracing 日志走 stderr，不污染 stdio 协议通道 |

## 三、架构

```
tools-agent/
  Cargo.toml                # 独立 workspace（resolver=2，workspace.dependencies 统一版本）
  docs/DESIGN.md            # 本文件
  docs/TOOL_CATALOG.md      # 工具清单（合入主项目工具墙的索引）
  scripts/smoke_test.py     # stdio 全链路冒烟（MCP 握手 + tools/list + 12 工具 + 守卫拒绝）
  crates/agent-server/      # MCP server（二进制 crate，生成 agent-server.exe）
    src/main.rs             # 入口：tracing 初始化（stderr）→ AgentServer::new().serve(stdio) → waiting()
    src/ps.rs               # PowerShell 执行器：ps_capture / ps_capture_timeout / json_array_of（UTF-8 强注入 + GBK 兜底）
    src/server.rs           # ServerHandler 实现（get_info / capabilities / instructions）占位文件
    src/tools/mod.rs        # #[tool_router] 薄壳：41 个 #[tool] 方法 = 参数 → 纯函数 → 文本结果
    src/tools/disk.rs       # collect_disks / collect_volume_usage / file_type_stats / collect_volume_meta（纯逻辑）
    src/tools/diskx.rs      # collect_biggest_files / collect_duplicate_files / collect_disk_io_usage
    src/tools/system.rs     # os_version（注册表 ProductName，GetVersionExW 兜底）/ memory_info / uptime
    src/tools/files.rs      # PathGuard（盘根/系统目录/主目录根守卫，环境变量推导）+ list/read/find/tree
    src/tools/process.rs    # Toolhelp32 快照列进程 + Psapi 内存 + QueryFullProcessImageNameW 路径（Windows）+ CPU 占用双采样
    src/tools/apps.rs       # 已安装程序枚举（注册表 Uninstall 键 + WOW6432Node + HKCU）
    src/tools/hw.rs         # collect_cpu/gpu/memory/motherboard/temperature/battery/disk_smart/usb_devices/disk_smart_raw（WMI/CIM，不需管理员）
    src/tools/net.rs        # collect_net_status/connections/speed/share/wifi（原生 WinAPI + PS 兜底）
    src/tools/sys.rs        # collect_services/drivers/boot_items（注册表 + PS）
    src/tools/sec.rs        # collect_event_logs/firewall_rules/login_events（PS Get-WinEvent/Get-NetFirewallRule）
    src/tools/steam.rs      # steam_games（复用 diskpilot-steam-inspector 盘点 ACF 游戏库）
    src/tools/cleanup.rs    # cleanup_suggestions（复用 diskpilot-scaffold + 单遍 walk 估算各 scope 可清字节）
```

### 分层原则

- `tools/mod.rs` 只做「参数反序列化 → 守卫检查 → 调纯函数 → 格式化中文文本」，不写系统调用。
- 系统调用全部收在 `disk.rs` / `system.rs` / `files.rs` / `process.rs` 的**纯函数**里，
  参数是普通类型（`&Path` / `usize`），便于日后直接内嵌进主项目复用（不经 stdio）。
- 所有工具返回 `CallToolResult`：成功 `text_result(...)`，守卫/逻辑错误 `tool_error(...)`
  （isError=true + 中文原因），AI 能区分「查到了」和「被拒了」。

## 四、安全模型

继承主项目铁律：**宁可错放 1000GB，不可错删一个文件**。当前 41 个工具全部**只读**，
对应主项目 L0 权限级（恒开无需确认）。两道防线：

1. **PathGuard**（`files.rs`）：`list_dir` / `read_file` / `find_files` / `file_type_stats` / `file_tree`
   的路径参数必须先过守卫——拒绝：
   - 盘根（`C:\` 及任意 `X:\`）
   - 系统目录（`C:\Windows`、`C:\Program Files`、`C:\Program Files (x86)`、`C:\ProgramData` 等系统根**自身**；
     注意**不**递归下探——`SystemRoot\System32\drivers` 这类正常可读路径放行，避免误杀）
   - 用户主目录根（`C:\Users\<name>` 本身，其下子目录可操作）

   守卫判定全部从**环境变量**推导：`SystemRoot` / `windir` / `ProgramFiles` / `ProgramFiles(x86)` /
   `ProgramData` / `USERPROFILE`。换到任意盘符（Windows 装在 D: 而非 C:）也能正确识别，不硬编码盘符。
   
   守卫判定失败返回 `isError=true` +「路径被安全守卫拒绝：…」，测试确认 `list_dir C:\` 被拦。

2. **只读不写**：本阶段没有任何 `delete` / `move` / `write` / 改系统设置的工具。
   写操作留给主项目既有 executor + 权限中心（L1/L2 确认 + recycle 回收站），合入时
   若要新增写工具，必须按「先出清单 + 用户确认 + 默认回收站」三原则做，server 端另加
   操作白名单兜底（禁止盘根 / 系统目录 / 主目录根，与 PathGuard 同语义）。

## 五、冒烟验证（2026-09-09）

`scripts/smoke_test.py` 通过 stdio 管道完整走 MCP 协议：

1. `initialize` 握手（协议协商 2025-03-26，`notifications/initialized`）；
2. `tools/list` 返回 41 个工具；
3. 逐个 `tools/call`：全部 41 个工具（磁盘/文件/进程/程序 + 硬件 9（含 USB / SMART 原始属性）+ 网络 5 + 系统服务/驱动/启动 3 + 安全审计 3 + 磁盘深入 3 + 回收站/环境变量 2 + Steam 游戏库 + 清理建议）返回真实数据；
4. 守卫验证：`list_dir C:\` 返回 `isError=true` + 中文拒绝原因。

**40/40 PASS**（debug + release 双冒烟通过）。回归时重跑：`python scripts/smoke_test.py`。

### 已知坑（本机 rustc 1.98 / Windows 环境）

- **`GetVersionExW` 是陷阱**：Win8.1+ 返回值被兼容层遮蔽（真实 Win11 显示 6.2.9200）。
  展示用版本必须读注册表 `HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion` 的
  `ProductName`（注意路径里有 `Windows NT`，漏掉会打开错误键静默兜底——已修复）；
  注册表失败才兜底 `GetVersionExW` 并保留 NT 版本号。
- **windows-sys 0.59 的 HANDLE 是裸指针**：`CreateToolhelp32Snapshot` 返回
  `isize`，与 `INVALID_HANDLE_VALUE` 比较用指针语义；注册表 `HKEY` 用
  `std::ptr::null_mut()` 初始化。
- **stdin EOF 服务端即退出**：冒烟必须用管道交互，不能 `重定向文件 + 整体读`。
- rmcp 0.4.1 模块位置：`ServerHandler` 在 `rmcp::handler::server`；`stdio` 需要
  `transport-io` feature 并从 `rmcp::transport::io::stdio()` 引入。

## 六、合并方案（主项目侧）

接入两条路（README 已列，细节在这里）：

### 方案 A：独立进程（推荐先行）
`agent-server.exe` 由 DiskPilot Rust 后端用 `std::process::Command` spawn（stdio 管道），
后端用 `rmcp` client 连接。前端 `execTool` / AI 工具调用的协议层换成
`client.call_tool(name, args)`。优点：进程隔离，server 崩溃不影响主进程；权限模型天然
干净（子进程只有继承的句柄）。代价：一次进程启动 + JSON 序列化往返。

### 方案 B：内嵌库
把 `crates/agent-server` 编成 rlib 挂进主 workspace，后端直接持 server 对象调用
`call_tool`（不经 stdio）。优点：少一层 IPC。代价：失去进程隔离；需处理
`#[tool_router]` 宏生成代码与主项目依赖树共存。**两个方案共用同一份 tool 实现**，
合入时优先方案 A，跑通再评估 B。

### 合入步骤建议
1. 把 `tools-agent` 整体并入主仓库（或把 `crates/agent-server` 移入主 `crates/`）；
2. `Cargo.toml` 依赖对齐：rmcp 版本、windows-sys 0.59、tokio/tracing 与主项目
   workspace 合并；
3. 方案 A 落地 `execTool → MCP client`，冒烟脚本的调用方式平移成主项目集成测试；
4. 把 TOOL_CATALOG 表注册进主项目工具墙（「AI 可操作」分区），`disk_health` /
   `system_info` 等可直接复用现有前端展示组件。

## 七、扩展路线（下一批工具候选）

按「AI 问得最多 + 只读安全」优先。**2026-09-09 更新：原路线中的 `file_tree` /
`process_details`（→ `process_info`）/ `app_list` / `disk_smart`（→ `hw_disk_smart`）已实现并冒烟通过**，剩余候选：

1. **进程 CPU 采样**：`GetProcessTimes` 两次快照差值得 CPU 使用率——需要跨调用状态
   （记录上次快照时间与内核/用户时间），与当前无状态纯函数风格冲突，放到合入后由
   主项目状态机持有再补。
2. **`read_file` 结构化增强**：日志尾部（只读最后 N 行）/ JSON/TOML 语法校验——低优先级。
3. **更多硬件细项**：风扇转速 / 电压需 LibreHardwareMonitor 类专用引擎（WMI 拿不到）。
   已补 USB 设备枚举（`Get-PnpDevice -Class USB`）与 SMART 原始属性（`Get-StorageReliabilityCounter`
   补 raw 值；读不到的机型如实降级，不编造）。
   显示器 EDID、外设（USB/蓝牙）枚举——中优先级。
4. **`security_event_logs` 加 `event_id` 过滤**：按事件 ID（如 41 内核电源 / 6008 意外关机）
   精准查——低优先级。

> **disk_smart 解阻说明**（2026-09-09 更正早期结论）：早期认为 `disk_smart` 需要
> `DeviceIoControl` + `STORAGE_DEVICE_DESCRIPTOR` 且被 windows-sys 面阻塞——**错误**。
> Windows 10 1607+ 提供 `Get-StorageReliabilityCounter`（PowerShell，普通权限即可），
> 直接返回通电时长/温度/磨损/错误计数/健康状态，无需管理员、无需 DeviceIoControl、
> 更不能用已从 Win11 24H2 移除的 `wmic`。已据此实现 `hw_disk_smart` 并冒烟通过
> （5 块物理盘：NVMe 健康、错误计数、温度启发式换算均正确）。
> 主项目 `hw.rs` 的 `disk_health`（容量层面）与本工具的 SMART（健康层面）互补，合入后并存。

写操作类（清理/回收站）**不进本目录**：留在主项目 executor + 权限中心，遵守
「先清单 + 确认 + recycle」铁律。
