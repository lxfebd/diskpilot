# DiskPilot 架构审计：接口-实现梳理与冗余分析

> 日期：2026-10-02 ｜ 范围：`apps/desktop/src`、`apps/desktop/src-tauri/src`、`crates/*`
> 性质：只读梳理（一个接口对应一个实现），**不包含任何代码改动**。
> 结论分级：`[删]` 死代码可清除 ｜ `[并]` 多套实现合并为一条 ｜ `[留]` 刻意保留（说明理由） ｜ `[改]` 架构倒挂建议修正

> **2026-10-02 落地更新**：阶段 1（删 3 死接口+修注释）、阶段 2（normKey×4/driveLetter×2/日期格式化收敛）、阶段 2b（合并 scheduleSizeFetch/fetchSizesNow）、阶段 4（改 bytes_freed 记账）、阶段 6（后台 cached_only=true）**已完成**，前端 tsc + 212 测试 + 后端 cargo check 全绿。阶段 3 评估为有意保留不合并。阶段 5（agent-server 采集收敛）待确认后做。

> **2026-10-03 落地更新（阶段 5 完成）**：
> - **5-2 agent-server 内部去重**：`parallel_walker`×3（files/diskx → tools/mod.rs 共享，disk.rs 的 max_depth(20) 刻意差异保留）、`v_str`×4（hw/net/sec/sys → 共享）、注册表读×2（apps.rs/sys.rs 的 read_str/read_dword 对 → `reg_read_str`/`reg_read_dword` 共享）。原"抽公共 crate"（跨 crate 双实现合流）评估为**不做**——主进程与 agent-server 各自进化中，强行合并是高风险大改，文档 §6 建议"先 (a) 后 (b)"调整为"agent-server 内部先去重 + 加缓存"。
> - **5-3 agent-server TTL 缓存**：常驻进程用 `OnceLock<Mutex<Option<TtlSlot>>>` 对齐主进程三层 TTL——`HW_STATIC_TTL=300s` 套 hw_cpu/hw_memory/hw_motherboard（静态硬件不再每次重跑 PS）、`HW_DISK_TTL=300s` 套 hw_disk_smart。温度**不缓存**（HWiNFO/SuperIO 直读毫秒级，且"烫不烫"要最新值；主进程 10s THERMAL_TTL 是为压测循环防进程风暴，agent-server 无此循环）。错误不缓存，`None`（读不到）也缓存。
> - **回归全绿**：workspace cargo check + clippy（0 警告）+ agent-server release 冒烟 70 OK / 8 ERR（全为预期守卫拒绝）/ 4 SKIP / 0 超时 + 前端 tsc + vitest 212/212。

> **2026-10-03 补全（阶段 5 清单 100% 完成）**：fmt_bytes×6（cleanup/steam/diskx/hw/process/net → mod.rs 公共版，bsod 的 Path 签名版保留，KB 精度统一 1 位）；温度假值过滤收敛为 `hw::thermal_ready`（27.31℃ 标记值排除，两处内联→共享）；SMART 脚本二合一（删 `DISK_SMART_RAW_PS`，raw 复用 `DISK_SMART_PS`）。**过程中发现并修复 TTL 缓存污染 bug**：hw_cpu/hw_memory/hw_motherboard 原共用一个缓存槽，冒烟实锤互相返回错误结果（memory/motherboard 输出 CPU 文本），改每工具独立槽。提交 c47d278。

---

## 〇、先回答用户关心的核心问题

**"一个功能不需要实现多种样式 / 不要冗余兜底 / A-B 能解决就不要拆 A-B-C-D"** —— 全库确有三类"臭抽象"：

| 性质 | 数量级 | 代表 |
|---|---|---|
| 死接口（导出但零调用） | 3 处 | `scope_sizes`、`agentApi.ready`、`pluginRegistryInstallVersion` |
| 同一能力多套实现 | ~12 处 | 字节格式化 ×7、`normKey` ×4、日期格式化 ×5、树 DFS ×3、PowerShell 执行器 ×2（tauri+agent） |
| 数据在层级间重复传输/重算 | ~8 处 | 真删用预估字节记账、同一棵树被 8+ 组件整树 DFS、agent-server 无缓存每次重跑 PS |

