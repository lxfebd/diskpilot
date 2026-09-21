# DiskPilot 项目架构与设计文档

> 本文件是 DiskPilot 的完整架构说明，面向工程师 / AI 读者。涵盖：项目定位、技术栈、crate 架构、核心数据流、安全模型、前端结构、性能设计与已知技术债。最后附带「如果让你接手，从哪看起」的建议。

---

## 一、项目定位与核心主张

DiskPilot 是一个 **Windows 优先的磁盘清理桌面应用**（Tauri 2 + React 18 + Rust）。它的核心主张是三句话：

1. **秒扫整盘**——Windows 上直读 NTFS MFT（Master File Table），整盘 C: 只需 2–5 秒；跨平台用 jwalk 兜底。
2. **把不认识的文件夹拖给 AI 问**——拖拽目录到聊天框，AI 解释"这是什么、能不能删、删了会丢什么"。
3. **已知应用走专属清理脚本（scaffold）**——每种常见软件（微信、Conda、浏览器等）一份 TOML 描述"哪些是可再生的缓存 + 哪些是红线绝不能碰"，按 scope 逐项进回收站清理。

与"按大小排序删大文件"类工具的本质区别（用户原话的答案）：**它不猜"哪个没用"，只认"已知可再生的缓存目录 + 位置签名吻合 + 有测试兜底不会越界"**。大而珍贵的用户数据天然不在可清理集合里。

设计口号："宁可错放 1000GB，不可错删一个文件。"

---

## 二、技术栈总览

| 层 | 技术 |
|---|---|
| 前端 | React 18 + TypeScript + Tauri 2 + react-markdown + zustand + d3-hierarchy + @tanstack/react-virtual + lucide-react |
| 后端 | Rust workspace（7 个 crate）+ Tauri IPC（command 层） |
| 扫描器 | Windows: NTFS MFT 直读（`ntfs` crate）/ 跨平台: `jwalk`（rayon 并行） |
| 清理执行 | `trash` crate（系统回收站）+ 自建 quarantine + undo 日志 |
| AI 接入 | BYOK（自带 Key）：Anthropic / OpenAI / Gemini / Ollama 四协议，JSON-mode 结构化输出 |
| 包管理 | pnpm workspace（根 package.json + apps/desktop）+ Cargo workspace |
| 数据 | 全部本机 `~/.diskpilot/`（undo.jsonl + quarantine/），不上云 |

版本：0.2.0 · MIT · Rust edition 2021 · 前端 vite 5 + vitest 2 + TS 5.6

---

## 三、Crate 架构（Rust workspace）

Cargo workspace 共 7 个成员 crate，**叶子 crate 之间零互依赖**（解耦干净）：

```
crates/
├── scanner/          # 磁盘扫描：jwalk 并行 walk + 进度回调 + 系统目录黑名单
├── scaffold/         # 清理脚本引擎：TOML 解析、env 展开、globset 编译、detect 匹配
├── executor/         # 安全执行：Recycle(默认)/Quarantine/Delete 三模式 + undo.jsonl
├── scaffold-lint/    # CI 红线校验：scaffold TOML 的 glob 边界 lint（必须 0 error）
├── steam-inspector/  # Steam 盘点：只读解析 .acf / libraryfolders.vdf（KeyValues 格式）
├── toolbelt/         # 图吧工具箱集成：CLI 工具目录 + 文档驱动的参数表（include_str! 内嵌）
└── agent-server/     # AI 可操作工具集：75 个 MCP 工具（64 只读 L0 + 11 写，stdio，spawn 即连，无共享状态）
apps/desktop/src-tauri/  # Tauri 壳：所有 Tauri command 的宿主，AppState 持有全局状态
```

### 各 crate 职责细节

