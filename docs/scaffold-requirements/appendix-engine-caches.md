# 附录：游戏引擎缓存（Unity / Unreal Engine / Godot）

> 分类：游戏引擎开发缓存。
> 目标用户：使用 Unity / Unreal Engine / Godot 做游戏开发的人，磁盘被 DDC / 包缓存 / 编辑器日志塞爆后需要一个"一键清缓存但不误删项目"的入口。
> 关联 scaffold：[scaffolds/engine-caches.toml](../../scaffolds/engine-caches.toml)
> 关联 safety test：[crates/scaffold/tests/engine_caches_safety.rs](../../crates/scaffold/tests/engine_caches_safety.rs)

## 1. 勘测发现

### 1.1 勘测方法

严格遵循隐私铁律：**只用 `ls` 列目录名，绝不 `Read` 任何文件内容**（尤其不能 `Read` 聊天 DB / 缓存二进制 / 账号 key）。

### 1.2 本机勘测结论（Windows 10，用户 `31672`）

| 路径 | 状态 | 观察到的子项（仅列目录名） |
|---|---|---|
| `%LOCALAPPDATA%/Unity/` | 存在 | `Caches`、`Editor`、`cache`、`config`、`licenses` + 两个顶层 log 文件 |
| `%LOCALAPPDATA%/Unity/cache/` | 存在 | `npm`、`packages`（Unity 6 时代 UPM 包下载缓存，实测有 `packages.unity.cn` 镜像目录） |
| `%LOCALAPPDATA%/Unity/Editor/` | 存在 | `Editor.log`、`Editor-prev.log`、`upm.log`（三件套日志） |
| `%LOCALAPPDATA%/Unity/Caches/bee/` | 存在 | 大量 64 位十六进制文件（Bee 编译缓存，可再生但**本 scaffold 不清**——它比 cache/ 深一层，本任务只锚定 cache/ 与 Editor/） |
| `%LOCALAPPDATA%/Unity/config/` | 存在 | `production.json`（**红线**——引擎全局配置） |
| `%LOCALAPPDATA%/Unity/licenses/` | 存在 | `packages/` 子目录（**红线**——授权文件） |
| `%LOCALAPPDATA%/Unity/Unity.Licensing.Client.log` | 存在 | 顶层 log 文件（**红线**——本 scaffold 只清 Editor/ 子目录，不碰顶层 Licensing/Entitlements log） |
| `%LOCALAPPDATA%/Unity/Unity.Entitlements.Audit.log` | 存在 | 同上 |
| `%LOCALAPPDATA%/UnityHub/` | **不存在**（用户数据在 Roaming） | — |
| `%APPDATA%/UnityHub/` | 存在 | Chromium-based Electron 全套数据目录：`Cache`、`Code Cache`、`GPUCache`、`Cookies`、`Cookies-journal`、`Local Storage`、`Session Storage`、`blob_storage`、`Settings`、`logs/`、`graphqlCache/`、`Cache/js`、`Cache/wasm`、`hubConfig.json`、`cloudConfig.json`、`favoriteProjects.json`、`projectsArchitecture.json` 等 |
| `%APPDATA%/Unity Hub/` | 存在 | 只有一个 `logs/`（Unity Hub 旧版数据根） |
| `%APPDATA%/UnityHubWebGLHost/` | 存在 | WebGL 运行时二进制（**红线**——本体不是缓存） |
| `%APPDATA%/UnityMCP/` | 存在 | `customer_uuid.txt`、`milestones.json`（**红线**——授权/账号状态） |
| `%APPDATA%/unityhub-updater/` | 存在但空 | — |
| `%LOCALAPPDATA%/UnrealEngine/` | 存在 | 版本目录 `4.16`、`4.21`、`4.24`、`4.25`、`4.26`、`4.27`、`5.0`、`5.1`、`5.3`、`5.4`、`5.6` |
| `%LOCALAPPDATA%/UnrealEngine/Common/` | **不存在** | DDC 目录本机不存在（DDC 是运行时首次编译时创建的，本机只是装过引擎从未在编辑器里构建过） |
| `%LOCALAPPDATA%/UnrealEngine/<version>/Saved/Config/` | 存在（各版本都是） | `WindowsEditor/*.ini`（**红线**——引擎运行时的用户配置） |
| `%LOCALAPPDATA%/Godot/` | **不存在** | — |
| `%APPDATA%/Godot/` | **不存在** | — |
| `%PROGRAMFILES%/Unity Hub/` | 存在 | Hub 安装本体（**红线**——安装目录） |