**架构层面最不合逻辑的一处**：`agent-server`（独立 MCP 服务）与 Tauri 主进程**各自维护了一整套平行的硬件采集/磁盘扫描/PowerShell 执行代码**，两边对同一物理事实各读一遍、各自修同一批 bug（如 ACPI 温度 27.31℃ 假值过滤、ADL GPU 脚本逐字节相同）。根源是 agent-server "每次调用重新 spawn、无共享状态"（`agent.rs:28`），导致它无法复用主进程的 `scan_tree`/`hw_info_cached` 三层 TTL 缓存，只能全量重算。

---

## 一、实际功能接口总表

### 1.1 前端调用面（104 个方法，聚合在 `api.ts`，是唯一后端入口）

- `api/scan.ts`（6）：`scan_path_usn`/`scan_path`/`cancel_scan`/`tree_subtree`/`volume_info`/`list_drives`/`estimate_size`
- `api/clean.ts`（25）：scaffold 增删/同步、`scope_sizes_batch`、`cleanup_suggestions`、`execute_scope`、`execute_ai_plan`、`list_undo`、`cleanup_preflight`、`undo`、`recycle_paths`、`scan_duplicate_files_cmd`、提醒/周报/空间历史
- `api/ai.ts`（4）：`web_search`/`ai_proxy`/`ai_cancel`/`chat_scan_context`
- `api/system-ext.ts`（58）：Steam、硬件（hw_info/hw_disk_health/hw_run_test/hw_report/风扇）、工具墙、插件市场/注册表/远程、系统工具（启动项/驱动/电源）
- `api/agent.ts`（3）：`agent_list_tools`/`agent_call_tool`/`agent_server_ready`（agent-server MCP 工具集）
- `api/mcp.ts`（8）：用户自定义 MCP 服务器增删改查/测试/调用/审计
- `api.ts` 自身（1）：`webview_heartbeat`

**结论**：前端所有 `invoke()` 全部收敛在 api 层，组件无一直接 `invoke`，分层干净。但 **api.ts:13 注释自称 "68 个 Tauri 命令 / 7 个 api 子模块"，实际 104 方法 / 6 子模块，注释过时**。

### 1.2 后端命令（108 个，注册于 `lib.rs:1272-1379`）

按域分组：扫描(6)、scaffold 管理(8)、清理执行(7)、撤销/预检(4)、重复/体积(3)、提醒(3)、周报(3)、空间/硬件历史(3)、Steam(7)、AI(3)、agent-server(3)、用户 MCP(8)、通用配置(4)、工具墙(13)、插件市场/注册表(16)、更新(1)、硬件(9)、系统探测(2)、启动项(3)、驱动(2)。

**后端共 8 个 crate**：`agent-server`(MCP 常驻服务) `executor` `scaffold` `scaffold-lint` `scanner` `steam-inspector` `toolbelt`。

### 1.3 agent-server 内置 MCP 工具（75 个）

磁盘/文件(13)、系统/进程(11)、硬件(17)、网络(6)、安全(5)、环境/应用/盘点(7)、工具箱/自修复/基准/压测/蓝屏/报告(16)。写工具共 11 个（`agent.rs:184 WRITE_TOOLS`），全部经 `require_confirmed()` 确认门。

---

## 二、死接口（建议删除）

| 接口 | 位置 | 证据 |
|---|---|---|
| `scope_sizes`（单根版） | `cleanup.rs`（tauri） | 全前端只有 `scope_sizes_batch`（clean.ts:54）；`scope_sizes` 是 batch 的单根退化，零调用。**`[删]`** |
| `agentApi.ready` → `agent_server_ready` | `api/agent.ts:33` | 全工程无调用方；组件直接用 `agent_list_tools` 不判断就绪。**`[删]`**（若产品需要"工具集就绪"提示需先回填 UI） |
| `pluginRegistryInstallVersion` | `api/system-ext.ts:238` | 全工程无调用方；UI 只调不带 version 的 `pluginRegistryInstall`。**`[删]`**（"按版本安装"分支不可达） |

> 另：`api.ts:13` 注释与实际不符，属**文档漂移**，建议一并修正 `[并]`。

---

## 三、同一能力多套实现（合并清单）

### 3.1 后端：Tauri 主进程 vs agent-server 的平行重复

这是全库重复度最高的地方，两边各自维护一套，**各自修同一批 bug**。

