# DiskPilot · 架构与数据流

> 面向开发者的架构总览：分层、模块、IPC 命令、数据流、并发、缓存与性能关键路径。
> 详细结构体/接口/流程的全量技术设计见 [docs/DESIGN.md](docs/DESIGN.md)；讲人话版见 [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)。

## 一、总体架构（5 层，单向依赖）

```
┌─────────────────────────────────────────────────────────┐
│ 用户交互层   点击 / 拖拽 / 对话 / 两步确认                │
├─────────────────────────────────────────────────────────┤
│ 前端表现层   React 18 + TS 组件 · zustand store · 17 主题  │
├─────────────────────────────────────────────────────────┤
│ IPC 层       Tauri Command（75 个）· AppState 全局状态     │
├─────────────────────────────────────────────────────────┤
│ 核心能力层   7 个 Rust crate（零横向依赖）                 │
├─────────────────────────────────────────────────────────┤
│ 系统层       NTFS MFT / jwalk / windows-sys / trash       │
└─────────────────────────────────────────────────────────┘
```

依赖方向严格自上而下，前端不包含业务逻辑，所有计算下沉到 Rust 端。

## 二、Rust crate 一览（7 个，零横向依赖）

| crate | 定位 | 主要 pub 能力 |
|---|---|---|
| `diskpilot-scanner` | 磁盘扫描 | `scan` / `scan_with` / `scan_with_stats_cancellable` / `scan_with_usn` / `sample_paths`；walker.rs：`diskpilot_walker` / `find_matching_dirs` / `tally_dir_scopes`；usn.rs：`parse_usn_records` / `merge_changes` |
| `diskpilot-scaffold` | 清理脚本引擎 | `parse_toml` / `load_dir` / `detect_compiled` / `compile_all` / `expand_env` / `glob_hits_red_line`；winapp2.rs 转换 |
| `diskpilot-executor` | 安全执行器 | `execute`（recycle/quarantine/delete）/ `protected_path` / `list_undo` / `restore_quarantined` / `remove_restored` |
| `diskpilot-scaffold-lint` | CI 静态校验 | 二进制 crate：schema + 非空 glob + 红线 glob 硬错 |
| `diskpilot-steam-inspector` | Steam 盘点 | `parse` / `parse_appmanifest` / `parse_libraryfolders` / `discover_steam_root` / `inspect` / `list_workshop_items` |
| `diskpilot-toolbelt` | 工具箱集成 | `catalog` / `catalog_tree` / `find` / `usage` / `find_tools_root` / `run` / `install_plugin_zip` / `export_plugin_zip` + `manifest::all_manifests`（30 个工具，include_str! 内嵌） |
| `diskpilot-agent-server` | AI 可操作工具集 | 75 个 MCP 工具（64 只读 L0 + 11 写操作，磁盘/文件/进程/硬件/网络/服务/安全审计/盘点/回收站/环境变量/AIDA64 复刻基准压测），rmcp stdio，PathGuard 环境变量守卫 |

所有 crate 通过 workspace 依赖聚合到 `apps/desktop/src-tauri`（磁盘布局见 [AGENTS.md](AGENTS.md)）。

## 三、后端（src-tauri）

### 3.1 模块与命令（`lib.rs` 1109 行，注册 75 个命令）

| 模块文件 | 职责 | 主要命令 |
|---|---|---|
| `scan.rs` | 扫描（全量 MFT/jwalk + USN 增量），打标签+截断 | `scan_path` / `scan_path_usn` / `cancel_scan` / `tree_subtree` |
| `cleanup.rs` | 从缓存/树/全盘统计可清理字节，生成建议 | `scope_sizes` / `scope_sizes_batch` / `cleanup_suggestions` / `chat_scan_context` |
| `executor.rs` | 清理执行 + undo 日志 | `execute_scope` / `recycle_paths` / `execute_ai_plan` / `list_undo` / `undo` |
| `conda.rs` | Conda 环境列表 + 路径采样 + 定位 | `list_conda_envs` / `inspect_path` / `reveal_in_explorer` |
| `ai.rs` | AI 建议 / 网页搜索 / AI 代理（SSRF 私网拦截） | `advise` / `set_advisor` / `web_search` / `ai_proxy` |
| `hw.rs` | 硬件检测与受控压测（TTL 缓存） | `hw_info` / `hw_disk_health` / `hw_run_test` / `hw_stop_test` / `hw_report` / `power_plan` |
| `steam.rs` | Steam 库盘点 | `list_steam_games` / `list_steam_workshop_items` / `fetch_workshop_titles` / `open_steam_url` |
| `system.rs` | 系统状态探针 + 网络连通 | `run_system_probe` / `network_probe` |
| `toolbelt.rs` | 工具箱：CLI 白名单执行 / 工具墙 / 插件管理 | `toolbelt_status` / `toolbelt_catalog` / `toolbelt_usage` / `toolbelt_run` / `toolbelt_manifests` / `toolbelt_launch` / `toolbelt_recycle` / `plugin_*` |
| `agent.rs` | AI 可操作工具桥：spawn agent-server + rmcp client | `agent_list_tools` / `agent_call_tool` / `agent_server_ready` |
| `updates.rs` | GitHub Releases 版本检查 | `check_update` |