**scanner**（`crates/scanner/src/lib.rs`）
- 入口：`scan_with_stats_cancellable(root, opts, cancel_flag, on_progress)` → `Node` 树
- `Node` = 目录树节点，含 `size`（子树字节和）、`file_count`、`top_extensions`（扩展名占比 Top N）、每目录 top-K 文件内存裁剪
- v0.2 规划：Windows 直接读 NTFS MFT 实现 2–5s 整盘扫描（当前实现是 jwalk + `ntfs` crate 的实验性入口，实测非管理员权限下 MFT 打开失败会 WARN 后回退 walkdir）
- 系统目录黑名单：WinSxS / Installer / $Recycle.Bin / hiberfil.sys / pagefile.sys 等在收集阶段跳过（避免污染"可回收潜力"统计，这些占几十 GB 但不可删）

**scaffold**（`crates/scaffold/src/lib.rs`）——**整个项目的核心概念**
- `Scaffold` 结构：`id / name / homepage / risk / disclaimer / detect[] / match{name_contains, must_have_child} / scopes[]`
- TOML 用 `[[scope]]`（单数）写作，JSON 给前端用 `scopes`（复数），serde rename 桥接
- `detect`：确认"这台机器装了这个软件"（如 `%LOCALAPPDATA%/Google/Chrome/User Data`）
- `match.name_contains`：目录名包含某片段；`match.must_have_child`：**必须存在某子目录**才判定命中（如微信必须 `all_users` 子目录，防止误伤用户自建同名目录）
- `compile_all` 把 detect globs 编译成 `GlobSet`（含 `%VAR%` / `$VAR` / `${VAR}` env 展开），`detect_compiled` 逐目录匹配——单个坏 pattern 只跳过自身不毒化整个 scaffold
- `winapp2` 子模块：从 WinApp2.ini 格式转换 legacy 清理规则（保留但非主路径）
- **检测性能（2026-09-07 实测优化）**：`detect_compiled` 的 `must_have_child` 原实现逐目录 `path.join(child).exists()` 磁盘 IO（C 盘 31 万目录 → tag 阶段 229s）；改为接收 `existing_children: Option<&[String]>` 内存子目录名列表查找后 → 10.7s

**executor**（`crates/executor/src/lib.rs`）
- `Action` 三模式：Recycle（默认，系统回收站可恢复）/ Quarantine（7 天隔离区）/ Delete（用户显式选择）
- 每次操作追加 `undo.jsonl`（timestamp/action/source/destination/reason），前端"最近清理"面板 + 撤销按钮读它还原

**agent-server**（`crates/agent-server/`）
- AI 可操作工具集：75 个 MCP 工具（64 个**只读 L0**：磁盘/文件/进程/硬件/网络/服务/安全审计/盘点/回收站/环境变量/信息补全 + 11 个写操作），分组见 `docs/tools-agent/TOOL_CATALOG.md`；写工具白名单（`process_kill`/`service_control`/`fan_selfheal_fix`/`fan_control`/`file_recycle`/`process_start`/`scheduled_task_manage`/`uninstall_app`/`toolbelt_run`/`stress_test`/`stress_test_gpu`）在 `agent.rs` WRITE_TOOLS 单源真值，要求 `confirmed=true` 硬校验
- 用 rmcp 0.4.1 官方 SDK（`#[tool_router]` / `#[tool_handler]` / `#[tool]` 声明式）+ stdio transport；桌面端经 `agent.rs` 每次调用重新 spawn 进程 + rmcp client over stdio 连接（无共享状态、崩溃自愈天然成立）
- 全部走 `tools/list` + `tools/call` 标准通道，前端工具墙「AI 可操作」分区可查看
- 路径守卫 PathGuard：从环境变量推导（SystemRoot/windir/ProgramFiles/ProgramData/USERPROFILE），拒绝盘根/系统目录/主目录根；不读 WiFi 密码，写工具走 WRITE_TOOLS 单源真值 + 关键服务/系统任务/关键进程黑名单 + 卸载器白名单三重守卫

> AI 顾问能力（provider 无关、JSON-mode、只发目录元数据）已随 `crates/advisor` 删除而单源化到**前端 `advisor/` 目录**（provider.ts/agent.ts/tools.ts）+ 后端 `ai.rs`（`advise`/`web_search`/`ai_proxy`），不再有独立 crate。

**scaffold-lint**（`crates/scaffold-lint/src/main.rs`）
- CI 里对每个 scaffold TOML 跑红线 lint，必须 0 error
- 配合 `crates/scaffold/tests/<id>_safety.rs` 集成测试（正向断言 + 红线断言）双重守护

