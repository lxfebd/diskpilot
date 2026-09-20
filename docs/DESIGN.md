# DiskPilot 详细设计方案

> 本文档基于真实代码（2026-09-07，workspace `diskpilot_src`）核对后修正，
> 与 `docs/ARCHITECTURE_FOR_AI.md` 互为补充：后者讲整体架构与演进，本文
> 讲模块接口、数据结构、流程、安全、性能与规范。所有结构体/函数名、字段、
> 数值均与代码一致，可直接作为开发、迭代与 AI 协作的基准设计文档。

---

## 一、总体设计总则

### 1.1 设计目标与核心原则

- **安全第一**：宁可漏删不可错删，所有破坏性操作必须有静态校验、运行时确认、可撤销三层保障。
- **性能优先**：磁盘 IO 是瓶颈，所有逻辑优先走内存计算，尽可能减少重复遍历和随机 IO。
- **离线优先**：全部数据本地存储，AI 功能支持本地模型（Ollama），无强制联网依赖。
- **解耦可测**：Rust crate 之间零横向依赖（6 个叶子 crate + toolbelt 均为独立 crate），每个模块可独立测试、独立替换。

### 1.2 分层架构全景

从下到上共 5 层，层间单向依赖，无反向调用：

| 层级 | 职责 | 交互方式 |
|---|---|---|
| 系统层 | 操作系统 API、文件系统、硬件信息 | Rust 原生调用 + windows-sys 绑定 |
| 核心能力层 | 扫描、脚本匹配、执行、AI、工具箱 | Rust crate 库函数调用 |
| IPC 层 | 前后端通信、全局状态、并发调度 | Tauri Command + 状态共享 |
| 前端表现层 | UI 渲染、交互、状态管理 | React 组件 + Zustand |
| 用户交互层 | 拖拽、点击、对话、确认流程 | 事件驱动 + 异步调用 |

### 1.3 模块解耦原则

- 叶子 crate 之间禁止直接依赖，公共能力通过各自 crate 的 `pub` 接口或 desktop 组装层下沉。
- 前端不包含任何业务逻辑，仅做渲染与交互，所有计算全部下沉到 Rust 端。
- 所有跨模块调用通过明确的接口（trait/函数签名），禁止隐式依赖实现细节。

---

## 二、后端核心 Crate 详细设计

### 2.1 scanner 磁盘扫描器

**核心定位**：只读遍历文件系统，输出标准化目录树，支持取消与进度回调。

**核心数据结构**

```rust
pub struct Node {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,               // 子树总字节数（递归聚合）
    pub file_count: u64,         // 子树文件总数
    pub children: Vec<Node>,     // 子节点
    pub scaffold_id: Option<String>, // 扫描后回填的清理脚本标签
    pub top_extensions: Vec<ExtShare>, // 扩展名占比 Top 8
}

pub struct ExtShare {
    pub ext: String,
    pub bytes: u64,
    pub count: u64,
}

pub struct ScanOptions {
    pub follow_symlinks: bool,
    pub max_depth: Option<usize>,
    pub keep_files_per_dir: Option<usize>, // 默认 500
}

pub struct ScanProgress {
    pub files_seen: u64,
    pub bytes_seen: u64,
    pub current_path: String,
}

pub struct ScanStats {
    pub mode: String,            // "mft" | "walkdir"
    pub mft_attempted: bool,
    pub mft_succeeded: bool,
    pub mft_ms: u64,
    pub walk_ms: u64,
    pub build_tree_ms: u64,
    pub total_ms: u64,
    pub files_seen: u64,
    pub bytes_seen: u64,
    pub dirs_in_acc: u64,
}
```

> 注：Node 的 `path` 是 `String`（非 `PathBuf`），因为要直接序列化给前端。

**对外接口**

```rust
/// 可取消的带统计扫描，主入口（返回树 + 分阶段耗时）
pub fn scan_with_stats_cancellable<P, F>(
    root: P,
    opts: ScanOptions,
    cancel: Option<&AtomicBool>,
    on_progress: F,
) -> anyhow::Result<(Node, ScanStats)>
where P: AsRef<Path>, F: Fn(&ScanProgress) + Send + Sync;

/// 简单入口（不取消、不要统计）
pub fn scan<P: AsRef<Path>>(root: P) -> anyhow::Result<Node>;

/// 轻量路径采样（供 AI 上下文用，最多 max_depth=3）
pub fn sample_paths<P: AsRef<Path>>(root: P, n: usize) -> Vec<String>;
```

**内部执行流程**

1. **NTFS MFT 快速路径（Windows 且根在 NTFS 卷）**：尝试 `mft::scan_volume` 直读 MFT。返回空树（BitLocker/权限/非标准 NTFS）或 panic（非 NTFS）时记录警告并落入 walkdir 兜底。
2. **walkdir 主路径**：jwalk 并行遍历，`process_read_dir` 阶段用黑名单剪枝子目录，并顺带把每个目录的子目录名收集进 `subdirs` map（内存重建树用，避免二次 `read_dir`）。
3. **并行消费**：`par_bridge` + 每线程分片 `DirAcc`，按文件逐级累加 size/file_count/ext 统计，结束后合并分片。进度节流 ~5000 文件或 500ms，取消标志每 5000 文件检查一次。
4. **build_tree**：用 `subdirs` + `accs` 纯内存重建树，文件按 top-K（`push_file` 保序）取最大 N 个。
5. 结果按 size 降序排列子节点。