### 3.2 AppState 全局状态（`lib.rs:50-76`）

```rust
struct AppState {
    scaffolds: Mutex<Vec<Scaffold>>,              // 静态配置，启动加载（12 个内嵌）
    advisor: Mutex<Option<Provider>>,             // 当前 AI Provider
    quarantine_root: PathBuf,                     // app_data_dir()/quarantine
    undo_log: PathBuf,                            // app_data_dir()/undo.jsonl
    scan_cancel: Arc<AtomicBool>,                 // 扫描取消标志
    cleanup_cache: Mutex<HashMap<String, CleanupCacheEntry>>, // key=归一化盘根，30 分钟 TTL
    scan_tree: Mutex<Option<Node>>,               // 最近一次完整扫描树（未截断）
    usn_cursors: Mutex<HashMap<String, UsnCursor>>, // USN 增量基线
    hw_stop: Arc<AtomicBool>,                     // 硬件压测停止开关
}
```

并发原则：复杂结构 `Mutex` 短锁；布尔标志 `AtomicBool` 无锁；`spawn_blocking` 把重活移出 UI 线程（但**磁盘 IO 仍是全局瓶颈**——见性能关键路径）。

### 3.3 启动时序（`main.tsx` / `lib.rs setup`）

1. `main.tsx`：`initTheme()`（防主题闪烁）→ 8 个 css 按序 import → `<ErrorBoundary><App/></ErrorBoundary>`。
2. 后端 `setup`：建 `app_data_dir` → `undo.jsonl` + `quarantine` → `load_all_scaffolds()`（resource_dir → ../../scaffolds → app_data_dir 逐层合并，内嵌 `include_dir!` 兜底，12 个）→ `manage(AppState)`。
3. 前端 `App.tsx`：`view` 默认 `'overview'`；启动**不自动扫描**（总览页挂载只发 `cleanup_suggestions(cachedOnly=true)`，未命中返回空 + 引导「先扫描」——2026-09-07 性能修复）。

## 四、数据流（4 条关键路径）

### 4.1 扫描

```
用户点扫描/盘卡 → App.tsx scan() → api.scan(t) → invoke('scan_path_usn')
  ├─ 命中 USN 基线 → 增量扫描（免全量）
  └─ 无基线/失败 → invoke('scan_path')：MFT 直读（Windows NTFS）→ 失败回退 jwalk
      → 进度事件 scan-progress 持续 emit（files_seen/bytes_seen/current_path）
      → 完成：suggestions_from_tree（先算建议）→ tag_and_truncate（打 scaffold 标 + 截断 100/50/20）
      → 完整树存 scan_tree，建议写 cleanup_cache，emit scan-stats → 返回截断树
前端：cacheDrives 写入 scanCache（LRU 6 盘）→ 渲染 TreeView + Treemap
```

### 4.2 清理建议（关键性能路径）

```
总览页挂载 → api.cleanupSuggestions(path, days, cachedOnly=true)
   → cleanup_suggestions 后端：
       ① cleanup_cache 命中（30 分钟 TTL）→ 直接返回
       ② 未命中 + scan_tree 树覆盖该路径（且未传 days 过滤）→ 内存树秒算 suggestions_from_tree
       ③ cached_only=true 且①②都 miss → return Ok(Vec::new())  ← 绝不回退全盘 walk
       ④ cached_only=false（AI get_cleanup_suggestions 工具 / 主动查询）→ 才允许全盘 walk 真算
前端空态文案：「扫描后自动计算可回收空间（不扫描不占磁盘 IO）」
```

**为什么这样设计**（2026-09-07 修复，即"应用一开鼠标卡成翔"根因）：此前 ①② miss 时静默回退整盘 `tally_dir_scopes` walk → 应用一开就磁盘 IO 饱和。现在展示性查询绝不遍历，主动查询（AI）才可真算。

### 4.3 执行清理