**steam-inspector**（`crates/steam-inspector/src/lib.rs`）
- 只读解析 Steam 的 `.acf`（appmanifest）和 `libraryfolders.vdf`（KeyValues 格式）→ 游戏清单/占用/最近游玩
- 硬规则：**此 crate 只读**，不写盘、不 shell out

**toolbelt**（`crates/toolbelt/src/lib.rs`）
- 集成"图吧工具箱"的 CLI 工具目录（对齐 luolangaga/tubatools TubaWinUi3 的 CliToolboxCatalog 设计）
- 权威参数表 = 内嵌的《CLI工具使用文档.md》（`include_str!` 编译期内嵌）——**文档即目录**，运行时解析，不维护第二份可能过期的参数表
- 核心出口：`catalog`/`find`（索引）、`usage`（用法+参数+示例）、`toolbelt_command_for`（解析成可执行命令）、`run`（执行+超时强杀）
- 工具目录旁可放 `toolbelt.toml` 覆盖 risk/args/usage/timeout
- `manifest` 子模块：30 个工具的 `ToolManifest`（purpose/when_to_use/invocation/output/risk/permission_level L1-L3/examples），驱动 AI 路由与前端详情卡

---

## 四、Tauri 层（apps/desktop/src-tauri）

lib.rs 约 2100 行，拆成 5 个子模块：

| 文件 | 职责 | 行数 |
|---|---|---|
| `lib.rs` | 核心：AppState、scan_path、cleanup_suggestions、execute_scope、undo、scaffold 加载 | ~2100 |
| `hw.rs` | 硬件信息（hw_info 30s TTL 缓存）、nvidia-smi、压测、电源计划 | 844 |
| `ai.rs` | AI 顾问代理、web_search（Bing/DDG 解析）、SSRF 防护（blocked_private_target） | 370 |
| `toolbelt.rs` | 工具墙：catalog/usage/run/launch/manifests | 385 |
| `system.rs` | 系统探测（CPU 占用采样）、网络 ping | 221 |
| `steam.rs` | Steam 盘点命令桥 | 148 |

### AppState（全局状态，Mutex 包裹）

```rust
struct AppState {
    scaffolds: Mutex<Vec<Scaffold>>,          // 加载的清理脚本
    advisor: Mutex<Option<Provider>>,         // AI provider
    quarantine_root: PathBuf,                 // 隔离区根目录
    undo_log: PathBuf,                        // undo.jsonl 路径
    scan_cancel: Arc<AtomicBool>,             // 扫描取消标志
    cleanup_cache: Mutex<HashMap<String, CleanupCacheEntry>>, // scope 建议缓存（key=归一化盘根）
    scan_tree: Mutex<Option<Node>>,           // 最近一次完整扫描树（未截断）
    hw_stop: Arc<AtomicBool>,                 // 硬件压测停止开关
}
```

### 核心命令

**扫描**：`scan_path(path, keep_files_per_dir)` → `Node`
- 在 `spawn_blocking` 里跑 scanner；完成后在内存里做三件事：`full = node.clone()`（保留完整树）→ `suggestions_from_tree`（基于完整树算各 scope 可清理字节，**必须在截断前**）→ `tag_and_truncate`（逐目录打 scaffold_id + 按深度截断 children 上限 100/50/20）
- 完整树存 `state.scan_tree`；截断树返回前端
- 计时日志：`scan_path: cmd_ms scanner_ms tag_ms overhead`

**建议**：`cleanup_suggestions(root_path, scope_days)` → Vec<CleanupSuggestion>
- 优先命中 `cleanup_cache`（scan 后写入）秒回
- 未命中 → 用 `state.scan_tree` 内存树兜底（仅从未扫过的盘才全盘 walk）——**这是"切页不重扫"修复的关键**