**错误与边界**

- 取消：返回 `scan:cancelled: ...` 错误（前端当普通中断处理，不弹错误 toast）。
- 权限拒绝：跳过并 WARN，不终止整体扫描。
- 根路径不存在：`build_tree` 返回 size=0 的空树（由调用方处理）。
- 系统剪枝黑名单（`walker.rs`，scanner 与 desktop 共用同一份）：`$recycle.bin`、`system volume information`、`.trash`、`.trashes`、`winsxs`；`installer` 仅在父目录为 `windows` 时剪枝。另有系统文件黑名单：`hiberfil.sys`、`pagefile.sys`、`swapfile.sys`。

### 2.2 scaffold 清理脚本引擎（核心）

**核心定位**：解析 TOML 脚本，编译 glob 规则，匹配目录是否命中清理范围。

**核心数据结构**

```rust
pub struct Scaffold {
    pub id: String,
    pub name: String,
    pub homepage: Option<String>,
    pub risk: Risk,              // low / medium / high
    pub disclaimer: String,
    pub detect: Vec<String>,     // 软件存在检测路径（glob）
    #[serde(rename = "match")] pub matcher: Match, // 目录匹配规则
    pub scopes: Vec<Scope>,      // TOML 用 [[scope]]，序列化给前端为 "scopes"
}

pub struct Match {
    pub name_contains: Vec<String>,   // 目录名包含片段
    pub must_have_child: Vec<String>, // 必须存在的子目录名
}

pub struct Scope {
    pub id: String,
    pub label: String,
    pub glob: String,
    pub mode: Mode,              // recycle（默认）/ quarantine / delete
    pub prompt: Option<Prompt>,  // none / days / bytes / choice / confirm
    pub category: Option<String>, // "cache"(默认) | "media" | "backup" | "envs"
    pub variant: Option<String>, // 版本标签（如 WeChat 3.x/4.x），未检测到时 UI 隐藏该 scope
    pub recycle_granularity: RecycleGranularity, // file(默认) / directory
}

pub enum RecycleGranularity { File, Directory } // serde lowercase

/// 编译后的可执行规则集
pub struct CompiledScaffold {
    pub id: String,
    detect_globs: globset::GlobSet,
    name_fragments_lc: Vec<String>,
    must_have_child: Vec<String>,
}
```

> 注：文档初稿把 `Match` 写成 `MatchRule`、`Risk` 写成 `RiskLevel`、`ActionMode` 与 `ScopeCategory` 等——真实代码中是 `Match` / `Risk` / `Mode`，`category` 是 `Option<String>` 而非枚举。本文以真实为准。

**对外接口**

```rust
pub fn parse_toml(s: &str) -> anyhow::Result<Scaffold>;
pub fn compile_all(scaffolds: &[Scaffold]) -> Vec<CompiledScaffold>; // 编译全部（坏 glob 跳过不致命）
pub fn detect_compiled(
    compiled: &[CompiledScaffold],
    path: &Path,
    existing_children: Option<&[String]>, // 内存子目录名列表，避免磁盘 IO
) -> Option<String>;                      // 命中返回 scaffold id
pub fn expand_env(s: &str) -> String;     // %VAR% / $VAR / ${VAR} 展开
```

**核心匹配逻辑**

1. **环境变量展开**：`expand_env` 支持 `%VAR%`、`$VAR`、`${VAR}` 三种格式，优先当前进程环境，缺失回退默认值。
2. **双重匹配**：先 `detect_globs`（`literal_separator(false)` + 大小写不敏感）命中，或 `name_contains`（目录名小写包含）且 `must_have_child` 全部满足；两者同时满足才算命中。
3. **性能优化点**：`must_have_child` 优先用上层传入的内存子目录列表（`existing_children` 内 `eq_ignore_ascii_case` 匹配），只有列表缺失时才回退 `path.join(child).exists()` 磁盘调用。
4. **错误隔离**：单个 scope/detect 的 glob 编译失败仅跳过该条（WARN），不导致整个 scaffold 失效。

### 2.3 executor 安全执行器

**核心定位**：统一执行文件操作，记录撤销日志，保证操作可回滚。

**核心数据结构**

```rust
pub enum Action { Recycle, Quarantine, Delete } // serde lowercase

pub struct Plan {
    pub action: Action,
    pub paths: Vec<PathBuf>,
    pub reason: String,
}

pub struct UndoEntry {
    pub timestamp: String,       // RFC3339 UTC
    pub action: Action,
    pub source: PathBuf,
    pub destination: Option<PathBuf>, // 仅 Quarantine 有值
    pub reason: String,
}
```

> 注：真实 UndoEntry 是 `destination: Option<PathBuf>`（非必填），没有 scaffold_id/scope_id 字段。

**对外接口**

```rust
pub fn execute(
    plan: &Plan,
    dry_run: bool,
    undo_log: &Path,
    quarantine_root: &Path,
) -> anyhow::Result<Vec<UndoEntry>>;

pub fn list_undo(undo_log: &Path) -> anyhow::Result<Vec<UndoEntry>>;

pub fn restore_quarantined(entry: &UndoEntry, quarantine_root: &Path) -> anyhow::Result<PathBuf>;

pub fn remove_restored(undo_log: &Path, restored_sources: &[PathBuf]) -> anyhow::Result<()>;

pub fn protected_path(p: &Path) -> Option<&'static str>; // 盘根/系统保留目录/主目录根
```