```
Studio 卡片 / AI 清单 → CleanupModal / CleanupProposalDialog（dry-run 预览 + 两步确认）
  → api.executeScope / executeAiPlan → execute_scope / execute_ai_plan
      → 后端 protected_path 守卫（盘根/系统保留目录/主目录根 → 直接 bail）
      → Recycle（trash::delete，带 Windows 误报兜底）/ Quarantine（移动隔离区）/ Delete（仅显式选择）
      → 追加写 undo.jsonl（每行一条 JSON，原子追加）
前端：applyRecycledPrune 一次 pruneMany 批量剪枝（root「全部磁盘」+ 受影响缓存树），selectedPath 置 null
撤销：UndoPanel → api.undo → list_undo 读条目 → restore_quarantined 移回 → remove_restored 清日志
```

### 4.4 AI 对话

```
拖目录进聊天框 → 前端组装元数据（路径脱敏 $HOME，不发文件内容）→ provider.callAdvisor / agentChat
  → Tauri ai_proxy（绕 CORS，SSRF 私网拦截）或浏览器直连 → Provider（OpenAI/Anthropic/Gemini/Ollama）
  → JSON-mode 结构化输出 → 前端 markdown 渲染
AI 工具（22 个，advisor/tools.ts）：web_search / path_size / list_dir / propose_cleanup_plan /
  get_disk_health / get_cleanup_suggestions（可真算）/ get_system_info / run_system_probe /
  get_tool_manifest / get_cli_tool_usage / run_cli_tool（中/高危需确认，会话免确认走权限模型）/
  get_hardware_info / generate_hw_report / run_hardware_test / adjust_power_plan / 插件管理 5 个
AI 另可调 75 个 MCP 工具（64 只读 + 11 写操作，agent_call_tool → agent-server：磁盘/进程/硬件/网络/审计/盘点/回收站/环境变量/AIDA64 复刻，见 docs/tools-agent/TOOL_CATALOG.md）
AI 无直接删除能力：清理 = propose_cleanup_plan → 前端确认清单 → execute_ai_plan
```

## 五、IPC 命令全景（前端 api.ts ↔ 后端 invoke）

前端 `api.ts`（643 行）是**唯一与后端交互层**，37+ 个包装函数。浏览器 mock 模式（`env.ts` 判 `isTauri`）下走 `mocks.ts`（假树/假 scaffold/假工具墙），纯 Tauri 命令 reject。命令分组：

| 领域 | 命令 |
|---|---|
| 扫描 | scan_path / scan_path_usn / cancel_scan / tree_subtree / estimate_size / volume_info / list_drives |
| scaffold | list_scaffolds / install_scaffold / uninstall_scaffold / scaffold_source / scope_sizes / scope_sizes_batch / cleanup_suggestions / chat_scan_context |
| 执行 | execute_scope / recycle_paths / execute_ai_plan / list_undo / undo |
| AI | advise / set_advisor / web_search / ai_proxy |
| 硬件 | hw_info / hw_disk_health / hw_run_test / hw_stop_test / hw_report / power_plan / run_system_probe / network_probe |
| Steam | list_steam_games / list_steam_workshop_items / fetch_workshop_titles / open_steam_url |
| 工具墙 | toolbelt_status / toolbelt_catalog / toolbelt_usage / toolbelt_run / toolbelt_manifests / toolbelt_launch / toolbelt_recycle / set_tools_root |
| 插件 | plugin_install / plugin_uninstall / plugin_market / plugin_activate / plugin_deactivate / plugin_export |
| 配置 | general_config / set_general |
| 更新 | check_update |
| AI 工具桥 | agent_list_tools / agent_call_tool / agent_server_ready |
| 杂项 | inspect_path / reveal_in_explorer / list_conda_envs |

## 六、前端架构（依赖方向：组件 → store → api → types）

```
src/
├── api.ts          # 唯一后端入口（invoke 包装 + mocks fallback）
├── store.ts        # zustand 全局状态（22 个 action），业务逻辑层
├── types.ts        # 前后端 schema 镜像类型
├── theme.ts        # 17 套主题预设 + 动态 accent + 字体缩放 + 偏好键
├── permissions.ts  # 四级权限模型（L0-L3）+ 会话授权缓存
├── advisor/        # AI 客户端：provider.ts / agent.ts / tools.ts / chatHistory.ts
├── components/     # 23 个纯渲染组件（清单见 component-api.md）
├── styles/         # 8 个 css（tokens/layout/chat/steam/settings/overview/toolwall/dark）
├── env.ts / mocks.ts  # 浏览器 mock 模式
└── App.tsx         # 三视图（overview/workspace/tools）+ AI 侧栏布局组装
```

store 关键点（`store.ts`，440 行）：