**执行**：`execute_scope(scaffold_id, scope_id, root_path, dry_run, ..., confirmed)` → Vec<UndoEntry>
- dry_run 返回命中清单；`dry_run=false` 时后端硬校验 `confirmed=true`，否则拒绝
- `execute_ai_plan(selected_paths, user_confirmed, dry_run)`：AI 提的清理计划批量执行
  （确认门 + `cleanup.execute` 权限门 + 强制回收站）
- `recycle_paths(paths, reason, confirmed)`：文件树右键 / 聊天建议卡的「移入回收站」。
  动作在后端写死 Recycle —— 前端提交不了永久删除（旧 `execute_plan` 收整个 Plan，
  等于把 `action=delete` 开放给任何能 invoke 的一侧，已删除）

**撤销**：`list_undo(limit)` / `undo(index)` — 读 undo.jsonl，从回收站/隔离区还原

**其他**：`list_scaffolds` / `detect_scaffold` / `scope_sizes` / `scope_sizes_batch` / `estimate_size` / `cancel_scan` / `inspect_path`（列目录样本）/ `reveal_in_explorer` / `chat_scan_context`（AI 总览摘要）/ `list_drives` / `volume_info`

### 性能实测数据（2026-09-07，C 盘 189 万文件 / 31 万目录）

| 阶段 | 优化前 | 优化后 |
|---|---|---|
| walk（jwalk 并行） | ~140s | ~140s（磁盘 IO 物理极限） |
| build_tree | ~2s | ~2s |
| tag_and_truncate（detect + 截断） | 229s | 10.7s |
| suggestions_from_tree | 152.7s | 17.3s |
| **总 cmd_ms** | **~302s** | **~169s** |

两个关键优化：
1. `must_have_child` 磁盘 exists → 内存 children 查找（`detect_compiled` 加 `existing_children` 参数）
2. `suggestions_from_tree` 文件节点短路：所有 scope 都是目录粒度（`recycle_granularity = "directory"`），文件节点无需 glob 匹配（原实现 190 万文件 × 54 scope 无效循环）

---

## 五、Scaffold 系统（清理脚本）

### 文件位置与清单（18 份 / 90 个 scope）

```
scaffolds/
├── browser-cache.toml    # Chrome/Edge 缓存 + ServiceWorker
├── conda.toml            # conda 环境/pkgs 缓存（stale env 按 mtime>90 天）
├── crash-dumps.toml      # 用户级 + WER + 系统级崩溃转储
├── dev-caches.toml       # npm/pnpm/yarn/pip/cargo 包缓存
├── discord.toml          # Electron 缓存 / GPUCache / Code Cache（5 scope）
├── docker-buildx.toml    # BuildKit 构建缓存 + buildx 元数据
├── engine-caches.toml    # Unity / UE / Godot 引擎级缓存（7 scope，绝不进 Library/.uproject/.godot）
├── firefox-cache.toml    # 网络缓存/启动缓存/shader
├── huggingface-cache.toml# HF hub 模型/数据集/spaces 缓存
├── ide-caches.toml       # VSCode/Cursor/IntelliJ 系缓存
├── obs-cache.toml        # OBS 崩溃转储/日志
├── qq-pc.toml            # QQ 9.x 经典版缓存（5 scope，绝不碰 nt_data 聊天消息）
├── security-suites.toml  # 360 全家桶 / 电脑管家 / 火绒缓存（2 scope，不动隔离区与本体）
├── steam-shadercache.toml# Steam ShaderCache
├── system-temp.toml      # %TEMP%
├── teams.toml            # Microsoft Teams 1.x + 2.x 双兼容（11 scope）
├── wechat-pc.toml        # 微信 3.x+4.x 双兼容，23 个 scope（最复杂）
└── zoom.toml             # 录制缓存 / WebRTC 媒体缓存（6 scope，录制只 detect 不 scope）
```

### TOML 结构示例（骨架）

```toml
id         = "wechat-pc"
name       = "WeChat (PC)"
risk       = "low"
disclaimer = """..."""

detect = ["%USERPROFILE%/Documents/xwechat_files", "**/WeChat Files"]

[match]
name_contains   = ["xwechat_files", "WeChat Files"]
must_have_child = ["all_users"]

[[scope]]
id                  = "wechat-cache"
label               = "微信缓存"
glob                = "**/xwechat_files/**/Cache/**"
mode                = "recycle"
category            = "cache"
prompt              = { kind = "days", default = 30 }
recycle_granularity = "directory"
```