**结论**：本机有 Unity 与 Unreal 的引擎数据（但 UE 的 DDC 从未被创建过），无 Godot 数据。Godot 相关 scope 与部分 UE scope 基于公开结构依据编写（见 §4）。

### 1.3 公开结构依据（本机未装部分）

| 引擎 | 目录 | 依据 |
|---|---|---|
| Unreal | `%LOCALAPPDATA%/UnrealEngine/Common/DerivedDataCache/` | 官方文档 <https://docs.unrealengine.com/en-US/Engine/Rendering/DerivedDataCache/index.html>：DDC 是引擎级全局派生数据缓存，存放 shader bytecode、cooked assets 等，重建无副作用，是 UE 用户最大可清缓存（动辄几十 GB） |
| Unreal | `%LOCALAPPDATA%/UnrealEngine/Common/DerivedDataCache_Partial/` | 同上，部分版本引擎在此目录存 DDC 增量片段 |
| Godot | `%APPDATA%/Godot/`（编辑器） | 官方 <https://docs.godotengine.org/en/stable/tutorials/editor/file_system.html>：Godot 编辑器全局数据根，含 `editor_settings.cfg`、`editor_cache/`、`export_cache/`、`app_userdata/<project_name>/` |
| Godot | `%LOCALAPPDATA%/Godot/` | 部分 Godot 版本将 `app_userdata` 放此处（每项目用户数据） |

## 2. 数据三级分级 + 默认行为

### L1 可重生缓存（点完即可由引擎自动重建）

| 子类 | 描述 | 默认勾选 | 保留期 | 备注 |
|-----|-----|---------|-------|-----|
| Unity 包下载缓存 | `%LOCALAPPDATA%/Unity/cache/`（UPM 包 tar.gz 下载缓存） | ✅ | 全量 | 下次装包重新下载 |
| Unity 编辑器日志 | `%LOCALAPPDATA%/Unity/Editor/{Editor.log,Editor-prev.log,upm.log}` | ✅ | 全量 | 引擎每次启动覆盖写入 |
| Unity Hub 日志 | `%APPDATA%/UnityHub/logs/`、`%APPDATA%/Unity Hub/logs/`、`%LOCALAPPDATA%/UnityHub/logs/` | ✅ | 全量 | Hub 每次启动覆盖写入 |
| UE 派生数据缓存 | `%LOCALAPPDATA%/UnrealEngine/Common/DerivedDataCache/**` | ✅ | 全量 | UE 最大可清缓存，数十 GB，重建无副作用 |
| UE DDC Partial | `%LOCALAPPDATA%/UnrealEngine/Common/DerivedDataCache_Partial/**` | ✅ | 全量 | 同上，部分版本引擎用此目录 |
| Godot 编辑器缓存 | `%APPDATA%/Godot/editor_cache/`、`~/.config/godot/Godot/editor_cache/` | ✅ | 全量 | 编辑器图标/缩略图缓存 |
| Godot 导出缓存 | `%APPDATA%/Godot/export_cache/` | ✅ | 全量 | 导出包临时产物 |

### L2 可选历史数据（用户原始内容）

**本 scaffold 不覆盖 L2**——引擎开发缓存全部是 L1。用户原始内容（项目 Assets/Source/Content）已在 L3 红线内。

### L3 红线（任何 scope glob 都不允许命中）

**通用红线**（继承 CLAUDE.md 铁律）：
- `*.db` / `*.db-wal` / `*.db-shm`
- `**/config/**`、`**/login/**`、`**/Accounts/**`
- `**/Favorite*/**`、`**/key/**`、`**/crypto/**`
- 浏览器登录数据（Cookies/Login Data/Bookmarks/logins.json 等）
- SSH / 云凭据、Windows 系统关键文件、回收站、虚拟磁盘镜像

