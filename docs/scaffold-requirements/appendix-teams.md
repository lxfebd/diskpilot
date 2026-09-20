# 实测附录：Microsoft Teams（桌面版，1.x 经典版 + 2.x 新版）

> 本文件是 [scaffolds/teams.toml](../../scaffolds/teams.toml) 的需求附录，对应主清单 [messaging.md](messaging.md) 的「§7 实测路径映射」格式（同 Discord §7.8）。
> 跑：`cargo test -p diskpilot-scaffold --test teams_safety`

## 1. 勘测发现（2026-09-08，Windows 开发机）

勘测方式：`ls` 只列目录名，**未 Read 任何文件内容**（配置 / 缓存 / 账号数据均未读取，遵守隐私铁律）。

| 探测路径 | 结果 |
|---|---|
| `%APPDATA%/Microsoft/` | 存在，含 `Office/`、`Windows/`、`AddIns/`、`Vault/`、`Crypto/` 等 27 个子目录；**无 `Teams/`** |
| `%APPDATA%/Microsoft/Teams` | **不存在**（本机未安装 1.x 经典版） |
| `%APPDATA%/Microsoft/Teams/desktop-config` | 不存在 |
| `%APPDATA%/Microsoft/Teams Meeting Add-in` | 不存在 |
| `%APPDATA%/Microsoft/TeamsClassic` | 不存在 |
| `%LOCALAPPDATA%/Microsoft/Teams` / `TeamsClassic` / `MSTeams` | 全部不存在 |
| `%LOCALAPPDATA%/Packages/Microsoft.Teams_8wekyb3d8bbwe` | 不存在 |
| `%LOCALAPPDATA%/Packages/MSTeams_8wekyb3d8bbwe` | **存在**（2.x 新版已安装） |
| `%USERPROFILE%/AppData/Local/Packages/MSTeams_8wekyb3d8bbwe/` | 顶层子目录：`AC`、`AppData`、`LocalCache`、`LocalState`、`RoamingState`、`Settings`、`SystemAppData`、`TempState` |
| 上述 `AppData/`、`LocalState/`、`SystemAppData/` | 均为**空目录**（应用未运行 / 已卸载残留壳） |
| `%USERPROFILE%/.config/microsoft-telemetry` | 不存在 |
| `%USERPROFILE%/../ProgramData/Microsoft/Microsoft Teams/` | 不存在 |

**结论：本机未安装 1.x 经典版，2.x 新版只有空的包壳。**
因此 1.x 的数据布局**不是本机实测**，依据是：
1. Electron 应用通用结构（Teams 1.x 是纯 Electron，与 Discord 同构——已在 §7.8 实测确认同一套 Chromium 缓存目录名）；
2. 多个开源清理项目（TeamsCleaner 系列）公开列出的 1.x 目录名；
3. 任务书给定的已知事实（`media-stack/`、`desktop-config/`、`app_settings.json`）。

2.x 新版的顶层子目录名（`AC` / `LocalCache` / `TempState` / `LocalState` / `RoamingState` / `Settings` / `SystemAppData` / `AppData`）是本机实测到的；其中 `AppData/` 下再套 `Local/`、`Roaming/`（MSIX 约定）这一层**未在实机验证**（本机该目录为空），按 MSIX 打包通用约定处理，并在 §4 标注为待验证假设。

## 2. 与初始假设不同的点