字段语义：
- `detect`：软件存在的证据（env 展开的路径 glob）
- `match.name_contains` + `must_have_child`：目录签名双重验证
- `scope.glob`：精确锚定的清理目标（**绝不用裸 `**/` 前缀**，防误伤）
- `mode`：recycle（默认）/ quarantine / delete
- `category`：cache / media / backup / envs——驱动 UI 分组（media 置顶、cache 合并按钮、backup 置底）
- `variant`：版本标签（如微信 3.x/4.x），未检测到该版本时隐藏 scope
- `recycle_granularity`：file（默认，逐文件回收站条目）/ directory（整目录单条回收站条目，适合海量小文件）
- `prompt`：执行前询问参数（days/bytes/choice/confirm/none）

### 红线（Hard rules，CLAUDE.md 硬约束）

任何 scope glob **不允许命中**：`*.db`/`*.db-wal`/`*.db-shm`（聊天/账号 DB）、`**/db_storage/**`、`**/Msg/**`、`**/MultiMsg/**`（聊天记录）、`**/Accounts/**`、`**/All Users/**`、`**/login/**`、`**/config/**`（账号状态）、`**/Favorite*/**`、`**/Fav/**`（收藏）、`**/key/**`、`**/crypto/**`（加密物料）。

### Safety 测试（强制）

每份 scaffold 必须配 `crates/scaffold/tests/<id>_safety.rs`：
- **正向断言**：每个 scope 至少一条命中路径
- **红线断言**：一组红线路径必须 zero match
- 模板：`crates/scaffold/tests/_templates/scaffold_safety.rs`；测试 helper 里的 env 展开要同时支持 `%VAR%` 和 `${VAR}`（镜像生产 `expand_env`）
- CI 必跑，没测试不收新 scaffold

---

## 六、前端架构（apps/desktop/src）

### 目录结构

```
src/
├── App.tsx            # 主布局：左侧树 + 中间聊天/工作台 + 右侧 Studio/总览
├── main.tsx           # 入口：initTheme() 防主题闪烁
├── api.ts             # Tauri command 的 TS 封装（含 isTauri 分支 + mock fallback）
├── mocks.ts           # 浏览器调试用的 mock 后端
├── store.ts           # zustand 全局状态（扫描树缓存、清理选择、聊天会话持久化）
├── types.ts           # Node/Scaffold/Scope/Advisor 前后端 schema 镜像
├── theme.ts           # 17 套主题预设（语义 token + 动态 accent + fontScale）
├── tooltree.tsx       # 树操作工具（pruneMany 批量剪枝、treeMayContain 判断）
├── permissions.ts     # 权限分级（L0-L3）+ isPermEnabled 门
├── advisorClient.ts   # AI 客户端（provider 路由、工具定义）
├── hwProfile.ts       # 硬件画像
├── colors.ts / format.ts / env.ts
├── advisor/           # AI 子模块（chatHistory/tools 等）
└── components/        # 24 个组件
```

### 核心组件

| 组件 | 职责 |
|---|---|
| **LeftPanel** | 磁盘/目录树 + 扫描入口 |
| **TreeView** | 虚拟滚动树视图（@tanstack/react-virtual），每行占用百分比条 |
| **Treemap** | d3-hierarchy 矩形树图 |
| **ChatPanel** | 拖拽文件夹给 AI + 对话（react-markdown 渲染） |
| **Studio** | 按 scaffold 渲染清理卡片（分类分组、scope 复选框、dry-run 预览） |
| **AssetOverview** | 首页总览：磁盘卡片墙 + 分类占用卡片墙 + Top N 大目录 |
| **Toolbelt** | 图吧工具箱工具墙（12 分类导航、风险徽标、详情卡、双击启动） |
| **CleanupModal / CleanupProposalDialog** | 清理确认流程（两步确认 + dry-run 预览） |
| **PermissionCenter** | 权限中心（L0-L3 分级，高危工具拦截） |
| **Settings** | 设置：17 套主题切换（色板+字体联动）+ AI 配置 |
| **SteamInspector / SteamWorkshopModal** | Steam 盘点 + 工坊订阅查看 |
| **UndoPanel** | 最近清理 + 一键撤销 |
| **HwPanels** | 硬件信息面板 |
| **ErrorBoundary** | 破坏性操作组件必须包裹的守卫 |