**执行与撤销流程**

1. **执行前守卫**：任何动作（包括 recycle/quarantine）先逐路径调 `protected_path`，命中盘根、系统保留目录、用户主目录根则直接 `bail!`（不可逆误操作零容忍）。
2. **执行阶段**：
   - dry_run：仅统计命中路径和大小，不实际操作。
   - Recycle：`trash::delete`，带已知 Windows 误报兜底（`IFileOperation` 误报 "operations aborted" 但路径已消失 → 按成功；仍在则回退 `SHFileOperationW`，最终以「路径是否消失」判定）。
   - Quarantine：移动到 `quarantine_root/时间戳/...`，保留原路径结构，写入 destination。
   - Delete：永久删除（调用方负责二次确认）。
   - 所有操作成功后追加写入 `undo.jsonl`（每行一条 JSON，追加原子性）。
3. **撤销阶段**：`list_undo` 读条目 → `restore_quarantined` 从隔离区移回 → `remove_restored` 清理已还原条目的日志记录。Recycle 由系统回收站还原，Delete 无法撤销（UI 上明确标识）。

### 2.4 advisor AI 顾问

**核心定位**：多 Provider 抽象，结构化输出，隐私数据过滤。

**核心数据结构**

```rust
pub struct ExtShare {
    pub ext: String,
    pub share: f32, // 占比
}

pub struct AdvisorRequest {
    pub path: String,
    pub size_bytes: u64,
    pub file_count: u64,
    pub top_extensions: Vec<ExtShare>,
    pub sample_paths: Vec<String>,   // 最多 20 条样本路径
    pub neighbors: Vec<String>,      // 同级目录名
    pub scaffold_hint: Option<String>,
}

pub struct AdvisorResponse {
    pub what: String,
    pub category: String,            // browser_cache|app_cache|package_cache|build_artifact|game_data|user_content|system|model_weights|unknown
    pub safe_to_delete: bool,
    pub risk: String,                // low|medium|high
    pub action: String,              // keep|recycle|delete|custom
    pub reasoning: String,
    pub needs_inspection: bool,
    pub suggested_scaffold: Option<String>,
}

/// Provider 是枚举（非 trait）
pub enum Provider {
    OpenAI { api_key: String, model: String, base_url: String },
    Anthropic { api_key: String, model: String, base_url: String },
    Ollama { base_url: String, model: String },
    Gemini { api_key: String, model: String, base_url: String },
}
```

**隐私与安全规则**

- 绝不发送文件内容：仅发送路径、大小、文件数、扩展名、样本路径名等元数据。
- 路径脱敏：发送给 AI 的路径自动把用户主目录替换为 `$HOME`（前端 `advisorClient.ts` 做）。
- JSON mode 强制开启：system prompt 要求严格 JSON 输出并给 schema，解析失败则重试/降级。
- 工具白名单：AI 可用 `list_dir`、`path_size`、`get_disk_health`、`get_cleanup_suggestions`、`get_system_info`、`web_search`、`run_cli_tool`（中/高风险需用户确认）、`get_tool_manifest`/`get_cli_tool_usage` 等。文件写入/删除只能走「生成待确认清单 → 用户逐项勾选确认 → execute_ai_plan」，AI 永远不能直接删除。

### 2.5 scaffold-lint 静态校验器

**核心定位**：CI 阶段校验 scaffold 脚本的安全性，防止危险规则合并。

**校验项**

1. TOML 可解析为 `Scaffold`（schema 检查），scope 为空则 WARN。
2. 每个 scope 有非空 glob。
3. **Scope glob 红线**：任何 scope glob 不得匹配以下 probe 路径（命中即硬错误，收紧 glob 而非放宽检查）：
   - 数据库文件：`*.db` / `*.db-wal` / `*.db-shm`
   - 聊天记录：`**/db_storage/**`（WeChat 4.x DB 簇）、`**/Msg/**`、`**/MultiMsg/**`（WeChat 3.x 聊天数据）
   - 账号信息：`**/Accounts/**`、`**/login/**`、`**/config/**`
   - 机器级状态：`**/All Users/**`
   - 用户收藏：`**/Favorite*/**`、`**/Fav/**`
   - 加密物料：`**/key/**`、`**/crypto/**`

**命令行调用**

```bash
cargo run -p diskpilot-scaffold-lint -- scaffolds/*.toml
# 0 error 通过，非0 失败并输出错误详情
```

> 注：初稿红线含 `**/User Data/**`，真实实现是 `**/All Users/**`（机器级状态）；`**/Fav/**`、`**/key/**` 等均在 RED_LINE_SAMPLES 内。测试统一 `USERPROFILE=C:/Users/test` 等确定性环境变量。

### 2.6 steam-inspector Steam 盘点

**核心定位**：只读解析 Steam 配置文件，输出游戏清单与占用。

**核心能力**

- 解析 `libraryfolders.vdf` 获取所有游戏库目录（`parse_libraryfolders`）。
- 解析每个游戏的 `appmanifest.acf` 获取名称、大小、安装时间、最后运行时间（`parse_appmanifest`）。
- 输出游戏列表、总占用、最近游玩排序（`inspect_at` / `inspect_at_with_clock`）。
- `steam_root_candidates` + `discover_steam_root` 自动定位 Steam 根。

**约束**