- `scanCache: Record<string, Node>`，key = `normKey()`（去尾反斜杠 + 大写），LRU 上限 6 盘（`cacheDrives` / `takeDrive`）。
- 扫描树缓存写入：`cacheDrives` 提 key 到最前、超限逐出最旧。
- 批量剪枝：`applyRecycledPrune` 一次 `pruneMany` 把所有成功路径同时从 root 和受影响缓存树剪掉——避免 N 次全树重建（P0 性能项）。
- 聊天多会话持久化 localStorage（`diskpilot.chatSessions` / `diskpilot.activeChatId`，20 会话 × 200 turns）。

三视图布局（App.tsx）：`overview` → AssetOverview；`tools` → Toolbelt；`workspace` → 顶部 DriveStrip + 左栏 LeftPanel（树/treemap/文件 tab）+ 中栏 FileDetailPanel + 右栏 Studio；全局 AI 侧栏 ChatPanel 常驻。

## 七、并发与缓存设计

| 缓存 | 位置 | 生命周期 | 用途 |
|---|---|---|---|
| `scan_tree` | 后端 AppState | 最近一次完整扫描树 | 清理建议/总览/AI/dry-run 内存秒算，免二次全盘 walk |
| `cleanup_cache` | 后端 AppState | 30 分钟 TTL | 清理建议缓存（key=归一化盘根） |
| `scanCache` | 前端 store | LRU 6 盘 | 扫描树前端缓存，点盘秒开 |
| `usn_cursors` | 后端 AppState | 会话内 | USN 增量基线 |
| 硬件 TTL | hw.rs | 30 秒 | hw_info 缓存避免高频采集 |

- 扫描进度节流：~5000 文件或 500ms 双阈值 emit `scan-progress`；取消标志每 5000 文件检查。
- AI 多轮 agentChat 上限 `MAX_ROUNDS=6`；历史超预算压缩摘要（chatHistory.ts）。
- 并发会话警告：工作区可能多会话同时改文件，**编辑前必须重读**。

## 八、安全架构（纵深三层 + 权限模型）

1. **静态层**：scaffold-lint（CI 红线硬错，`RED_LINE_SAMPLES` 含 `*.db`/`Msg`/`Accounts`/`Favorite`/`key` 等族）+ 每个 scaffold 的 safety 集成测试（正向 + 红线 zero-match）。
2. **运行时层**：`protected_path` 守卫（盘根/系统目录/主目录根直接 bail）；两步确认（dry-run 预览 + 再点一次，禁 window.confirm）；四级权限（L0 恒开 / L1 默认开+确认+会话免确认 / L2 默认关+每次确认 / L3 永禁——`isPermEnabled` 对 L3 恒 false，工具墙启动也被 manifest `permission_level==='L3'` 拦截如 FPT64）。
3. **可逆层**：回收站兜底 + 7 天隔离区 + `undo.jsonl` 追加式日志。AI 只出清单，无直接删除。

## 九、性能基线（C 盘 189 万文件 / 31 万目录，实测）

| 指标 | 数值 |
|---|---|
| MFT 快速路径 | 秒级（NTFS 卷直读；失败落 walkdir 且 WARN） |
| walk 遍历 | ~140s（walkdir 物理极限） |
| 树构建 | ~2s（内存聚合） |
| 标签+截断 | ~10.7s（must_have_child 内存化后，原 229s） |
| 建议计算 | ~17.3s（内存树；文件节点短路后原 152.7s） |
| 端到端 | ~169s（原 ~302s） |
| 增量（USN） | 秒级（有基线时） |

**性能铁律**：磁盘 IO 是瓶颈 → 优先内存计算；展示性查询绝不触发全盘 walk；组件无启动定时器。

## 十、技术债与演进

- ✅ 已完成：流式扫描、USN 增量、内存树复用、批量剪枝、TreeView 虚拟滚动、lib.rs 拆子模块、styles.css 拆 8 文件、toolbelt 30 工具、scaffold-gen CLI、启动不遍历。
- ⏳ 待做：完整插件系统（分发/下载/签名，模型已就位待产品决策）、国际化 i18n、macOS/Linux 预编译、更多 scaffold。
- 完整路线图与优先级见 [ROADMAP.md](ROADMAP.md)。

## 扩展阅读

- [docs/DESIGN.md](docs/DESIGN.md) —— 详细技术设计（结构体/接口/流程全量，2026-09-07 核对）
- [docs/ARCHITECTURE_FOR_AI.md](docs/ARCHITECTURE_FOR_AI.md) —— AI 视角完整架构 + 「如果让你接手，从哪看起」
- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) —— 讲人话版（给普通用户，含开源依赖表）