| 能力 | Tauri 主进程 | agent-server | 判定 |
|---|---|---|---|
| PowerShell 执行器 | `hw.rs:273 ps_capture` | `ps.rs:21 ps_capture` | 逐行一致（ps.rs 自述"移植自 hw.rs，零耦合纯函数"）。**`[并]`** 提到公共 crate 或 `pub(crate)` 复用，入参仅差一个 `timeout` |
| ADL GPU 实时读取脚本 | `hw.rs:204 ADL_PROBE_PS` | `hw.rs:285 ADL_PROBE_PS` | 几乎逐字节相同（agent 注释自述"保持一致"）。**`[并]`** |
| 磁盘 SMART | `hw.rs:445 HW_DISK_PS` | `hw.rs:1991 DISK_SMART_PS` | 都 `Get-PhysicalDisk + Get-StorageReliabilityCounter`。**`[并]`** |
| 温度 27.31℃ 假值过滤 | `hw.rs:644` | 5 条路径各写一遍（`hw.rs:1270`等） | 同一坑、两套修复。**`[并]`** 抽 `thermal_filter` |
| 清理建议 | `cleanup.rs:455`（有 `scan_tree`/`cleanup_cache` 缓存优先） | `cleanup.rs:115`（独立进程无缓存，单遍 walk） | **面上是重复；但要保留**——agent 无法复用主进程 AppState 缓存。**`[留]`**，但应抽公共 `tally_dir_scopes`/glob 匹配（scaffold crate 已复用） |
| 重复文件 | `dups.rs:177 scan_duplicate_files` | `diskx.rs:149 collect_duplicate_files` | 同思路（size 分组+头64KB哈希），agent 缺 `min_size` 参数。**`[并]`** 抽到公共函数 |
| 大文件/Top 目录 | `cleanup.rs:630 chat_scan_context`（从内存树算） | `diskx.rs`（独立 walk） | 独立实现，语义相同。**`[并]`** 统一从 scan_tree 计算 |
| 盘符可用空间 | `lib.rs:603 volume_info` + `system.rs:102` | `disk.rs:16 collect_disks` | 都 `GetDiskFreeSpaceExW`，三处各自实现。**`[并]`** |

**agent-server 内部重复**（独立于 Tauri 之外）：

| 能力 | 重复处 | 判定 |
|---|---|---|
| `fmt_bytes` | `mod.rs:73`(已成 pub)，但 `steam.rs:130`/`cleanup.rs:287`/`diskx.rs:52`/`process.rs:461`/`hw.rs:70`/`bsod.rs:309` 各复刻一份 | **`[并]`** 收敛到 mod.rs 公共函数 |
| `parallel_walker` | `files.rs:465` 与 `diskx.rs:35` 完全相同，`disk.rs:163` 仅差 max_depth | **`[并]`** 提 `pub(crate)`（diskx 自述"是 private 无法复用故复刻"） |
| `v_str` JSON 取值 | `hw.rs:45`/`net.rs:107`/`sec.rs:62`/`sys.rs:108` 实现一致 | **`[并]`** 抽 tools/common |
| 注册表读/枚举 | `apps.rs:28,73` 与 `sys.rs:61,768` | **`[并]`** 统一 NUL 处理/枚举模式 |
| 温度读取口径 | `collect_temperature`/`collect_sensors`/`collect_sensors_fast`/`max_cpu_temp`/`collect_temperature_fast_parse` 五条路径 | **`[并]`** 抽单一 `temp_snapshot` + 公共假值过滤 |
| SMART 三份 PS | `DISK_SMART_PS`/`DISK_SMART_RAW_PS`/`collect_temperature` 内嵌一版 | **`[并]`** 共用一个脚本+结构 |

### 3.2 前端：同一能力多套实现