- 只读不写，不修改任何 Steam 文件；不调用 Steam API，全部本地解析；解析失败返回空列表，不崩溃。

### 2.7 toolbelt 工具箱集成

**核心定位**：统一管理第三方 CLI 工具，文档驱动，权限分级。

**核心数据结构**（`crates/toolbelt/assets/tool_manifests.json`，编译期 include_str!，共 **30 个工具**）

```json
{
  "name": "Prime95",
  "category": "处理器工具",
  "exe_rel": "处理器工具/Prime95/prime95.exe",
  "purpose": "...",
  "when_to_use": "...",
  "when_not_to_use": "...",
  "invocation": { "mode": "cli|gui", "launchable": "...", "args_template": "...",
                  "params": [{"name","type","required","default","desc"}], "timeout_secs": 30 },
  "output": { "format": "csv|json|stdout", "parser": "smart_csv|kv_lines|stdout", "schema": "..." },
  "risk": "low|medium|high",
  "permission_level": "L0|L1|L2|L3",
  "side_effects": "...",
  "examples": [{"args","desc","expect"}]
}
```

**设计原则**

- 文档即目录：工具清单由 `cli_tools_doc.md` 半自动转写为 `tool_manifests.json`，不维护第二份参数表。
- 统一执行入口：`toolbelt_run` / `toolbelt_launch`（GUI 与 CLI 分离），统一超时控制、权限校验、日志记录。
- 超时强杀：默认 30s 超时，高危工具更短，避免挂起。
- 输出解析器：`smart_csv`（WizTree/Autoruns/USBDeview 带表头 CSV → JSON）、`kv_lines`（HWiNFO/BatteryInfoView）、`stdout` 原样，绑定 manifest `output.parser`。

---

## 三、Tauri IPC 层详细设计

### 3.1 AppState 全局状态设计

```rust
struct AppState {
    scaffolds: Mutex<Vec<Scaffold>>,          // 静态配置，启动时加载
    advisor: Mutex<Option<Provider>>,         // 当前 AI Provider
    quarantine_root: PathBuf,                 // app_data_dir()/quarantine
    undo_log: PathBuf,                        // app_data_dir()/undo.jsonl
    scan_cancel: Arc<AtomicBool>,             // 扫描取消标志
    cleanup_cache: Mutex<HashMap<String, CleanupCacheEntry>>, // 清理建议缓存（key=归一化盘根）
    scan_tree: Mutex<Option<Node>>,           // 最近一次完整扫描树
    hw_stop: Arc<AtomicBool>,                 // 硬件压测停止开关
}
```

**并发设计说明**

- 复杂结构体用 `Mutex` 包裹，单次加锁时间尽量短；布尔标志用 `AtomicBool` 无锁。
- `scan_tree` 保留完整未截断树，供 `cleanup_suggestions` / AI 总览 / dry-run 在内存树秒算，避免重复全盘 walk。
- `cleanup_cache` 以归一化盘根为 key（`norm_path_key`：小写 + `/` 分隔），扫描完成写入，切页秒回。
- `CleanupCacheEntry { at: Instant, suggestions: Vec<CleanupSuggestion> }`，`at` 仅作新鲜度参考。

### 3.2 核心 Command 接口规范

所有 Command 统一错误格式 `Result<T, String>`（可执行器错误用 `anyhow` → `to_string` 透出）。实际注册的命令（按 invoke_handler 顺序）：

| 命令 | 入参 | 返回值 | 职责 |
|---|---|---|---|
| `scan_path` | path, keep_files_per_dir? | Node | 扫描路径返回截断树 + 填缓存 |
| `cancel_scan` | 无 | () | 置 scan_cancel |
| `estimate_size` | path | u64 | 估算目录大小 |
| `list_scaffolds` | 无 | Vec\<Scaffold\> | 全部清理脚本 |
| `detect_scaffold` | path | Option\<String\> | 单路径检测 |
| `scope_sizes` / `scope_sizes_batch` | ... | Vec | scope 大小统计 |
| `cleanup_suggestions` | root_path | Vec\<CleanupSuggestion\> | 清理建议列表 |
| `chat_scan_context` | top_n? | JSON | AI 总览上下文 |
| `execute_scope` | scaffold_id, scope_id, root_path, dry_run, ..., confirmed | Vec\<UndoEntry\> | 执行单个清理项（非 dry-run 需 confirmed） |
| `list_conda_envs` | conda_root | Vec\<CondaEnv\> | Conda 环境列表 |
| `ai::advise` | AdvisorRequest | AdvisorResponse | AI 目录咨询 |
| `inspect_path` | path, sample_count | Vec\<String\> | 路径采样 |
| `reveal_in_explorer` | path | () | 资源管理器定位 |
| `recycle_paths` | paths, reason, confirmed | Vec\<UndoEntry\> | 显式路径移入回收站（后端强制 Recycle + 确认门） |
| `execute_ai_plan` | paths, user_confirmed, dry_run | Vec\<UndoEntry\> | AI 建议执行（确认门 + cleanup.execute 权限门） |
| `list_undo` / `undo` | limit / index | Vec / bool | 撤销历史与撤销 |
| `ai::set_advisor` / `web_search` / `ai_proxy` | ... | ... | AI 配置与联网 |
| `volume_info` / `list_drives` | path / 无 | VolumeInfo / Vec | 磁盘信息 |
| `steam::list_steam_games` / `list_steam_workshop_items` / `fetch_workshop_titles` / `open_steam_url` | ... | ... | Steam 盘点 |
| `general_config` / `set_general` / `set_tools_root` | ... | ... | 通用配置 |
| `toolbelt::toolbelt_status` / `toolbelt_catalog` / `toolbelt_usage` / `toolbelt_run` / `toolbelt_manifests` / `toolbelt_launch` / `plugin_uninstall` / `plugin_install` / `plugin_market` | ... | ... | 工具箱 |