### 状态管理（store.ts）

- zustand + 局部持久化（localStorage：聊天会话、活动会话、主题）
- `MAX_CACHED_DRIVES = 6` 的 LRU 扫描树缓存
- 关键操作：`recyclePaths` / `aiRecyclePaths` 用 `pruneMany` 批量剪枝（一次树遍历删 N 个目标），避免 N 次全树重建
- `treeMayContain(treeKey, path)`：整段盘符/路径前缀判断（大小写不敏感）

### 主题系统（theme.ts）

- **17 套预设**：Vercel 技术风 / Minimalist 极简 / 豆包 Docs / Claude 暖纸 / Google 清新 / 火山引擎 / Nerv 霓虹 / TRAE 工坊 / Motion Fit 动感 / Barbie 芭比 / Apple 果味 / Golden Time 鎏金 / Vibe Camp 露营 / 21st 石墨 / Steam 蒸汽 / 深色(Nerv) / 专业工具风
- 每套定义完整语义层（`--bg/--surface/--text/--accent/--risk-*`）+ 兼容旧 `--ink/--paper/--pink` 命名
- `applyTheme` 写 CSS custom properties 到 `<html>`（inline 胜出 cascade）+ `data-theme` 属性 + `colorScheme`
- 动态 accent：用户自定义主色，暗色主题自动提亮；`--ok`/`--risk-safe` 跟随 accent（够亮才替换）
- 字体联动：每套主题配 `ui` / `editor` 字体族；fontScale 0.8–1.3 缩放

---

## 七、安全模型（三层防御）

1. **静态层（lint + safety test）**：scaffold-lint 检查 glob 边界；safety 集成测试锁死"可清理集合"不越界。glob 写宽 = 测试 fail，不许放宽测试只能收紧 glob。
2. **运行时层（确认流 + 权限门）**：
   - 所有破坏性命令（execute_scope / recycle_paths / execute_ai_plan / toolbelt_recycle）要求：ErrorBoundary 包裹 + 两步确认或 dry-run 预览（禁止裸 `window.confirm`）+ 默认 recycle 模式；**并且后端同口径硬校验 `confirmed=true`**（前端的确认窗只是体验层）
   - 工具墙高危工具（BIOS 刷写 FPT64 等）L3 永禁；中危 L2 需确认 + 后端风险门双保险
   - `execCliTool` 类入口检查权限开关
3. **可逆层（回收站 + 隔离区 + undo）**：默认 recycle 可还原；每次操作写 undo.jsonl 支持一键撤销；可选 7 天 quarantine

隐私：枚举用户数据目录只用 Glob/`ls` 列文件夹名，**绝不 Read 文件内容**；AI 只收目录元数据；SSRF 防护（ai.rs `blocked_private_target` 拦截内网目标）。

---

## 八、AI 顾问链路

1. 用户拖拽目录 → 前端组装 `AdvisorRequest`（path/size/file_count/top_extensions/sample_paths≤20/neighbors/scaffold_hint）
2. `advise` command → 前端 advisor/（provider.ts 按 Anthropic/OpenAI/Gemini/Ollama 路由）+ 后端 `ai.rs` 代理，JSON-mode 结构化输出
3. `chat_scan_context`：从 `state.scan_tree` 直接算结构化摘要（无需重新扫描）给 AI 全局视野
4. AI 可调用工具：`run_cli_tool`（toolbelt 集成）、`path_size`（estimate_size 单次 walk）、`web_search`（Bing/DDG HTML 解析，带 SSRF 防护）、65 个 MCP 只读工具（agent_call_tool → agent-server）
5. AI 提出清理计划 → `execute_ai_plan`（用户确认 + dry-run 后执行）

---

## 九、测试策略