| 能力 | 重复处 | 判定 |
|---|---|---|
| `normKey` 路径归一化 | `App.tsx:42`/`store.ts:13`/`AssetOverview.tsx:37`/`DriveStrip.tsx:14`（4 处完全相同） | **`[并]`** 提到 `format.ts` |
| `driveLetter` | `AssetOverview.tsx:41`/`DriveStrip.tsx:17` | **`[并]`** 同上 |
| 日期/相对时间格式化 | `cleanup/model.ts:165 formatLastActive` / `WeeklyHealthCard.tsx:17 reportAgeLabel` / `history/model.ts:54+79 dayGroupLabel` / `SteamInspector.tsx:45` 内联 + 约 8 处 `new Date().toLocaleString()` 内联 | **`[并]`** 至少 5 套，收敛到 `format.ts` |
| 按 scaffold 收集节点 DFS | `cleanup/matches.ts:36`（已被 Studio/CleanupPage/store 共用，良好）/ `ChatPanel.tsx:233 findAllScaffoldNodes` / `AssetOverview.tsx:151 aggregateByScaffold` | 三者逻辑一致但各自实现。**`[并]`** 共用 `matches.ts` |
| 清理执行序 | `useCleanupStore.ts:550-649 runRealDelete` / `AssetOverview.tsx:475-541 doClean`（各自遍历→逐个 executeScope→汇总 toast） | **`[并]`** 收敛到一个执行器 |
| `scheduleSizeFetch` / `fetchSizesNow` | `useCleanupStore.ts:118-149` 与 `:152-180` 函数体完全重复，仅差是否防抖 | **`[并]`** 合并为一函数带防抖参数 |
| `formatBytes` / `formatBytesTriple` | `format.ts:1` 与 `:16` 底层单位数组/toFixed 各写一遍 | **`[并]`** 让 triple 复用 bytes |

**注意：`format.ts formatBytes` 是唯一权威（25 个文件 import），无内联重复——这是干净的、可当范本。**

### 3.3 刻意保留的（不要强行合并）

| 项 | 理由 |
|---|---|
| `translate_steam_names`（cache-only） vs `translate_steam_names_fetch`（网络+写缓存） | 两阶段设计：快速读缓存 + 按需网络刷新。可用 `fetch: bool` 参数合并，但产品上"秒开"与"刷新"是两个动作，**`[留]`** |
| `hw_info`（CIM 一次性取全） vs `run_system_probe`（Win32 实时） | 结构/用途不同（历史快照 vs 实时），**`[留]`** |
| `scan_path` vs `scan_path_usn` 共用 `run_full_scan` | 已正确复用，**`[留]`** |
| `tree_subtree`/`chat_scan_context` 读内存 `scan_tree` | 已正确复用避免重走盘，**`[留]`** |

---

## 四、数据在层次间反复传输 / 重算（性能与架构倒挂）

### 4.1 后端重算（agent-server 无缓存）

**核心事实**：`agent.rs:28` 明确 agent-server "每次调用重新 spawn、无共享状态"，因此**任何采集都无法跨调用共享**。而 Tauri 主进程有三层 TTL 缓存（`HW_SNAPSHOT_TTL=300s`/`HW_DISK_TTL=300s`/`THERMAL_TTL=10s`）。

- 每次调 `hw_cpu`/`hw_memory`/`hw_motherboard` 各跑一次 PowerShell/WMI，完全可 5min 缓存静态硬件。
- `system_report`（report.rs:84）一次报告连续调 8 次独立采集，与手动逐个调工具完全重复。
- `sensor_alert`（report.rs:204）单次调用内读最多 3 次温度快照。
- 建议：agent-server 已是常驻进程，用 `OnceLock<Mutex<Option<...>>>` 对齐 Tauri 三层 TTL 缓存（同 `hw.rs:13,23,609` 手法）。**`[改]`**

### 4.2 前端重算（同一棵树被多处整树 DFS）

后端已有能力：`scan_tree`（内存完整树）＋ `tree_subtree`（按需子树）＋ `chat_scan_context`（结构化摘要）。但前端对同一棵树反复 DFS：

| 组件 | 对 root 的动作 |
|---|---|
| `Studio.tsx:47` | 整树 DFS（collectScaffoldCards） |
| `CleanupPage.tsx:27` | 整树 DFS |
| `useCleanupStore.ts:294 select` | 整树 DFS |
| `ChatPanel.tsx:233` | 整树 DFS（findAllScaffoldNodes） |
| `AssetOverview.tsx:151` | 整树 DFS（aggregateByScaffold） |
| `Treemap`/`FileView` | 各自遍历 |

这些都是**引用而非深拷贝**（无内存爆炸），但同一棵树的**派生聚合（按 scaffold 收集节点）被 5 处各自算一遍**。后端 `chat_scan_context`（"给定 scaffold 收集节点"的能力）没有被 TaskOverview/AssetOverview 复用，前端仍自己 aggregate。**`[改]`** 优先让"按 scaffold 收集"走单一前端函数（matches.ts）；后续可给后端加"scaffold 命中"接口让前端免 DFS。

### 4.3 层次倒挂（前端拿不到真值）