### 3.3 核心执行链路设计

**scan_path 全链路**

1. 前端调用 → Tauri 接收 → 重置 `scan_cancel=false`。
2. 在 spawn_blocking 外先 `compile_all` + 克隆 raw Scaffold（避免把 Mutex 带进线程）。
3. `spawn_blocking` 内调 `scanner::scan_with_stats_cancellable`，进度回调 emit `scan-progress` 事件。
4. 扫描完成后：先 `suggestions_from_tree`（必须在前）→ `tag_and_truncate`（打标签 + 深度截断 100/50/20）。
5. 外层把 suggestions 写入 `cleanup_cache`，完整树（truncate 前克隆）写入 `scan_tree`，emit `scan-stats`（含 cmd_ms/tag_ms），返回截断树。

**cleanup_suggestions 优化链路**

1. 归一化 root_path 为 cache key，命中 `cleanup_cache` 直接返回。
2. 未命中则查 `scan_tree`，树存在且包含该路径则在内存树重算。
3. 都不存在才发起新的目录扫描（`find_matching_dirs` walk）。

---

## 四、前端架构详细设计

### 4.1 模块边界与依赖

```
src/
├── api.ts          # 底层 Tauri 调用封装，唯一与后端交互层
├── store.ts        # 全局状态（zustand），业务逻辑层，调用 api
├── types.ts        # 类型定义，前后端 schema 镜像
├── theme.ts        # 主题系统（17 套预设 + 动态 accent + 字体缩放）
├── permissions.ts  # 权限分级 L0-L3
├── advisor/        # AI 客户端（provider.ts / agent.ts / tools.ts / chatHistory.ts）
├── components/     # 纯渲染组件，从 store 取数据
└── App.tsx         # 布局组装
```

依赖方向：组件 → store → api → types，禁止反向依赖。

### 4.2 状态管理设计（zustand）

按领域拆分切片：扫描（scanTrees LRU 最多 6 盘符 / currentPath / scanning / progress）、清理（suggestions / selectedScopes / executing）、聊天（messages / conversationId / loading）、设置（theme / aiConfig / permissions）。

关键优化：

- `recyclePaths` / `aiRecyclePaths` 批量剪枝：用 `tooltree.pruneMany` + `treeMayContain`（大小写不敏感前缀匹配）一次遍历删除多个节点，避免 N 次全树重建。
- `treeMayContain(root, p)`：路径前缀判断快速定位节点。

### 4.3 核心组件设计

- **TreeView**：基于 `@tanstack/react-virtual` 虚拟滚动，仅渲染可视区；行含展开箭头、图标、目录名、大小、占比条、scaffold 标签；支持键盘导航、右键菜单、拖拽到聊天框。
- **Studio 清理工作台**：按 category 分组（media 置顶 / cache 合并按钮 / backup 置底 / envs），卡片含风险标签、scope 列表、可清理大小、dry-run；支持全选、按分类选择、天数筛选。
- **ChatPanel AI 对话**：拖拽目录/文件到输入框自动组装元数据，流式渲染，AI 回复中的清理建议可一键执行（走确认流程），历史持久化 localStorage。
- **CleanupModal / CleanupProposalDialog**：清理确认弹窗，dry-run 结果预览 + 最终确认，默认 Recycle。
- **SteamInspector / SteamWorkshopModal**：Steam 游戏/工坊盘点。
- **Toolbelt / PermissionCenter**：工具墙（12 分类）+ 权限中心。

### 4.4 主题系统设计

- **17 套预设**（theme.ts `PRESETS`），全部 CSS 变量实现，切换仅改 `<html>` 属性。
- 语义化变量：`--bg`、`--surface`、`--text-primary`、`--text-secondary`、`--accent`、`--ok`、`--warning`、`--danger` 等。
- 动态 accent：暗色模式自动提亮 20%（`lighten` 逻辑按感知亮度判定）保证对比度。
- 字体联动：每套主题对应 ui/editor 字体族，支持 0.8–1.3 倍缩放。

---

## 五、核心业务流程全链路设计

### 5.1 磁盘扫描主流程

```
用户点击扫描 → 前端设置 loading → 调用 scan_path
    → Tauri 接收 → 重置取消标志 → MFT 快速路径尝试（失败落 walkdir）
    → jwalk 并行遍历 → 黑名单剪枝 → 并行分片聚合
    → 内存重建树 → 进度回调（~5000 文件/500ms）→ 检查取消
    → 扫描完成 → 计算清理建议（suggestions_from_tree）→ 写 cleanup_cache
    → 打标签 + 截断（tag_and_truncate）→ 完整树存 scan_tree
    → 返回前端
→ 前端更新树 → 渲染 TreeView + Treemap → 关闭 loading
```

### 5.2 清理建议生成流程