| 层 | 工具 | 覆盖 |
|---|---|---|
| Rust 单元 | cargo test | scaffold 解析/detect 等价性/executor/undo/winapp2 转换 |
| Rust 集成 | cargo test（tests/） | 9 个 scaffold safety 测试（正向+红线） |
| Rust 回归 | cargo test --workspace | 全 workspace 测试全绿（scanner/executor/toolbelt/steam-inspector/scaffold/agent-server/desktop lib） |
| 前端类型 | tsc --noEmit | 0 错误 |
| 前端单元 | vitest | 62 测试（format/tooltree/advisor/toolbelt/agent tools 等） |
| 手动 | pnpm tauri dev | 扫描提速实测、切页不重扫、工具墙、多主题 |

注意：本环境 rustc 1.98 拒绝 `{:.1f}` 格式字符串（浮点格式化必须用 `{:.*}`）；`cargo test` 需要 `DISKPILOT_NO_ADMIN=1`（main.rs 的 build.rs 需要 elevation 否则 os error 740）。

---

## 十、已知技术债 / 待办

1. **README 路线图**：
   - [x] 秒扫、AI 分析、18 份清理脚本、撤销、Steam 着色器、首页总览、性能专项
   - [ ] 更多软件脚本（Docker buildx ✅ 已完成、HuggingFace ✅、OBS ✅、IDE 缓存 ✅、QQ/Discord/引擎/Teams/Zoom/安全软件 ✅——README 已同步为 18 份 / 90 scope）
   - [ ] macOS/Linux 预编译版（需签名 + 真机验证）
   - [ ] `scaffold new` CLI + 社区分享仓库
2. **性能 P1/P2（用户未点头，保持挂起）**：scanner 祖先链 ext 复制优化、subdirs 去克隆、scan_path 骨架/流式/按需子树、execute_scope 复用 scan 树缓存、MFT 对齐 walkdir top-K
3. **架构清理**：styles.css 单文件 4800 行（硬编码色 + 死 CSS 区块）；lib.rs 历史 3781 行已拆 5 模块但仍偏大
4. **`_backup_spec/`**：.gitignore 忽略的旧版组件备份（tech-pink 主题 CSS + 旧 App/AssetOverview），无引用，保留作为历史快照
5. **视觉交接**：UI 视觉修改已交接给视觉 AI（遗留"流量卡广告"排查 ≈ 已解决，README 广告已移除）

---

## 十一、开发工作流

```bash
pnpm install                        # 装前端依赖
pnpm tauri dev                      # 桌面 app（Rust 编译 5-15 分钟）
pnpm -C apps/desktop dev            # 仅前端 + mock
cargo test --workspace              # Rust 全量测试（需 DISKPILOT_NO_ADMIN=1）
cargo run -p diskpilot-scaffold-lint -- scaffolds/<id>.toml   # scaffold 红线 lint
```

新增 scaffold 的完整 14-phase 工作流见 [development.md](../development.md) 的「新增/修改 scaffold」章节（需求文档 → 真实路径勘测 → TOML → safety test → UI 目视 → PR）。

---

## 十二、给接手 AI 的阅读路线

1. **先读** `CLAUDE.md`——仓库级硬约束（红线、safety test 强制、UI 防御三件套、隐私铁律）
2. **理解核心** `crates/scaffold/src/lib.rs`（schema + detect 逻辑）+ `scaffolds/wechat-pc.toml`（最复杂样例）
3. **看后端接线** `apps/desktop/src-tauri/src/lib.rs`（scan_path → suggestions_from_tree → tag_and_truncate → cleanup_suggestions 这条链是整个 app 的心脏）
4. **看前端对应** `apps/desktop/src/types.ts`（schema 镜像）+ `components/Studio.tsx`（scaffold 渲染）
5. **性能基线**：C 盘 189 万文件 / 31 万目录，walk ~140s + tag 28s = 总 ~169s；任何改动跑一次 `scan_path` 日志对比 `cmd_ms/tag_ms` 三阶段
6. **测试基线**：`cargo test --workspace`（118 绿）+ `tsc --noEmit`（0 错）+ `vitest run`（33 绿）
