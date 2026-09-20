# Zoom 清理需求附录

> 交付物：[scaffolds/zoom.toml](../../scaffolds/zoom.toml) + [crates/scaffold/tests/zoom_safety.rs](../../crates/scaffold/tests/zoom_safety.rs)
>
> 交付日期：2026-09-08。scaffold id = `zoom`，6 个 scope，`risk = "low"`。
>
> 参照样板：[scaffolds/discord.toml](../../scaffolds/discord.toml)（Electron 缓存 glob 形态 / disclaimer 结构）、
> [scaffolds/qq-pc.toml](../../scaffolds/qq-pc.toml)（后缀白名单写法）、
> [scaffolds/obs-cache.toml](../../scaffolds/obs-cache.toml)（`recycle_granularity = "directory"`）、
> `crates/scaffold/src/lib.rs` 的 `red_line_samples()`（第 241 行起）。

## 1. 勘测发现

### 1.1 本机状态

**本机未安装 Zoom，无任何数据可探测。** 以下路径全部不存在：

```text
%APPDATA%/zoom.us          不存在
%APPDATA%/Zoom             不存在
%LOCALAPPDATA%/zoom.us     不存在
%LOCALAPPDATA%/Zoom        不存在
%USERPROFILE%/Documents/Zoom         不存在
%USERPROFILE%/Documents/ZoomRecordings 不存在
```

补充确认：`find %APPDATA% %LOCALAPPDATA% -maxdepth 2 -iname "*zoom*"` 零结果，
`find %USERPROFILE%/Documents -maxdepth 2 -iname "*zoom*"` 零结果。
即本机既没有 Zoom 数据根，也没有任何「同名字符串」的近似目录（无 ZoomCar / ZoomInfo 之类干扰项）。

### 1.2 依据

因此本 scaffold 的 glob 布局基于**公开信息**推导，而非本机实测：

- Zoom 是 **Electron + 原生混合**客户端。旧客户端（`ZoomUs.exe`，5.x / 6.x 世代）写
  `%APPDATA%/zoom.us`；新客户端（`Zoom.exe`，7.x+）写 `%APPDATA%/Zoom`。
  两者共用 `data/` 结构（`data/WebRTC`、`data/Telemetry`、`data/im`），
  同一台机器上两个目录可能同时残留。
- `%LOCALAPPDATA%/Zoom` 是 Electron 的 `app.getPath('userData')` 根，
  持有 Chromium 的 `Cache` / `Code Cache` / `GPUCache` 等，以及子进程目录 `app/`。
- `%USERPROFILE%/Documents/Zoom` 与 `ZoomRecordings/` 是**会议录制根**，用户内容。
- 各桶名与「可清 / 不可清」的划分来自社区清理脚本（Zoom-Cleanup 一类脚本）的公开描述。

**保守后果**：无法核实的桶（`downloads/`）只给窄 glob，且路径不存在时天然匹配为空。
本机实测推翻的部分见 §5「实测附录」。

## 2. 数据根与目录树（公开结构推导）