```
进入 Studio 页面 → 调用 cleanup_suggestions
    → 查询 cleanup_cache → 命中则直接返回
    → 未命中 → 读 scan_tree 内存树 → suggestions_from_tree 重算
    → 都不存在才重新全盘 walk
    → 组装建议列表（按字节降序）→ 写缓存
→ 前端按分类渲染卡片
```

### 5.3 清理执行与撤销流程

```
用户点击清理 → CleanupModal 确认框 → 显示 dry-run 结果
    → 用户确认 → 调用 execute_scope（默认 Recycle）
        → 后端 protected_path 守卫 → 遍历命中路径
        → 移入回收站/隔离区 → 追加写 undo.jsonl
    → 返回执行结果 → 前端 pruneMany 剪枝更新树
→ 操作完成 → 提示成功 + UndoPanel 撤销入口
用户点击撤销 → 调用 undo → 读取 undo 条目 → 还原 → 清理日志
→ 前端刷新列表
```

### 5.4 AI 顾问交互流程

```
用户拖拽目录到聊天框 → 前端组装 AdvisorRequest（元数据 + $HOME 脱敏）
    → 调用 ai::advise → 后端路由到对应 Provider（OpenAI/Anthropic/Ollama/Gemini）
    → 发送元数据（不发文件内容）→ JSON mode 结构化输出
    → 解析结果 → 返回前端
→ 前端渲染回复 → 展示清理建议按钮（生成待确认清单）
用户逐项勾选确认 → execute_ai_plan → 走清理执行流程
```

---

## 六、Scaffold 脚本系统规范

### 6.1 TOML 完整字段规范

| 字段 | 必填 | 说明 | 约束 |
|---|---|---|---|
| id | 是 | 唯一标识 | 小写+下划线，全局唯一 |
| name | 是 | 显示名称 | 简洁 |
| risk | 是 | 风险等级 | low/medium/high |
| disclaimer | 是 | 免责声明 | 至少说明删除后果 |
| detect | 是 | 检测路径 | 至少 1 条，支持环境变量 |
| match.name_contains | 是 | 目录名匹配 | 至少 1 个片段（可为空但建议给） |
| match.must_have_child | 否 | 子目录校验 | 建议必填，防误匹配 |
| scope.id | 是 | 清理项 ID | scaffold 内唯一 |
| scope.label | 是 | 显示名称 | 简洁清晰 |
| scope.glob | 是 | 匹配规则 | 禁止裸 `**` 前缀，必须有明确锚点 |
| scope.mode | 否 | 执行模式 | 默认 recycle |
| scope.category | 否 | 分类 | cache(默认)/media/backup/envs |
| scope.recycle_granularity | 否 | 回收粒度 | 默认 file；海量小文件用 directory |
| scope.prompt | 否 | 询问参数 | none/days/bytes/choice/confirm |
| scope.variant | 否 | 版本标签 | 未检测到时 UI 隐藏该 scope |

### 6.2 红线规则（强制禁止）

任何 scope glob 不得匹配以下路径族（scaffold-lint `RED_LINE_SAMPLES`）：

- 数据库文件：`*.db`、`*.db-wal`、`*.db-shm`、聊天库 `**/db_storage/**`
- 聊天记录：`**/Msg/**`、`**/MultiMsg/**`
- 账号信息：`**/Accounts/**`、`**/login/**`、`**/config/**`
- 机器级状态：`**/All Users/**`
- 用户收藏：`**/Favorite*/**`、`**/Fav/**`
- 加密物料：`**/key/**`、`**/crypto/**`

### 6.3 编写与校验流程

1. 需求确认：明确软件版本、缓存目录、红线目录。
2. 实地勘测：在目标机器上确认路径结构。
3. 编写 TOML：遵循字段规范，glob 尽量精准。
4. 运行 lint：`scaffold-lint` 检查红线与语法。
5. 编写 safety 测试：正向用例 + 红线反向用例（模板 `crates/scaffold/tests/_templates/scaffold_safety.rs`，expand() 需支持 `%VAR%` 和 `${VAR}`）。
6. 真机验证：dry-run 确认命中范围正确。
7. 提交 PR：CI 全量测试通过后合并。

---

## 七、安全体系详细设计

### 7.1 静态防御层（第一道防线）

- scaffold-lint：CI 阶段自动检查所有脚本，红线 0 容忍（probe 路径命中即硬错误）。
- safety 集成测试：每个 scaffold 配套正反用例（正向至少命中一个 scope + 红线零命中）。
- 代码评审：新增 scaffold 双人评审，glob 只收窄不放宽。

### 7.2 运行时防御层（第二道防线）

- 两步确认：破坏性操作必须经过 dry-run 预览 + 最终确认（CleanupModal/CleanupProposalDialog），禁止一键直接执行。
- 默认安全：默认执行模式 Recycle（回收站可恢复），Delete 需手动切换并二次确认。
- 权限分级：L0 只读 / L1 单次确认 / L2 二次确认 / L3 默认禁用（PermissionCenter + executor 后端 risk 门双保险）。
- 路径守卫：executor 对任何动作先 `protected_path`，盘根/系统目录/主目录根直接拒绝。
- 组件守卫：破坏性操作组件包裹 ErrorBoundary，异常不静默。

### 7.3 可逆防御层（第三道防线）

- 回收站兜底：默认全部走系统回收站，用户可手动还原。
- 隔离区机制：Quarantine 保留完整路径结构，undo 可一键还原。
- undo 日志：所有操作可追溯、可撤销，追加写不可篡改；每条记录含时间、原因。
- AI 只出清单：AI 生成待确认清单 → 用户逐项勾选 → 才执行，AI 无直接删除能力。