| 初始假设 | 勘测结果 | 处置 |
|---|---|---|
| 「`%LOCALAPPDATA%/Microsoft/Teams/...` 下有日志」 | 本机不存在该目录；无证据表明新版日志走此路径 | 日志 scope 保留，但用 brace 同时覆盖 Roaming/Local 与 `log`/`logs` 两种拼写，**未清空的假路径不影响安全**（glob 不匹配则无操作） |
| 「新版 Teams 路径结构可能不同——保守处理」 | 确认不同，且差异比预想大：新版是 **MSIX 包**，数据根在 `%LOCALAPPDATA%/Packages/<PackageFamilyName>/` 下，**不在** `%APPDATA%/Microsoft/Teams/` | 新增 4 个新版 scope（`new-app-cache` / `new-local-cache` / `new-temp-state` / `new-chromium-cache`），detect 加 `%LOCALAPPDATA%/Packages/MSTeams_8wekyb3d8bbwe` 与 `**/MSTeams_8wekyb3d8bbwe` |
| 「`%APPDATA%/Microsoft/Teams/media-stack/**` 是会议音视频临时，可清」 | 本机未验证（经典版未装） | **保留为 scope**，依据为公开清理项目一致说法；`category = "media"` 使其在 UI 归到媒体桶，用户可单独跳过 |
| 「新版路径结构可能不同」 | 本机实际检测到的是 `MSTeams_8wekyb3d8bbwe`（不是常被引用的 `Microsoft.Teams_8wekyb3d8bbwe`） | detect 用实际观测到的包族名 `MSTeams_8wekyb3d8bbwe`，不写猜测变体 |
| 「`meeting-addin/**` 谨慎，是外接程序安装」 | `%APPDATA%/Microsoft/Teams Meeting Add-in` 本机不存在 | 不进任何 scope；写进 disclaimer 的「绝不触碰」清单 + safety 测试红线断言 |
| — | 新版包的 `LocalState/`、`RoamingState/`、`Settings/`、`SystemAppData/` 是账号与会话状态 | 全部列为红线（见 §3.3），不进 scope |

## 3. 数据三级分级映射表

### 3.1 L1 可重生缓存（默认勾选，全量清，`prompt.kind = "none"`）

| scope id | label | glob | mode | 依据 |
|---|---|---|---|---|
| `cache` | Chromium 缓存 | `%APPDATA%/Microsoft/Teams/Cache/**` | recycle | Electron/Chromium 标准缓存区，与 Discord §7.8 同构 |
| `code-cache` | 代码缓存 | `%APPDATA%/Microsoft/Teams/Code Cache/**` | recycle | JS 编译产物，重启重建 |
| `gpu-cache` | GPU / WebGPU 缓存 | `%APPDATA%/Microsoft/Teams/{GPUCache,DawnGraphiteCache,DawnWebGPUCache}/**` | recycle | Chromium GPU 管线缓存，重启重建 |
| `blob-storage` | Blob 临时存储 | `%APPDATA%/Microsoft/Teams/blob_storage/**` | recycle | Blob URL 暂存，重启重建 |
| `crashpad` | 崩溃报告 | `%APPDATA%/Microsoft/Teams/Crashpad/**` | recycle | Breakpad 崩溃 dump，与 Discord 同构 |
| `media-stack` | 会议音视频临时 | `%APPDATA%/Microsoft/Teams/media-stack/**` | recycle | 媒体管线中间产物，非用户原始内容 |
| `app-logs` | 运行日志 | `%USERPROFILE%/AppData/{Roaming,Local}/Microsoft/Teams/{log,logs}/**` | recycle | 运行日志；brace 覆盖两处 × 两种拼写 |
| `new-app-cache` | 新版 Application Cache | `%LOCALAPPDATA%/Packages/MSTeams_8wekyb3d8bbwe/AC/**` | recycle | 本机实测存在的顶层缓存目录 |
| `new-local-cache` | 新版 LocalCache | `%LOCALAPPDATA%/Packages/MSTeams_8wekyb3d8bbwe/LocalCache/**` | recycle | 本机实测存在的顶层缓存目录 |
| `new-temp-state` | 新版临时区 | `%LOCALAPPDATA%/Packages/MSTeams_8wekyb3d8bbwe/TempState/**` | recycle | 本机实测存在的顶层临时目录 |
| `new-chromium-cache` | 新版 Chromium 缓存 | `%LOCALAPPDATA%/Packages/MSTeams_8wekyb3d8bbwe/AppData/{Local,Roaming}/{Cache,Code Cache,GPUCache,ShaderCache,CodeCache,blob_storage,DawnGraphiteCache,DawnWebGPUCache,Crashpad}/**` | recycle | MSIX 约定，包内再套 AppData/{Local,Roaming} |

