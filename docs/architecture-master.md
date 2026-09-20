# DiskPilot 架构总设计文档（代码级确认版）

> 本文档是 DiskPilot 的**架构总设计**：以 2026-09-20 代码级确认为准，逐模块说明「是干什么的、目的是什么」，作为后续优化完善修复的**基准**。
> 所有规模数字均来自当日实跑命令（非文档引用）。
> 全仓中文口径：UI 文本 / 文档 / 回复一律中文；代码标识符、路径、术语（`scope`/`glob`/`dry-run`/`recycle`）保留英文。

---

## 0. 项目定位（一句话）

Windows 优先的磁盘清理桌面应用（Tauri 2 + Rust 后端 + React 18 + TypeScript 前端），做三件事：**秒扫整盘**（NTFS MFT 直读 + USN 增量）、**拖文件夹给 AI 问**（BYOK 多模型）、**已知应用走清理脚本**（scaffold 按 scope 进回收站）。

设计口号：**宁可错放 1000GB，不可错删一个文件。**

---

## 1. 顶层架构树（代码级确认）

```
diskpilot_src/
├── apps/desktop/                    桌面应用（前后端一体）
│   ├── src/                         前端：React 18 + TS + Vite
│   │   ├── App.tsx                  根组件：view 状态三页签（overview|workspace|tools）+ 常驻 AI 侧栏
│   │   ├── api.ts                   唯一后端入口聚合（6 个 api/ 子模块再导出）
│   │   ├── api/                     按领域拆的 invoke 封装
│   │   │   ├── scan.ts             扫描（scan_path / scan_path_usn / cancel / tree_subtree / estimate_size）
│   │   │   ├── clean.ts            清理（execute_scope / execute_ai_plan / recycle / undo / suggestions / conda）
│   │   │   ├── ai.ts               AI 对话（ai_proxy / ai_cancel / web_search）
│   │   │   ├── agent.ts            agent-server 桥（list_tools / call_tool / server_ready）
│   │   │   ├── mcp.ts              用户自定义 MCP 服务器 CRUD + 工具调用 + 审计
│   │   │   └── system-ext.ts       系统扩展（toolbelt / 插件市场 / 硬件 / steam / 服务 / 启动项 / 驱动）
│   │   ├── store.ts                 zustand 全局状态
│   │   ├── permissions.ts           22 个 PermId 权限定义 + isPermEnabled
│   │   ├── theme.ts                 17 套主题预设 + 动态 accent + 字体缩放
│   │   ├── components/              28 个组件（详见 §4.2）
│   │   ├── advisor/                 AI 工具链（22 个内置 AI 工具 + 多 provider 客户端 + 聊天历史）
│   │   ├── i18n/                    全应用多语言（15 命名空间 / 2766 键）
│   │   ├── styles/                  8 个 css（tokens/layout/chat/steam/settings/overview/toolwall/dark）
│   │   ├── types.ts / env.ts / mocks.ts / hwCache.ts / hwProfile.ts / tooltree.tsx / format.ts
│   │   └── src-tauri/               Tauri 2 后端（Rust）
│   │       ├── src/lib.rs           1550 行：AppState(16 字段) + 100 条命令注册 + 22 子模块
│   │       ├── src/agent.rs         agent-server MCP 桥（execTool → agent_call_tool，11 个写工具白名单）
│   │       ├── src/ai.rs            AI provider 代理（SSRF 防护 417-460）+ 建议
│   │       ├── src/cleanup.rs       清理建议 / scope 尺寸 / chat 扫描上下文
│   │       ├── src/conda.rs         Conda 环境枚举 / 路径检视 / 资源管理器定位
│   │       ├── src/drivers.rs       驱动列表 / 更新检查
│   │       ├── src/dups.rs          重复文件扫描命令
│   │       ├── src/executor.rs      执行：execute_scope / execute_ai_plan / recycle / undo
│   │       ├── src/hw.rs            硬件面板（HW 信息 / 磁盘健康 / 压测 / 风扇 / 电源计划）
│   │       ├── src/hw_history.rs    硬件快照对比
│   │       ├── src/mcp.rs           用户自定义 MCP 服务器管理 + 工具调用
│   │       ├── src/plugin_registry.rs 插件市场（社区注册表 + 内置去重 + 安装台账）
│   │       ├── src/plugin_remote.rs 远程 URL 安装（https 强制 + Ed25519 校验）
│   │       ├── src/reminder.rs      清理提醒后台线程（只出清单不删）
│   │       ├── src/scaffold_registry.rs scaffold 注册表（列表 / 安装 / 卸载）
│   │       ├── src/scan.rs          扫描（MFT / USN 增量 / walker 回退）
│   │       ├── src/space_history.rs 空间历史
│   │       ├── src/startup.rs       开机启动项（列表 / 设置 / 移除）
│   │       ├── src/steam.rs         Steam 游戏 / 创意工坊
│   │       ├── src/system.rs        系统探测 / 网络探测
│   │       ├── src/toolbelt.rs      图吧工具箱（目录 / 运行 / 插件市场 / 回收）
│   │       └── src/updates.rs       更新检查
│   └── src-tauri/tauri.conf.json    Tauri 配置
├── crates/                          7 个独立 Rust crate（零横向依赖）
│   ├── scanner/                     MFT 直读 / USN 增量 / walker 遍历（扫描核心）
│   ├── scaffold/                    清理脚本 TOML 解析 + 加载（winapp2 兼容）
│   ├── executor/                    执行引擎（回收站 / 隔离区 / 删除 + undo.jsonl）
│   ├── scaffold-lint/               scaffold 红线校验 CLI
│   ├── steam-inspector/             Steam 库盘点（ACF 解析）
│   ├── toolbelt/                    工具墙（30 manifest / 白名单执行 / 签名）
│   └── agent-server/                独立 MCP 进程（75 个工具：64 只读 + 11 写需确认）
├── scaffolds/                       18 份清理脚本 TOML（90 个 scope）+ _templates/
├── docs/                            历史详细设计（ARCHITECTURE/DESIGN/ARCHITECTURE_FOR_AI/tools-agent）+ 本文档
├── AGENTS.md / CLAUDE.md            协作约定
└── ROADMAP.md                       路线图与优先级
```