### 7.4 隐私保护

- 零云存储：所有数据本地存储，无账号、无上传、无遥测。
- AI 数据最小化：只发目录元数据，绝不读文件内容。
- 路径脱敏：发给 AI 的路径自动替换用户主目录为 `$HOME`。
- 本地模型支持：Ollama 本地部署，数据完全不出本机。

---

## 八、性能设计与优化方案

### 8.1 当前优化点原理

**优化 1：must_have_child 内存化**

- 问题：原实现每个目录的 `must_have_child` 校验都 `path.join(child).exists()`，大量随机磁盘 IO。
- 方案：扫描目录时已获取子目录名列表（`subdirs` map），经 `detect_compiled(…, Some(&child_names))` 内存字符串匹配。
- 收益：tag 阶段 229s → 10.7s（约 21 倍）。

**优化 2：文件节点短路**

- 问题：原实现遍历所有文件节点做 glob 匹配，但所有现有 scope 都是目录粒度（`recycle_granularity="directory"`），文件匹配完全无效。
- 方案：文件节点直接跳过循环（仅目录节点做 glob 匹配）。
- 收益：建议计算 152.7s → 17.3s（约 8.8 倍）。

**优化 3：scan_tree 内存树复用**

- `scan_path` 完成后把完整树存 `AppState.scan_tree`，`cleanup_suggestions`/AI 总览/dry-run 目录匹配都在内存树秒算，不再每次切页全盘 walk（修「切页面就重扫」bug）。

### 8.2 性能基线（C 盘 189 万文件 / 31 万目录，实测）

| 指标 | 数值 | 说明 |
|---|---|---|
| MFT 快速路径 | 秒级 | NTFS 卷直读 MFT（失败落 walkdir） |
| walk 遍历 | ~140s | 磁盘 IO 物理极限（walkdir 模式） |
| 树构建 | ~2s | 内存聚合 |
| 标签+截断 | ~10.7s | 内存匹配 |
| 建议计算 | ~17.3s | 内存计算 |
| 总耗时 | ~169s | 端到端（原 ~302s） |

### 8.3 后续优化路线

1. ~~P1：流式扫描（边扫边输出，前端渐进式渲染）~~ ✅ 已完成（2026-09-07：`scan-progress`/`scan-stats` 事件全量扫描期间持续 emit，前端 App.tsx 监听并驱动进度条 determinate/indeterminate 双态 + 「N 个文件 · X」实时文本）。
2. ~~P1：扫描祖先链 ext 复制优化~~ ✅ 已完成（2026-09-07：热路径再优化——祖先循环从 `entry(dir.to_path_buf())`（每文件每层克隆 PathBuf + 双哈希）改为 `get_mut` 借用快路径零拷贝，仅首遇某目录才 clone；新增 `ancestor_chain_shared_dirs_accumulate_exactly` 回归测试钉死深层共享祖先的 size/count/ext 精确累计）。
3. ~~P2：增量扫描（基于 USN 日志），无需每次全量~~ ✅ 已完成（2026-09-07：见 P1 中期清单第 2 项）。
4. ~~P2：按需加载子树（前端展开时才加载子目录）~~ ✅ 已完成（2026-09-07：完整树本就在 `state.scan_tree` 内存缓存里，无需二次扫盘——`tree_subtree(path)` 命令纯内存按路径取完整未截断子树；`tag_and_truncate` 截断时记录 `children_truncated` 原始数，TreeView 展开被截断目录时懒加载覆盖渲染，带「+N」徽标与加载态）。
5. ~~P2：scanner 与 walkdir 对齐 top-K 语义~~ ✅ 已完成（2026-09-07：mft 路径此前已对齐 walkdir「目录全保留 + 文件按大小 top-K + ext 覆盖全部直系文件」；新增 `keep_files_per_dir_top_k_semantics` 契约测试钉死三条不变式，防两条路径漂移）。

> 注：初稿的「NTFS MFT 直读整盘 2-5 秒」**已实现**（`scanner/src/mft.rs`，含空树守卫与 panic 兜底），从 P1 移到「已落地」；「P2：macOS/Linux 适配」是长期目标，非 P2 本期。

---

## 九、数据持久化设计

### 9.1 本地目录结构

```
%APPDATA%/dev.diskpilot.app/   # app_data_dir()（Tauri 默认，Windows）
├── undo.jsonl          # 撤销日志，追加写入
├── quarantine/         # 隔离区
│   └── 20260907_143022/ # 每次操作一个时间戳目录（RFC3339 命名）
│       └── 原路径结构...
└── ...（其他应用数据）
```

> 注：真实路径是 Tauri `app_data_dir()`（如 `%APPDATA%/dev.diskpilot.app/`），不是 `~/.diskpilot/`。config.toml/扫描缓存/日志目录是规划项，未实现。

### 9.2 关键文件格式

- undo.jsonl：每行一个 JSON 对象（UndoEntry），追加写原子性。
- quarantine：保留原始完整路径，便于还原。
- config：主题/AI/权限存 localStorage + Tauri 配置（`general_config`/`set_general`）。

---

## 十、测试与质量体系

### 10.1 分层测试策略