**prompt 形态**：全部 `none`。理由——Teams 缓存都是无保留期语义的可重生中间产物（同 Discord），没有「删 30 天以前的」这种用户可理解的时间维度；给 `days` 会让用户误以为删掉的是旧聊天。

### 3.2 L2 可选历史数据

**未提供任何 L2 scope。** 理由（可追溯）：

- Teams 的用户原始内容（会议录制、聊天中收到的文件）不在缓存目录下——录制落在用户自己选的目录（默认 `%USERPROFILE%/Documents/Recordings` 或 OneDrive），聊天文件落在 OneDrive 或用户下载目录。**缓存目录里没有可清的 L2 内容**，强行开 scope 只会删错东西。
- `media-stack/` 看似像「会议内容」，实际是播放/推流的**临时中间产物**（无用户可识别文件名、无持久引用），归 L1 而非 L2。
- 这与 Discord 的结论一致（§7.8 保守决策 2）：Electron 类聊天应用的缓存区不含用户原始内容，不做「接收文件」scope。

### 3.3 L3 红线（任何 scope glob 都不允许命中）

继承 [messaging.md](messaging.md) §2 L3 通用红线（`*.db` / `*.db-wal` / `*.db-shm` / `**/Accounts/**` / `**/login/**` / `**/config/**` / `**/Favorite*/**` / `**/Fav/**` / `**/key/**` / `**/crypto/**`），追加 Teams 专属：

| 红线路径 | 原因 |
|---|---|
| `%APPDATA%/Microsoft/Teams/IndexedDB/**` | 聊天消息 / 会话存储（Electron 主数据库），删了 = 丢聊天历史 |
| `%APPDATA%/Microsoft/Teams/Local Storage/**` | 登录态 + 设置持久化 |
| `%APPDATA%/Microsoft/Teams/Session Storage/**` | 当前会话状态 |
| `%APPDATA%/Microsoft/Teams/Network/**` | Cookie / HTTP 缓存 / Trust Tokens（登录凭据） |
| `%APPDATA%/Microsoft/Teams/{Preferences,Cookies,Login Data,Local State,First Run,Widevine CDM/**}` | Chromium 顶层凭据文件 |
| `%APPDATA%/Microsoft/Teams/desktop-config/**` | 账号配置（任务书明确要求列入红线） |
| `%APPDATA%/Microsoft/Teams/app_settings.json` | 用户设置（任务书明确要求列入红线） |
| `%APPDATA%/Microsoft/Teams Meeting Add-in/**` | Outlook 外接程序安装目录，删了 = 会议室入口失效 |
| `%LOCALAPPDATA%/Packages/MSTeams_8wekyb3d8bbwe/{LocalState,RoamingState,Settings,SystemAppData}/**` | 新版账号与会话状态（本机实测存在的目录） |
| `%LOCALAPPDATA%/Packages/MSTeams_8wekyb3d8bbwe/AppData/{Local,Roaming}/{Local Storage,Session Storage,IndexedDB,Network,Preferences,Local State,Cookies,Login Data}/**` | 新版包内的 Chromium 登录/会话区——与缓存同级，最容易因 glob 写宽而误删 |

## 4. 保守决策与理由

1. **新版只 clear 三个纯缓存顶层目录 + 包内 Chromium 缓存子目录，绝不用 `AppData/**` 或 `AppData/*/**`。**
   MSIX 包内 `AppData/` 树里 `Local Storage/`、`IndexedDB/`、`Network/` 与 `Cache/` 是**同级并列**的。用一层通配（`AppData/**`）就能命中全部——这正是「glob 写宽了」的典型陷阱。最终用 `AppData/{Local,Roaming}/{Cache,Code Cache,GPUCache,ShaderCache,CodeCache,blob_storage,DawnGraphiteCache,DawnWebGPUCache,Crashpad}/**` 两层显式白名单，safety 测试里对同树下每个红线目录逐条断言 zero match。

2. **新版 `AC` / `LocalCache` / `TempState` 整目录 clear。**
   这三个是包顶层、专用于缓存/临时的容器（本机实测目录名），下面没有会话状态混存。不像 `AppData/` 那样是「缓存 + 登录态」的混合树，所以整目录 glob 可接受。