- **真删用预估字节记账**：`useCleanupStore.ts:621` `addReclaimed(preview.totalBytes)`——dry-run 已算出每 scope 命中字节，真删时却用预估字节而非后端实际返回。注释承认 UndoEntry 不带实际字节。**`[改]`** 后端 `execute_scope`（dry_run=false）应返回实际回收字节，前端据此记账。
- **同一批路径两次 execute_scope**：dry-run（false）与真删（true）对同一路径各调一次（`useCleanupStore.ts:451-506` 与 `:560-613`）。这是"先预览后执行"的设计使然，合理；但真删阶段若分批删除，期间 `preview.totalBytes` 可能因前置删除而失真。
- **总览页散点健康信号前端拼装**：`AssetOverview.tsx:309-376 HealthCheckupCard` 把 `fanCurveAdvice`/`listStartupItems`/`getSpaceHistory`/`cleanupSuggestions` 各自拉取后在前端拼。后端已有 `generate_weekly_report` 聚合能力，本可一次返回。**`[改]`**（或 `[留]` 若产品需要实时性）。
- **`chat_scan_context` 双通道**：`ChatPanel.tsx:233`（前端 DFS+inspect）与 `:332`（后端 `chat_scan_context`）。非 Tauri 环境走前端 fallback，Tauri 走后端。**前端保留一套完整 fallback，属同一数据组织逻辑前后端各写一遍。** 若产品只跑 Tauri，可删前端 fallback。**`[改]`**
- **插件 manifest 全量重复传输**：`advisor/tools.ts:1350` 拉全量 23 工具 manifest，AI 通道里再 `slice(0,4000)` 截断（`:479`）。后端应缓存/按需返回，避免每轮对话重复传大 JSON。**`[改]`**

### 4.4 后台高频重算的隐患

`reminder.rs:166` / `weekly_report.rs:92` 后台线程对 C..Z 每个盘调用 `cleanup_suggestions(cached_only=false)`——若此前无扫描缓存，会**触发多次全盘 walk**。这是并发全盘扫描的高负载风险，与前端 `cleanupSuggestions(cachedOnly=true)` 的行为不一致。**`[改]`** 后台应强制 `cached_only=true`，未命中则跳过该盘。

---

## 五、结论与重构优先级

### 架构层面最不合理的摆放

**agent-server 与 Tauri 主进程的"平行双实现"是最大架构债务。** 二者对同一物理事实（温度/磁盘/SMART/启动项/已装程序）各读一遍、各修一遍 bug，代码量近似翻倍。根因是 agent-server 的"无共享、每次 spawn"架构使它无法复用主进程缓存与内存树。

**第一性原理化解**：要么 (a) 把底层采集（ps_capture/SMART/温度/ADL）抽到公共 crate，agent-server 与 Tauri 都引用同一份实现；要么 (b) agent-server 改为常驻进程引入与 Tauri 对齐的 TTL 缓存（`OnceLock<Mutex>`），从源头消除重复采集。二者**不必同时做**——(a) 消除代码重复，(b) 消除计算重复，建议先 (a) 后 (b)。

### 建议处理顺序（按性价比）

1. **删死代码**（零风险）：`scope_sizes` / `agentApi.ready` / `pluginRegistryInstallVersion` / 修正 `api.ts:13` 注释。
2. **前端小工具去重**（低风险）：`normKey`×4 / `driveLetter`×2 / 日期格式化×5 收敛到 `format.ts`；`scheduleSizeFetch`/`fetchSizesNow` 合并；`finalBool`。
3. **树聚合去重**（中风险）：按 scaffold 收集节点统一走 `matches.ts`，删 ChatPanel/AssetOverview 各自 DFS。
4. **清理记账修正**（中风险，产品价值高）：后端 `execute_scope` 返回实际清理字节，前端据此记账，消除"预估字节"倒挂。
5. **agent-server 采集收敛**（高风险大改）：抽公共采集 crate / 加 TTL 缓存；`fmt_bytes`/`parallel_walker`/`v_str`/注册表读取/温度口径/SMART 三份脚本统一。
6. **后台全盘 walk 风险**（安全）：reminder/weekly_report 强制 `cached_only=true`。

> 本审计为只读产出。除 1-2 项（删死代码、改注释）外，其余均为多文件重构，涉及并发会话编辑（本工作区有并行会话），**任何实际改动需用户确认后、按"先重读再改"的约定执行**。