| 层级 | 工具 | 覆盖范围 | 通过率要求 |
|---|---|---|---|
| Rust 单元 | cargo test | 每个 crate 函数逻辑、边界条件 | 核心函数 100% |
| Rust 集成 | cargo test | scaffold 安全、端到端流程 | 所有 scaffold 必须有 safety 测试 |
| 前端类型 | tsc --noEmit | 类型安全 | 0 错误 |
| 前端单元 | vitest | 工具函数、状态逻辑 | 核心模块全覆盖 |
| 真机验证 | 手动 | 扫描、清理、撤销全流程 | 每个大版本必测 |

当前实测基线：cargo test --workspace 118 passed / 0 failed；tsc --noEmit 0 错误；vitest 33/33。

### 10.2 CI 流水线

代码提交 → 自动触发：cargo fmt → cargo clippy → cargo test --workspace → scaffold-lint → tsc --noEmit → vitest run → 构建验证。

---

## 十一、技术债与演进规划

**P0 近期（0.2 版本前）**

1. ~~补全 README，更新 scaffold 数量从 8 份到 12 份~~ ✅ 已完成（2026-09-07：docker-buildx / huggingface-cache / obs-cache / ide-caches 新增，README 表与说明同步）。
2. ~~拆分 desktop lib.rs~~ ✅ 已完成（2026-09-07：按 scan/cleanup/executor/conda/ai/hw/steam/system/toolbelt 子模块拆分，lib.rs 精简为入口+状态+例外；11 单测全绿）。
3. ~~拆分 styles.css~~ ✅ 已完成（2026-09-07：4803 行单文件 → styles/{tokens,layout,chat,steam,settings,overview,toolwall,dark}.css 8 文件，main.tsx 按原顺序 import；vite build 产物合并后 113.77 kB 无变化）。
4. 清理 _backup_spec 遗留代码（已被 .gitignore 忽略；按安全铁律保留不删，仅作历史快照，见 docs/ARCHITECTURE_FOR_AI.md）。
5. ~~工具墙：tool_manifests.json 覆盖 19 个工具~~ ✅ 已完成（2026-09-07：19 个工具五段 schema + smart_csv/kv_lines 解析器 + 权限 L1-L3 + 测试；2026-09-07 P1-3 扩充至 30 个，新增 11 个真实磁盘工具：CPU-Z/CoreTemp/ThrottleStop/GPU-Z/DDU/CrystalDiskMark/HDTune/SpaceSniffer/HWMonitor/Dism++/Rufus，其中 10 个 GUI 工具 mode=gui（仅启动不可脚本化），CLI 20 个）。

**P1 中期**

1. ~~流式扫描（边扫边输出渐进渲染）~~ ✅ 已完成（2026-09-07：`scan-progress` 事件实时推送 files_seen/bytes_seen/current_path，前端进度条 determinate/indeterminate 双态渐进渲染）。
2. ~~增量扫描与 USN 日志支持~~ ✅ 已完成（2026-09-07：`scanner/src/usn.rs` USN Journal 增量扫描——query_journal 有效性校验 + FRN 表构建 + read_changes 差异 + 仅变更路径重新 stat + 缓存树增量 merge；`scan_with_usn` 公开 API；2026-09-07 桌面接线：`scan_path_usn` Tauri command（Windows）优先增量、失败自动回退全量并重记基线游标，全量扫描后记录基线 cursor 到 `AppState.usn_cursors`，`api.scan` 自动走增量、cancel 期间可中断 FRN 枚举）。
3. ~~完善 toolbelt 工具数量到 30+~~ ✅ 已完成（2026-09-07：19 → 30，新增 CPU-Z/CoreTemp/ThrottleStop/GPU-Z/DDU/CrystalDiskMark/HDTune/SpaceSniffer/HWMonitor/Dism++/Rufus，10 个 GUI 工具 mode=gui）。
4. ~~新增 scaffold 脚手架 CLI~~ ✅ 已完成（2026-09-07：`diskpilot-scaffold-gen` 交互式生成器——问答式收集 id/name/risk/detect roots/cache buckets/policy，自动锚定 glob 到 detect roots、红线硬校验、生成 TOML + 安全测试骨架；`scaffold-lint` 重构为与生成器共享 `glob_hits_red_line` 单一红线清单）。

**P2 长期**

1. macOS/Linux 适配与预编译包。（长期目标，非本期，见 8.3 注）
2. ~~社区 scaffold 一键安装~~ ✅ 已完成基础（2026-09-07：`install_scaffold`（校验 toml → 写入 `app_data_dir/scaffolds/{id}.toml` → 热重载，id 防路径穿越白名单校验 + 单测）/ `uninstall_scaffold`（只删用户目录文件，内嵌脚本不可卸载）/ `scaffold_source`（embedded|user 查询）；前端 Studio 头部「+」安装弹窗（粘贴 toml）+ 用户安装卡片「卸载」按钮。仓库索引/自动拉取留待社区仓库就位后接入）。
3. 插件系统，支持第三方扩展。（toolbelt 的 `ToolPluginMeta`/`tool.plugin.json` 插件元数据模型已就位，标注「供插件市场使用」；完整实现——插件分发/下载/签名校验——需产品决策。scaffold 一键安装（上一项）是插件系统的最小先行者，复用同一套用户目录加载+热重载机制。）
4. 多语言国际化。（前端现为全硬编码中文 UI 文本，无 i18n 基础设施；完整实现需产品决策：目标语言/跟随系统/翻译维护流程。）