3. **`media-stack/` 保留但归 `media` 桶。**
   本机无法验证，但依据公开清理项目一致说法是媒体管线临时区。归到 `media` category 后，UI 上它与缓存桶分开，用户如果只想清 Chromium 缓存可以单独跳过它——把不确定性的处理权交给用户，而不是靠 glob 精确到子路径去赌目录结构。

4. **不做账号通配段。**
   Teams 单账号实例（多组织切换不产生多份数据目录，`desktop-config/<guid>/` 是账号配置不缓存），`Cache/` 等缓存区不区分账号。无需 `<account>/` 通配段，简化 glob 即降低误删面。

5. **不用 `name_contains`。**
   裸 `"Teams"` 在 Windows 上命中面太宽（`%APPDATA%/Microsoft/` 下有很多 Teams 无关的 Microsoft 子目录，用户自己命名的 `Teams 截图/` 之类也会中）。只用具体的 `Microsoft/Teams` 与 `Packages/MSTeams_8wekyb3d8bbwe` 路径 + `**/...` 兜底通配。safety 测试里加了 `SomeApp/Microsoft/Teams/Cache/`（前缀污染）与 `Microsoft/TeamsCache/`（前缀撞名）两条负向断言钉死这一点。

6. **日志 scope 覆盖 `log`/`logs` 两种拼写 + Roaming/Local 两处，即使假路径也无害。**
   本机没有经典版，无法确认日志目录确切拼写。brace 全覆盖的代价只是 glob 表达式长一点，收益是不依赖未验证假设；不存在的分支 glob 不匹配任何文件，零风险。

7. **版本兼容说明（写进 disclaimer）**。
   scaffold 同时服务 1.x 经典版与 2.x 新版；两代可以同时存在（经典版未卸载时）。新版包族名以本机实测的 `MSTeams_8wekyb3d8bbwe` 为准——社区文档常引用的 `Microsoft.Teams_8wekyb3d8bbwe` 本机不存在，未加入 detect。若未来新版迁移包族名，需要更新 `detect` 与 4 个新版 scope 的 glob（单一改动点）。

## 5. safety test 覆盖

`crates/scaffold/tests/teams_safety.rs`：

- **正向断言 22 条**：11 个 scope id 每个至少 1 条（`cache` / `code-cache` / `gpu-cache`×3 / `blob-storage` / `crashpad` / `media-stack` / `app-logs`×4 / `new-app-cache` / `new-local-cache` / `new-temp-state` / `new-chromium-cache`×7）。
- **红线断言 38 条**，覆盖：
  - 1.x 登录态 / 数据：`Local Storage`、`Session Storage`、`IndexedDB`（含 `LOCK`、`.db` 后缀变体）、`Network/Cookies`、`Network/Trust Tokens`
  - 1.x 顶层凭据：`Preferences`、`First Run`、`Local State`、`Cookies`、`Login Data`、`Widevine CDM/decryption_keys.json`
  - 账号配置 / 设置：`desktop-config/<guid>/config.dat`、`app_settings.json`
  - 外接程序：`Teams Meeting Add-in/install/.../TeamsMeetingAddin.dll`
  - 通用红线族：`key/`、`config/`、`Accounts/`、`Favorite/`、`Fav/`（位于 Teams 数据根下）
  - 2.x 状态区：`LocalState/`、`RoamingState/`、`Settings/current/`、`SystemAppData/`
  - 2.x 包内 Chromium 登录/会话区：`AppData/Local/{Local Storage,IndexedDB,Network,Session Storage,Preferences,Local State,Login Data}` 与 `AppData/Roaming/{Local Storage,Network}`
  - 误伤防御：`SomeApp/Microsoft/Teams/Cache/`（前缀污染）、`Microsoft/TeamsCache/`（前缀撞名）
- **结果**：`1 passed; 0 failed`。
- **lint 结果**：`ok: scaffolds/teams.toml (teams, 11 scopes, no red-line hits)`。