**引擎专属红线**（本任务新增）：
- **Unity 项目目录整体**：`<project>/Library/**`（Unity 项目的资源导入数据库——新手最常误删的对象，删了强制全量重导且可能丢编辑器状态）、`<project>/Assets/**`（项目源码）、`<project>/ProjectSettings/**`（项目编辑器配置）、`<project>/Temp/**`（本任务拿不准 = 不动）、`<project>/.git/**`、`<project>/*.sln`
- **Unity 全局配置**：`%LOCALAPPDATA%/Unity/config/`、`%LOCALAPPDATA%/Unity/licenses/`、`%LOCALAPPDATA%/Unity/Unity.*.log`（顶层 log 文件——属授权/审计状态）
- **Unity Hub 用户状态**：`%APPDATA%/UnityHub/` 下的 Chromium-based 数据（`Cookies`、`Cookies-journal`、`Local Storage`、`Session Storage`、`Settings`、`hubConfig.json`、`cloudConfig.json`、`favoriteProjects.json`、`projectsArchitecture.json`、`blob_storage/`、`TransportSecurity`、`Cache/js`、`Cache/wasm`、`Code Cache/js`、`GPUCache`）、`%APPDATA%/UnityHubWebGLHost/`（WebGL 运行时本体）、`%APPDATA%/UnityMCP/`（MCP 授权状态）
- **Unreal 项目目录整体**：任何 `.uproject` 所在目录全部（`Content/**`、`Source/**`、`Config/**`、`Saved/**`、`Intermediate/**`、`.git/**`）
- **Unreal Engine 安装目录**：`C:/Program Files/Epic Games/UE_*/Engine/**`（本体）、`Feature Packs/**`
- **Unreal 引擎运行时配置**：`%LOCALAPPDATA%/UnrealEngine/<version>/Saved/Config/**`、`%LOCALAPPDATA%/UnrealEngine/Common/Intermediate/**`（本任务不动）、`%LOCALAPPDATA%/UnrealEngine/<version>/Saved/Logs/**`（本任务不动——日志属可再生但可能含用户路径敏感信息，本任务保守不清）
- **Godot 编辑器全局配置**：`%APPDATA%/Godot/editor_settings.cfg`、`editor_layout.cfg`、`editor_recent_fs`
- **Godot 每项目用户数据**：`%APPDATA%/Godot/app_userdata/<project_name>/**`、`%LOCALAPPDATA%/Godot/app_userdata/<project_name>/**`
- **Godot 项目目录**：`<project>/.godot/**`（含 `.godot/editor/script_cache.bin`、`.godot/editor/scene_groups_cache.cfg`、`.godot/imported/**`、`.godot/global_script_class_cache.cfg`、`.godot/uid_cache.bin`、`.godot/editor_state`）、`<project>/project.godot`、`<project>/res/**`、`<project>/.git/**`

## 3. 通用 prompt 形态

| Bucket 类型 | `prompt.kind` | default | UI label |
|-----|-----|------|------|
| 全部引擎缓存 | `none` | – | – |

**理由**：与 `browser-cache` / `steam-shadercache` / `obs-cache` 保持一致——这些可再生缓存**没有保留期概念**（不像聊天图片缓存需要留 30 天的历史），全量清空即可。用户如果确实想留最近 7 天的编辑器日志方便排查，可以自己从回收站还原（本 scaffold 走 `recycle`，可还原）。

## 4. 用户偏好（Phase 1-2 访谈结论）

- **默认 `mode`**：`recycle`（回收站，可还原）
- **整体 `risk`**：`low`（只清缓存，绝不删用户内容）
- **`recycle_granularity`**：`directory`（DDC 动辄几万个 shader 文件，per-file 会产生海量回收站条目）
- **变体识别**：用 `variant = "unity" / "unreal" / "godot"` 区分三家引擎，UI 侧可折叠展示

## 5. 给后端的 TOML 设计提示

见最终落地 [scaffolds/engine-caches.toml](../../scaffolds/engine-caches.toml)。核心 glob 骨架：

| scope id | variant | glob 骨架 | mode | prompt |
|----------|---------|-----------|------|--------|
| `unity-package-cache` | unity | `{%LOCALAPPDATA%,${HOME}/.cache/unity}/Unity/cache/**` | recycle | none |
| `unity-editor-logs` | unity | `{%LOCALAPPDATA%,${HOME}/.cache/unity}/Unity/Editor/**` | recycle | none |
| `unity-hub-logs` | unity | `{%LOCALAPPDATA%,%APPDATA%,${HOME}/.config/unityhub}/UnityHub/{logs,Logs}/**` | recycle | none |
| `unreal-derived-data-cache` | unreal | `{%LOCALAPPDATA%,${HOME}/.cache/ue}/UnrealEngine/Common/DerivedDataCache/**` | recycle | none |
| `unreal-dlc-partial` | unreal | `{%LOCALAPPDATA%,${HOME}/.cache/ue}/UnrealEngine/Common/DerivedDataCache_Partial/**` | recycle | none |
| `godot-editor-cache` | godot | `{%APPDATA%,${HOME}/.config/godot}/Godot/editor_cache/**` | recycle | none |
| `godot-export-cache` | godot | `{%APPDATA%,${HOME}/.config/godot}/Godot/export_cache/**` | recycle | none |