**关键数字（2026-09-20 实跑）**

| 项 | 值 | 来源 |
|---|---|---|
| scaffold 文件 | 18 | `ls scaffolds/*.toml \| wc -l` |
| scope 总数 | 90 | `grep -c "\[\[scope\]\]" scaffolds/*.toml` |
| Rust crate | 7 | crates/ 目录 |
| 后端子模块 | 22 | `grep -E "^mod [a-z_]+;" lib.rs` |
| 后端命令注册 | 100 | lib.rs:1027 generate_handler 块 |
| 前端 invoke 命令 | 93 | api/ + App.tsx invoke 去重 |
| MCP 工具（agent-server） | 75 | `grep -c "#\[tool("` mod.rs |
| toolbelt manifest | 30 | tool_manifests.json |
| i18n 键 | 2766 | 15 命名空间文件 |
| 前端组件 | 28 | components/ *.tsx |
| 前端 ts/tsx 行数 | 23946 | find + wc |
| 后端 Rust 行数 | 14520 | wc apps/desktop/src-tauri/src/*.rs |
| crate Rust 行数 | 10146+15458 | wc crates/*/src/*.rs |
| crate 单元测试 | 72 | grep #\[test\] |
| tauri 后端测试 | 109 | grep #\[test\] |
| 前端测试文件 | 13 | *.test.ts* |

---

## 2. 核心数据流（三条主链路）

### 2.1 扫描链路
```
点扫描 → scan::scan_path_usn（有基线走 USN 增量重放）
       → 失败回退 scan::scan_path（MFT 直读 → walker 并行遍历）
       → 打 scaffold 标签 + 截断 → 写 scan_tree + cleanup_cache + usn_cursors
       → 返回树给前端
```

### 2.2 清理链路
```
进总览/工作台 → cleanup_suggestions → 命中 cleanup_cache 直接回
             → 未命中读 scan_tree 内存秒算（cached_only=true 绝不回退全盘 walk）
             → 展示清单 → 用户逐项确认
执行 → executor::execute_scope / execute_ai_plan
     → protected_path 守卫（盘根/系统目录/主目录根拒绝）
     → 默认 recycle 进系统回收站（quarantine 7 天隔离可选；delete 仅显式选择）
     → 追加写 undo.jsonl（含真实 bytes_freed）
     → 前端批量剪枝
```

### 2.3 AI 对话链路
```
拖目录 → 组装元数据（不发文件内容，路径脱敏 $HOME）
       → ai::advise 单轮 或 agentChat 多轮（BYOK：Anthropic/OpenAI/Gemini/Ollama）
       → JSON 结构化输出 → 清理建议「生成待确认清单 → 用户逐项勾选」
       → execute_ai_plan（同样过确认门 + 守卫 + undo）
```

---

## 3. 安全模型（写操作三层防线 + 只读恒开）

| 层级 | 机制 | 位置 |
|---|---|---|
| L0 | 只读工具恒开，无确认 | 所有 *_list / *_info / *_collect |
| L1 | 受控操作：默认开 + 确认门 + 会话免确认 | confirm_gate（dry_run 恒过，真实写需 confirmed==Some(true)） |
| L2 | 高危操作：默认关 + 每次确认 | agent.rs:174-186 WRITE_TOOLS 白名单（11 个写工具） |
| L3 | 永禁不可解锁：格式化/刷 BIOS/超频/删系统文件 | 未注册任何工具；protected_path 兜底 |
| 守卫 | protected_path：盘根/系统目录/主目录根直接拒绝 | lib.rs:625-638 |
| 守卫 | scaffold 红线：glob 不得命中 DB/聊天/账号/收藏/加密物料 | scaffold_red_line_reject |
| 守卫 | SSRF：AI provider URL 白名单 + 内网段拒绝 | ai.rs:417-460 |
| 守卫 | 温度熔断：压测超阈值自动停机 | hw.rs:1620 |
| 守卫 | 插件安装：force https + Ed25519 签名校验 | plugin_remote.rs |
| 审计 | undo.jsonl 追加式撤销日志（可追溯可撤销） | executor.rs |

**铁律**：清理必须先出清单 + 用户确认，禁自动执行；默认进系统回收站；宁可错放 1000GB 也不误删单个文件。

---

## 4. 模块详解（干什么 + 目的）

### 4.1 后端 src-tauri 22 子模块

| 模块 | 干什么 | 目的 |
|---|---|---|
| lib.rs | AppState（16 字段）+ 100 命令注册 + 生命周期 | 应用装配层，一切命令的入口 |
| scan.rs | MFT 直读 / USN 增量重放 / walker 回退 | 秒扫整盘，性能铁律第一环 |
| cleanup.rs | 清理建议 / scope 尺寸 / chat 扫描上下文 | 把「可清什么」算出来给用户 |
| executor.rs | 执行 scope / AI 计划 / 回收 / 撤销 | 安全执行 + 可追溯 |
| conda.rs | Conda 环境枚举 / 路径检视 / reveal | 处理 Conda base 环境占位 |
| ai.rs | 多 provider 代理 / web_search / SSRF 防护 | BYOK 对话能力 |
| agent.rs | agent-server 桥（agent_call_tool）+ 写工具白名单 | 把 75 个 MCP 工具接到前端 |
| mcp.rs | 用户自定义 MCP 服务器 CRUD + 调用 + 审计 | 用户自接工具生态 |
| toolbelt.rs | 图吧工具箱目录 / 运行 / 插件市场 | 工具墙核心 |
| plugin_registry.rs | 社区插件市场（列表/搜索/安装/验证/更新/回滚）+ 内置去重 | 插件生态闭环 |
| plugin_remote.rs | 远程 URL 安装（https + Ed25519） | 安全安装通道 |
| hw.rs | 硬件信息 / 磁盘健康 / 压测 / 风扇 / 电源计划 | 硬件面板能力 |
| hw_history.rs | 硬件快照对比 | 硬件状态历史 |
| steam.rs | Steam 游戏 / 创意工坊 / 名称翻译 | Steam 盘点 |
| system.rs | 系统探测 / 网络探测 | 系统状态面板 |
| startup.rs | 开机启动项 CRUD | 启动项管理 |
| drivers.rs | 驱动列表 / 更新检查 | 驱动面板 |
| reminder.rs | 清理提醒后台线程（只出清单不删） | 定期提醒 |
| scaffold_registry.rs | scaffold 注册表（列表/安装/卸载） | 清理脚本管理 |
| dups.rs | 重复文件扫描命令 | 重复文件识别 |
| space_history.rs | 空间历史 | 空间变化趋势 |
| updates.rs | 更新检查 | 版本更新 |

### 4.2 前端 28 组件

| 组件 | 干什么 |
|---|---|
| Overview（总览页） | 磁盘健康 / 空间趋势 / 清理建议入口 |
| Workspace（工作台） | 扫描结果树 + 批量勾选清理 |
| ChatPanel | AI 对话侧栏（拖目录问答） |
| Toolbelt | 工具墙：内置市场 + 社区插件市场（内置去重）+ 已安装台账 |
| HwPanels | 硬件面板（CPU/GPU/内存/主板/温度/压测） |
| SteamInspector(+Modal) | Steam 游戏盘点 |
| SteamWorkshopModal | 创意工坊 |
| McpServers | 自定义 MCP 服务器管理 |
| PermissionCenter | 四权限级可视化 |
| Settings | 设置（AI 服务商/主题/字体） |
| ReminderSettings | 清理提醒配置 |
| CleanupModal / CleanupProposalDialog | 清理确认两件套 |
| SystemTools | 系统工具 |
| Studio | 压测/基准工作室 |
| FileView | 文件视图 |
| DriveStrip | 磁盘条 |
| LeftPanel / Splitter / ProgressButton / ErrorBoundary / Disclaimer / AssetOverview | 布局与通用件 |

### 4.3 7 个 Rust crate

| crate | 干什么 | 公开接口要点 |
|---|---|---|
| scanner | 扫描核心 | `scan(root)` / `scan_with(opts, on_progress)` / Node / usn::{parse_usn_records, resolve_paths, merge_changes, UsnCursor, JournalState} / mft::scan_volume |
| scaffold | 清理脚本 TOML 解析 | `parse_toml(s)` / `load_dir(dir)` / Scaffold{Match, Scope, RecycleGranularity, Mode, Risk, Prompt} / winapp2 兼容（Winapp2Entry/FileKey/RegKey） |
| executor | 执行引擎 | Action{Recycle, Quarantine, Delete} / execute / UndoEntry（含 bytes_freed）/ undo / protected_path 守卫 |
| scaffold-lint | 红线校验 CLI | 遍历 scaffolds/*.toml 断言 glob 不命中红线（DB/聊天/账号/收藏/加密物料） |
| steam-inspector | Steam 库盘点 | 注册表定位 Steam + 读 ACF 清单 → 游戏列表 + 建议清理 |
| toolbelt | 工具墙 | `all_manifests()`（30 工具）/ find_manifest / parser_for / ToolManifest{ToolMode} / Risk / MIN_TIMEOUT 5s MAX 3600s / signature 签名校验 / EMBEDDED_DOC |
| agent-server | 独立 MCP 进程 | stdio 传输，75 个工具（`#[tool_router]` 宏注册），每次调用重新 spawn（无共享状态、崩溃自愈） |

### 4.4 agent-server 75 个工具分布（23 个子模块）

| 子模块 | 工具主题 | 数 |
|---|---|---|
| disk / diskx | 磁盘健康/分区/卷元数据/类型统计/最大文件/重复文件/IO | ~12 |
| files | list_dir / read_file / find_files / file_tree / file_recycle | 5 |
| process | 进程列表/详情/杀进程/CPU 占用 | 4 |
| system / sys | 系统信息/服务/驱动/启动项/服务控制/计划任务 | ~8 |
| apps | 已装程序/卸载/许可证 | 3 |
| hw / hwinfo / superio | CPU/GPU/内存/主板/温度/传感器/电池/SMART/USB/CPU 特性/DRAM/显示器/IPMI/ACPI | ~18 |
| net | 网络状态/连接/速率/共享/WiFi/网卡详情 | 6 |
| sec | 事件日志/防火墙/登录事件/Defender/用户账户 | 5 |
| steam | Steam 游戏盘点 | 1 |
| cleanup | 清理建议 | 1 |
| envx | 环境变量/回收站状态 | 2 |
| selfheal | 风扇自修复（diag/fix/control） | 3 |
| toolbelt | 工具墙 list/run | 2 |
| bench | CPU/内存/磁盘基准 + CPU/GPU 压测 + 停止 + 内存测试 + GPU 快照 | 7 |
| bsod | 蓝屏转储分析 | 1 |
| report | 体检报告/传感器趋势/阈值告警 | 3 |
| 合计 | | 75 |

> 写操作 11 个：process_kill / process_start / file_recycle / scheduled_task_manage(enable/disable) / uninstall_app / service_control / fan_selfheal_fix / fan_control / toolbelt_run / stress_test / stress_test_gpu —— 全部走确认门（L1/L2），其余 64 个只读（L0）。

### 4.5 i18n 15 命名空间

chat(122) / cleanup(212) / common(50) / errors(18) / hw(130) / mcp(206) / overview(192) / perm(112) / settings(178) / shell(208) / steam(198) / studio(118) / system(202) / theme(34) / toolbelt(786) —— 共 2766 键，zh/en 成对；`coverage.test.ts` 守卫键集对等、前缀、静态 `t('key')` 命中与 `MIGRATED_FILES` 棘轮（49 文件硬编码中文 = 0）。

---

## 5. 性能铁律（2026-09-07 专项后）

- **磁盘 IO 是瓶颈**：逻辑优先走内存计算（`scan_tree` / `cleanup_cache` / `usn_cursors`），减少重复遍历。
- **展示性查询绝不触发全盘 walk**：总览页 `cleanup_suggestions(cachedOnly=true)` 未命中缓存就返回空 + 引导「先扫描」；只有 AI 主动查建议（`get_cleanup_suggestions` 工具）才可真算。
- 组件无启动定时器；硬件的 5s 轮询只在硬件页打开时跑。
- 内存缓存三件套：`scan_tree`（完整扫描树）/ `cleanup_cache`（30 分钟清理建议缓存）/ `usn_cursors`（USN 基线游标），按盘根 key。

---

## 6. 已知现状与约束（2026-09-20）

- 无遥测、无账号、无云存储；唯一对外请求是用户配置的 AI 服务商。
- 主仓库 lxfebd/diskpilot **尚不存在**（2026-09-20 验证）——README 下载入口已诚实化（无死链），发布通道待用户决定。
- 插件索引仓库（community registry 正式源）与 Linux 真机验证仍欠。
- 既有缺陷三处未经用户点头不动：window.confirm 违规 / 双括号 / ping 查表（见 memory audit-2026-09-19）。
- rustc 1.98：浮点格式一律 `{:.*}`，禁 `{:.1f}`；vitest 锁 2.x；desktop 用 pnpm 跑 dev；Windows 下 `DISKPILOT_NO_ADMIN=1` 跳过提权；工作区有并发会话，编辑文件前必须重读。

---

## 7. 后续优化完善方向（基于本文档的基准）

> 用户意图：**按照这个架构一步一步开始优化完善修复**。本文档是基准，后续每个改动都应在架构上定位到模块，先出清单再动手。

候选方向（按架构层分组，待用户拍板）：
1. **扫描/性能层**（scanner / scan.rs / cleanup.rs）：USN 增量边界case、MFT 解析健壮性、缓存失效策略。
2. **安全层**（executor / agent.rs / ai.rs）：确认门覆盖度审计、protected_path 边界补漏、SSRF 白名单维护。
3. **插件生态层**（plugin_registry / plugin_remote / toolbelt.rs）：正式社区索引仓库、安装台账一致性、回滚机制验证。
4. **agent-server 层**：75 工具覆盖度审计、写工具确认链路全真验证、新工具按需。
5. **前端体验层**：i18n 剩余硬编码排查、组件 API 收敛、主题/无障碍。
6. **发布层**（用户后续自行决定）：GitHub 仓库创建 / push / tag / release。