```text
%APPDATA%/zoom.us/              （旧客户端）        %APPDATA%/Zoom/           （新客户端，同构）
├── data/                       L3 边界：混着账号配置与 DB，绝不做 `data/**`
│   ├── WebRTC/                 L1 音视频临时文件（已知最大头）
│   ├── media-cache/            L1 媒体缓存
│   ├── Telemetry/              L1 遥测（保留 30 天）
│   ├── im/                     L3 聊天 / 会议消息 DB（*.db / *.db-wal / *.db-shm）
│   ├── zoom.us.conf            L3 账号配置
│   ├── config/                 L3 配置目录
│   ├── login/                  L3 登录态
│   └── Accounts/               L3 账号状态
├── logs/                       L1 客户端日志（保留 30 天）
└── downloads/                  L1 安装包 / 补丁归档（保留 30 天）

%LOCALAPPDATA%/Zoom/            （Electron UserData 根）
├── Cache/  Code Cache/         L1
├── GPUCache/ GrShaderCache/    L1
│   DawnGraphiteCache/ DawnWebGPUCache/  ShaderCache/
├── app/
│   ├── logs/  Crashpad/        L1
│   └── Local Storage/ Session Storage/ IndexedDB/ Network/
│       Login Data/ Cookies/ Local State/ Preferences/ First Run/
│       key/ Crypto/            L3 Chromium 登录态 / 加密 key 材料
├── Login Data/ Cookies/ Local State/ Preferences/ First Run/   L3
└── Session Storage/ Local Storage/ IndexedDB/ Network/         L3

%USERPROFILE%/Documents/Zoom/         L3 用户录制（整树）
%USERPROFILE%/Documents/ZoomRecordings/  L3 用户录制（整树）
```

## 3. L1 / L2 / L3 分级映射表

### L1 可重生缓存（默认勾选）

| scope id | label | glob 骨架 | mode | prompt | 分级理由 |
|---|---|---|---|---|---|
| `data-cache` | 音视频缓存（WebRTC / 媒体缓存） | `%APPDATA%/zoom.us/data/{WebRTC,media-cache}/**` | recycle | none | 抓帧 / 缓冲临时数据，重启即重建；已知磁盘最大头 |
| `telemetry` | 应用遥测数据 | `%APPDATA%/zoom.us/data/Telemetry/**` | recycle | days=30 | 只服务上报，删了不影响本地功能；留 30 天便于回溯卡顿归因 |
| `logs` | 客户端日志 | `%APPDATA%/zoom.us/logs/**` | recycle | days=30 | 日志，滚动生成 |
| `installer-downloads` | 安装包与补丁归档 | `%APPDATA%/zoom.us/downloads/**` | recycle | days=30 | 安装包缓存，清掉不影响已装版本 |
| `electron-cache` | 浏览器缓存（Chromium / GPU） | `%LOCALAPPDATA%/Zoom/{Cache,Code Cache,GPUCache,GrShaderCache,ShaderCache,DawnGraphiteCache,DawnWebGPUCache}` | recycle | none | Electron 网页缓存，删后自动重建 |
| `electron-logs` | 应用日志与崩溃报告 | `%LOCALAPPDATA%/Zoom/app/{logs,Crashpad}/**` | recycle | days=30 | 日志 / 崩溃转储 |

### L2 可选历史数据

**Zoom 无 L2 scope。** 原因：Zoom 的「历史数据」全部落在这两处，都是用户原始内容，
按本项目规则直接归 L3 不碰，而不是归 L2 给保留期：

- 会议录制（`%USERPROFILE%/Documents/Zoom`、`ZoomRecordings`）——用户内容，云端同步状态不明，删了未必找得回。
- 聊天 / 会议消息（`%APPDATA%/zoom*/data/im/*.db`）——本地 DB，删了历史不可恢复。

这与 [qq-pc.toml](../../scaffolds/qq-pc.toml) 的处理相反：QQ 的接收文件可以「重新下载」，
所以放 L2 给保留期；Zoom 没有对应机制，所以不列。

### L3 红线（任何 scope glob 都不允许命中）

- **通用红线**（继承 `CLAUDE.md` 铁律 #1 / `red_line_samples()`）：
  `*.db` / `*.db-wal` / `*.db-shm`、`**/Accounts/**`、`**/config/**`、`**/login/**`、
  `**/key/**`、`**/crypto/**`、`**/Favorite*/**`、`**/Fav/**`、
  浏览器登录数据（Cookies / Login Data / Local State / Preferences / First Run /
  Local Storage / IndexedDB / Network / Session Storage）、`.ssh` / `.aws` 等凭据。
- **Zoom 专属红线**（写入 disclaimer + safety 测试）：
  - `%USERPROFILE%/Documents/Zoom/**` 与 `%USERPROFILE%/Documents/ZoomRecordings/**` —— **第一红线**，用户录制
  - `%APPDATA%/zoom*/data/zoom.us.conf` 及 `data/config/**` —— 账号配置
  - `%APPDATA%/zoom*/data/im/**` —— 聊天 / 会议消息 DB
  - `%APPDATA%/zoom*/data/{login,Accounts}/**` —— 登录态 / 账号状态
  - `%LOCALAPPDATA%/Zoom/**` 与 `%LOCALAPPDATA%/Zoom/app/**` 的 Chromium 登录态与 `key` / `crypto`

## 4. 保守决策与理由（可追溯）

| 决策 | 内容 | 理由 |
|---|---|---|
| D1 | 不用 `data/**` 遍历 | `data/` 下同时混着 `WebRTC`（可清）与 `im` DB / `config`（红线）。宽 glob 必然触发硬规则 #1。改为**显式桶名白名单** `{WebRTC,media-cache}`，把可清范围锁死在纯缓存桶 |
| D2 | 两个 Roaming 根只写 `zoom.us` | `zoom.us` 与 `Zoom` 同构，但写 `%APPDATA%/Zoom/...` 会让「用户自建一个 `Documents/Zoom` 缓存」之类场景更容易被误认；`**/zoom.us` 与 `**/Zoom` 都进了 detect，卡片仍可见。取窄，不取宽 |
| D3 | `electron-logs` 只取 `app/{logs,Crashpad}` | `app/` 目录与 Electron UserData 根同级共存，里面还有 `Local Storage` / `IndexedDB` / `Network` / `key` / `Crypto`。写 `app/**` 会误删登录态与加密 key。显式白名单 + 测试里对 `app/Cache`、`app/Key`、`app/Crypto` 各下一条红线断言 |
| D4 | `%LOCALAPPDATA%/Zoom` 顶层只取缓存目录名，不写 `**` | 顶层直接躺着 `Login Data` / `Cookies` / `Local State` / `Network`。写 `%LOCALAPPDATA%/Zoom/**` 必中。用 `{Cache,Code Cache,GPUCache,...}` 花括号枚举 Chromium 缓存目录 |
| D5 | 录制根只进 detect，不进 scope | `Documents/Zoom` 是用户内容。放进 detect 让用户能看见 Zoom 卡片、知道数据在哪；放进 scope 就是事故。disclaimer 显式点名 + 测试里 6 条红线断言 |
| D6 | `downloads` 桶保留期而非全量 | 该桶名来自社区脚本描述，本机无 Zoom 无法核对是否存在、内容是否真为安装包归档。给 30 天保留期，且路径不存在时匹配为空，代价最小 |
| D7 | 全部 `recycle_granularity = "directory"` | `WebRTC` / `logs` 内是小文件量极大的目录。按文件回收会产生上千个回收站条目、耗时数分钟；按目录回收一个逻辑单元一条目（与 obs-cache.toml 同形态） |
| D8 | 日志 / 遥测给 `days=30`，纯缓存给 `none` | 日志与遥测有排查价值，全量删会把「上周那场会为什么卡」的证据也删了；WebRTC 与媒体缓存无保留价值，全量清 |
| D9 | 不放 `name_contains` | 裸 `"Zoom"` 会误命中 ZoomCar / ZoomInfo / 用户自建 `Desktop/Zoom`。检测只靠具体路径（同 discord.toml 的处理） |
| D10 | `risk = "low"` | 6 个 scope 全部是可重生缓存，无任何 scope 触碰用户内容或账号数据。红线靠 glob 锚定 + 测试双保险，不靠 risk 级别 |

## 5. 实测附录（本机无 Zoom，记录与初始假设的差异）

本节记录「初始假设」与「可用证据」之间对不上的地方，供下一轮有 Zoom 的机器上复核。

1. **假设**：本机可勘测 `%APPDATA%/zoom.us` 与 `%APPDATA%/Zoom` 的真实子目录。
   **实际**：两处都不存在，`find` 按名匹配零结果。所有 glob 均未经本机路径验证。
2. **假设**：`%LOCALAPPDATA%/Zoom/app/` 下存在 `logs` 与 `Crashpad` 两个可清目录。
   **实际**：无法验证。**风险**：`app/` 的真实子目录清单未知，`{logs,Crashpad}` 是
   Electron 常见布局的推测。缓解：白名单而非遍历——即使真实子目录是别的名字，
   最坏结果是这两个 scope 匹配为空（漏清），不会误删。这是本次刻意选择的失败方向。
3. **假设**：安装包缓存桶名为 `downloads`。
   **实际**：无法验证，来自社区清理脚本的公开描述。**风险**：若真实桶名是别的
   （如 `setup` / `installer`），该 scope 匹配为空。缓解：只给单一路径 + 保留期，不铺开。
4. **假设**：`media-cache` 与 `WebRTC` 同级、同属可清临时数据。
   **实际**：无法验证。`media-cache` 的桶名来自社区脚本描述。**风险**：若
   `media-cache` 实际存放用户可见的历史媒体副本（而非缩略图缓存），误删不可恢复。
   缓解：disclaimer 提示「历史消息里的图片与附件会显示已过期」，并默认进回收站可还原。
   **建议**：下一轮实测确认 `data/media-cache` 内容后再决定是否保留此 scope。
5. **假设**：`%APPDATA%/zoom*/data/im/` 是聊天 / 会议消息 DB 目录。
   **实际**：无法验证目录名，但「Zoom 本地有消息 DB」这一点与社区描述一致。
   该路径在红线断言中覆盖（`data/im/Message.db` / `.db-wal` / `.db-shm` /
   `data/im/Meetings/Meeting.db`），即便真实文件名不同，`data/im` 也未被任何
   scope 覆盖——因为 D1 决定不写 `data/**`。
6. **假设**：新版客户端数据根为 `%APPDATA%/Zoom`（大写 Z）。
   **实际**：无法验证，且 `globset` 配置为 `case_insensitive = true`，
   `zoom` / `Zoom` 大小写差异不影响匹配。detect 同时列了 `**/zoom.us` 与 `**/Zoom`。
7. **未验证的跨平台**：Zoom macOS 数据根（`~/Library/Application Support/Zoom`）
   与 Linux（`~/.config/zoom`）本次未覆盖。本 scaffold 定位 Windows（AGENTS.md），
   detect 也不含跨平台路径，非 Windows 上不会出卡片。若后续要支持，需另开需求。

### 5.1 下一轮实测清单（有 Zoom 的机器上跑）

```powershell
# 只列目录名，不读任何文件内容（隐私铁律）
ls "$env:APPDATA\zoom.us" "$env:APPDATA\zoom.us\data"
ls "$env:APPDATA\Zoom" "$env:APPDATA\Zoom\data"
ls "$env:LOCALAPPDATA\Zoom" "$env:LOCALAPPDATA\Zoom\app"
ls "$env:USERPROFILE\Documents\Zoom" "$env:USERPROFILE\Documents\ZoomRecordings"
```

重点复核 D6 / §5.2 / §5.4 三项，以及 `app/` 的真实子目录清单（§5.2）。

## 6. 红线断言覆盖清单

[crates/scaffold/tests/zoom_safety.rs](../../crates/scaffold/tests/zoom_safety.rs) 共
77 条红线断言（45 条 Zoom 专属 + `red_line_samples()` 通用清单 32 条），分组如下：

| 分组 | 条数 | 覆盖路径 |
|---|---|---|
| 用户录制（第一红线） | 6 | `Documents/Zoom/{MyMeeting,Recordings}` 下的 `.mp4` / `.aitxt`、`Documents/ZoomRecordings/` 下的 `.mp4` / `Chat.txt` / `Thumbnails/*.jpg` |
| 账号配置 | 4 | `zoom.us/data/zoom.us.conf`、`data/zoom.exe.conf`、`data/config/{settings.ini,preferences.json}` |
| 聊天 / 会议 DB | 4 | `data/im/Message.db`、`.db-wal`、`.db-shm`、`data/im/Meetings/Meeting.db` |
| 登录态 / 账号状态 | 2 | `data/login/session.dat`、`data/Accounts/.../settings.dat` |
| Electron 顶层 Chromium 凭据 | 9 | `Local/Zoom/{Login Data,Cookies,Local State,Preferences,First Run,Session Storage,Local Storage,IndexedDB,Network,Trust Tokens}` |
| `app/` 内 Chromium 状态 + 加密 key | 8 | `Local/Zoom/app/{Network,Local Storage,Session Storage,IndexedDB,key,Key,crypto,Crypto}` |
| 锚定陷阱（app 内缓存 / data 外同名目录 / 日志与遥测的相邻目录 / 归档桶相邻目录） | 6 | `app/Cache`、`app/GPUCache`、`zoom.us/WebRTC`（data 外）、`data/log`、`data/telemetry-cache`、`downloads2` |
| 同名应用 / 用户自建目录 | 5 | `zoom.us-extra`、`ZoomCache`、`ZoomInfo`、`Desktop/Zoom`、`Documents/ZoomCache` |
| 通用红线 | crate 内置 32 条 | `*.db` / `.db-wal` / `.db-shm`、`Accounts`、`login`、`config`、`Favorite`、`Fav`、`key`、`crypto`、Chrome / Firefox 登录数据、`.ssh`、`.aws`、`.config/gcloud`、`pagefile.sys`、`hiberfil.sys`、`$Recycle.Bin`、`System Volume Information`、Docker / WSL vhdx |

正向断言 10 条，覆盖全部 6 个 scope id（每个 ≥1 条）。

**验证结果**：

```text
$ cargo run -p diskpilot-scaffold-lint -- scaffolds/zoom.toml
ok: scaffolds/zoom.toml (zoom, 6 scopes, no red-line hits)

$ cargo test -p diskpilot-scaffold --test zoom_safety
running 1 test
test zoom_globs_are_safe ... ok
test result: ok. 1 passed; 0 failed
```

## 7. Disclaimer 文案核对

- 「绝不删 X / Y / Z」：已列 Documents/Zoom、ZoomRecordings、im DB、zoom.us.conf、
  Chromium 登录态与加密物料。
- 「删除后某些功能可能受影响」：已提示历史图片 / 附件显示「已过期」。
- 「走系统回收站，可还原」：已写「清理走系统回收站，可还原」。
- **不含「安全」二字**：已核对，disclaimer 全文无此词。

## 8. 隐私核对

勘测全程只用 `ls` / `find -iname` 列目录名（且全部命中「不存在」），
未读取任何 Zoom 配置文件、缓存内容或数据库。本附录引用的 Zoom 目录结构
来自公开信息（社区清理脚本描述 + Electron 通用布局），未从本机读取。