## 6. Disclaimer 文案要点

- ✅ **绝不删**：Unity 项目 Library/、Assets/、ProjectSettings/、.git、.sln、Temp/；任何 .uproject 项目目录整体；Unreal Engine 安装目录（C:/Program Files/...）；Unreal Saved/Config/、Common/Intermediate/；Godot editor_settings.cfg、editor_layout.cfg、app_userdata/；Godot 项目 .godot/editor 关键文件
- ✅ **走回收站可还原**（`mode = "recycle"` + `recycle_granularity = "directory"`）
- ✅ **不使用"安全"二字**（遵循 CLAUDE.md 文案规范）
- ✅ **明确可再生**：缓存删除后引擎下次启动会自动重建

## 7. 保守决策与理由（可追溯）

| 决策 | 内容 | 理由 |
|---|---|---|
| D1 | 不清 Unity 项目 `Library/` | **本任务最重要红线**。Library/ 是 Unity 项目的资源导入数据库（ArtifactDB、ShaderCache、ScriptAssemblies 等），删除后 Unity 强制全量重导所有资源（可能数小时），且可能丢失编辑器状态（未保存的脚本状态、导入设置）。这是新手最常误删的对象。 |
| D2 | 不清 Unity 项目 `Assets/` | 项目源码本体 |
| D3 | 不清 Unity 项目 `ProjectSettings/` | 项目编辑器配置（红线 `config/`） |
| D4 | 不清 Unity 项目 `Temp/` | 拿不准 = 不动。Temp/ 里是 Unity 运行时临时产物（UnityTempFile-*.dat、obj/Debug/*.cs 中间产物），删了下次编译会自动重建——但**边界不清晰**（例如某些编辑器插件把用户自定义数据放这里），本任务保守不清 |
| D5 | 不清 Unity 项目 `.git/`、`*.sln` | 项目源码与解决方案文件 |
| D6 | 不清 Unity 全局 `config/`、`licenses/` | 红线 `config/` 与授权文件 |
| D7 | 不清 Unity 顶层 `Unity.Licensing.Client.log`、`Unity.Entitlements.Audit.log` | 顶层 log 属授权/审计状态（不是 Editor/ 子目录下的常规编辑器日志）。本任务只清 `%LOCALAPPDATA%/Unity/Editor/` 子目录内的日志 |
| D8 | 不清 `%LOCALAPPDATA%/Unity/Caches/bee/` | Bee 是 Unity C# 编译后端缓存（可再生），但**位置与 cache/ 平级而非其下**，本任务只锚定 `cache/` 与 `Editor/` 两个明确路径。要清 Bee 需另开 scope，本任务保守不收 |
| D9 | Unity Hub 只清 `logs/` 子目录，不清其它 | Unity Hub 是 Electron 应用，`%APPDATA%/UnityHub/` 下绝大部分是 Chromium 数据（Cookies/Local Storage/Settings/blob_storage 等）——删了等于登出 Hub 并丢失用户偏好。只清 `logs/` 是明确可再生的日志 |
| D10 | 不清 `%APPDATA%/UnityHubWebGLHost/` | WebGL 运行时安装本体，不是缓存 |
| D11 | 不清 `%APPDATA%/UnityMCP/` | MCP 授权状态（`customer_uuid.txt` 含用户标识） |
| D12 | UE 只清 `Common/DerivedDataCache/` 与 `Common/DerivedDataCache_Partial/` | 这是官方明确的引擎级派生数据缓存，是本任务要清的最大目标 |
| D13 | 不清 UE 任何项目目录 | `.uproject` 所在目录整体红线（Content/Source/Config/Saved/Intermediate 全部） |
| D14 | 不清 UE `Engine/` 安装目录（`C:/Program Files/Epic Games/...`） | 引擎安装本体。glob 严格锚定 `%LOCALAPPDATA%/UnrealEngine/Common/...`，`C:/Program Files/...` 天然不命中 |
| D15 | 不清 UE `%LOCALAPPDATA%/UnrealEngine/<version>/Saved/Config/` | 引擎运行时的用户配置（EditorPerProjectUserSettings.ini 等） |
| D16 | 不清 UE `Common/Intermediate/` | 拿不准 = 不动。Intermediate 是引擎/项目的中间编译产物（.obj/.exp 等），删了会强制重编，但**位置与 DDC 平级**，且部分版本引擎把它当"配置"用。本任务保守只清 DDC 两个目录 |
| D17 | 不清 UE `<version>/Saved/Logs/` | 拿不准 = 不动。日志属可再生，但可能含用户项目路径等敏感信息 |
| D18 | Godot 只清 `editor_cache/` 与 `export_cache/` | 明确的引擎缓存目录 |
| D19 | 不清 Godot `editor_settings.cfg`、`editor_layout.cfg` | 编辑器全局配置（红线） |
| D20 | 不清 Godot `app_userdata/` | 每项目用户数据（红线——用户存档、截图、自定义数据都在这里） |
| D21 | 不清 Godot 项目 `.godot/` | 项目级缓存/元数据（`.godot/imported/` 的 .ctex 资源、`.godot/editor/script_cache.bin`、`.godot/uid_cache.bin` 等）。**即使 `.godot/editor/` 子目录理论上可清**，但边界不清晰（比如 `.godot/global_script_class_cache.cfg` 在顶层而不在 `editor/` 子目录），本任务保守不清 |
| D22 | 全部走 `recycle_granularity = "directory"` | DDC 是几万个小文件，per-file recycle 会产生海量回收站条目 |
| D23 | `prompt.kind = "none"` | 引擎缓存没有保留期概念（不像聊天图片需要留 30 天历史）；用户如果想留最近几天的日志可以从回收站还原 |
| D24 | 用 `variant` 字段区分 unity / unreal / godot | UI 侧可以按引擎折叠展示；三个引擎用户不同（Unity 用户不会看 UE scope） |
| D25 | 每个引擎至少 1 个 scope | 满足任务要求；实际 Unity 3 个、UE 2 个、Godot 2 个 |
| D26 | glob 用 brace 展开 `{A,B,C}/path/**` 合并 Windows/Unix 路径 | 沿用 `obs-cache.toml` / `browser-cache.toml` 既有模式 |
| D27 | glob 值**必须用引号**包裹 | TOML 1.0 中裸 `{...}` 是 inline table 语法，会被误解析；`browser-cache.toml` 已用引号（例如 line 34），本 scaffold 遵循同一惯例 |

## 8. Safety test 覆盖清单

见 [crates/scaffold/tests/engine_caches_safety.rs](../../crates/scaffold/tests/engine_caches_safety.rs)。红线断言覆盖：

**Unity 项目红线**（13 条）：`Library/{ArtifactDB,ArtifactURLToHashCache,BurstCache,ShaderCache,ScriptAssemblies,EditorOnlyVirtualTextAssets}`、`Assets/{Scenes,Scripts,Plugins}`、`ProjectSettings/{ProjectSettings,QualitySettings}`、`.git/HEAD`、`*.sln`、`Temp/{UnityTempFile,obj}`

**Unity 全局红线**（7 条）：`config/production.json`、`licenses/packages/lic.lic`、`Unity.Licensing.Client.log`、`Unity.Entitlements.Audit.log`

**Unity Hub 红线**（17 条）：`cloudConfig.json`、`Settings`、`favoriteProjects.json`、`projectsArchitecture.json`、`hubConfig.json`、`Cookies`、`Cookies-journal`、`Network Persistent State`、`Local Storage/leveldb/`、`Session Storage/`、`blob_storage/`、`Cache/js/`、`Code Cache/js/`、`GPUCache/`、`TransportSecurity`、顶层 `000005.log`、`CURRENT`、`MANIFEST-*`

**Unreal 项目红线**（10 条）：`*.uproject`、`Content/Levels/`、`Content/Blueprints/`、`Source/`、`Config/DefaultGame.ini`、`Saved/Config/WindowsEditor/Game.ini`、`Saved/Autosaves/`、`Intermediate/Build/`、`.git/HEAD`

**Unreal Engine 安装目录红线**（4 条）：`C:/Program Files/Epic Games/UE_5.3/Engine/{Binaries,Source,Content}`、`Feature Packs/`

**Unreal 引擎配置红线**（6 条）：`%LOCALAPPDATA%/UnrealEngine/5.3/Saved/Config/WindowsEditor/{EditorPerProjectUserSettings,GameUserSettings}.ini`、`%LOCALAPPDATA%/UnrealEngine/4.27/Saved/Config/WindowsEditor/Game.ini`、`%LOCALAPPDATA%/UnrealEngine/Common/Intermediate/`、`%LOCALAPPDATA%/UnrealEngine/5.6/Saved/Logs/UE5.log`

**Godot 红线**（13 条）：`editor_settings.cfg`、`editor_layout.cfg`、`editor_recent_fs`、`app_userdata/<project>/{save.slots,settings.cfg,screenshot.png}`、`%LOCALAPPDATA%/Godot/app_userdata/<project>/save.slots`、`project.godot`、`.godot/editor/{script_cache.bin,scene_groups_cache.cfg}`、`.godot/imported/texture.png-*.ctex`、`.godot/global_script_class_cache.cfg`、`.godot/uid_cache.bin`、`.godot/editor_state`、`res/{scenes/main.tscn,scripts/player.gd}`、`.git/HEAD`

**假阳性防线**（4 条）：`Documents/Unity/cache/`、`Documents/UnrealEngine/Common/DerivedDataCache/`、`Downloads/Godot/editor_cache/`、`Desktop/UnityHub/logs/`

**通用红线样本**（29 条）：完整继承 `diskpilot_scaffold::red_line_samples()` 全清单

## 9. 验证命令

```bash
cd J:/xiangm_transfer/xiangm/tools/diskpilot_src

# 1) scaffold 红线 lint（必须 ok + no red-line hits）
cargo run -p diskpilot-scaffold-lint -- scaffolds/engine-caches.toml
# 输出：ok: scaffolds/engine-caches.toml (engine-caches, 7 scopes, no red-line hits)

# 2) safety test（必须 1 passed）
cargo test -p diskpilot-scaffold --test engine_caches_safety
# 输出：test result: ok. 1 passed; 0 failed
```

## 10. 每个引擎清了什么 / 没清什么

| 引擎 | 清了 | 没清（红线） |
|---|---|---|
| **Unity** | ① 包下载缓存 `%LOCALAPPDATA%/Unity/cache/**`（`npm/`、`packages/` 两个 UPM 包下载桶）<br>② 编辑器日志 `%LOCALAPPDATA%/Unity/Editor/**`（Editor.log、Editor-prev.log、upm.log 三件套）<br>③ Hub 日志 `%APPDATA%/UnityHub/logs/**` 与 `%APPDATA%/Unity Hub/logs/**`（旧版） | ① 任何项目 `Library/`、`Assets/`、`ProjectSettings/`、`Temp/`、`.git/`、`*.sln`<br>② 全局 `config/`、`licenses/`、顶层 Licensing/Entitlements log<br>③ Hub 的 Chromium 数据（Cookies/Local Storage/Settings/hubConfig/cloudConfig/favoriteProjects/…）<br>④ Hub 安装本体、WebGLHost 运行时、UnityMCP 授权状态<br>⑤ `Caches/bee/`（Bee 编译缓存——拿不准，本任务不动） |
| **Unreal Engine** | ① 派生数据缓存 `%LOCALAPPDATA%/UnrealEngine/Common/DerivedDataCache/**`（DDC，UE 最大可清缓存，数十 GB）<br>② DDC Partial `%LOCALAPPDATA%/UnrealEngine/Common/DerivedDataCache_Partial/**`（部分版本引擎的增量片段） | ① 任何项目目录整体（`.uproject` 所在 + `Content/`、`Source/`、`Config/`、`Saved/`、`Intermediate/`、`.git/`）<br>② 引擎安装目录 `C:/Program Files/Epic Games/UE_*/Engine/`<br>③ 引擎运行时配置 `%LOCALAPPDATA%/UnrealEngine/<version>/Saved/Config/`<br>④ `Common/Intermediate/`（拿不准 = 不动）<br>⑤ `<version>/Saved/Logs/`（拿不准 = 不动） |
| **Godot** | ① 编辑器缓存 `%APPDATA%/Godot/editor_cache/**`<br>② 导出缓存 `%APPDATA%/Godot/export_cache/**` | ① 编辑器全局配置 `editor_settings.cfg`、`editor_layout.cfg`<br>② 每项目用户数据 `%APPDATA%/Godot/app_userdata/<project>/**`<br>③ 项目 `.godot/` 整体（`.godot/editor/*`、`.godot/imported/*`、`.godot/uid_cache.bin`、`.godot/global_script_class_cache.cfg` 等——即使部分子目录理论可清，边界不清晰本任务保守不动）<br>④ 项目源码 `project.godot`、`res/**`、`.git/` |
