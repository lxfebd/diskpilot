# DiskPilot 安全模型（威胁模型与防线总览）

> 读者：安全审查者、插件/脚本作者、关心"它到底能删什么"的用户。
> 本文描述 DiskPilot 面对的威胁、每一层防线落在哪个文件，以及 R505 深度审查后的加固点。
> 代码是唯一事实来源——本文只做导航，具体以源码为准。

## 威胁模型（我们防什么）

| # | 威胁 | 攻击面 | 主防线 |
|---|---|---|---|
| T1 | 误删用户数据 / 系统文件 | 清理执行路径（scope / 回收 / 删除 / 撤销） | 红线清单 + protected_path + 确认门 + 回收站默认 |
| T2 | 路径穿越（`..` / 尾空格 / ADS / UNC / 设备名） | 插件 zip 解压、台账路径拼接、launch entry | `ensure_path_under` / 段级校验 / UNC 拒绝 |
| T3 | 插件市场供应链（篡改索引、伪造签名、zip 炸弹） | 远程索引下载、zip 安装、更新回滚 | 信封 Ed25519 + sha256 + 包内签名 + signer 锚定 + 解压上限 |
| T4 | SSRF（AI 代理 / 下载 / MCP HTTP 指向内网、元数据服务） | 所有出站 HTTP | 字面量私网 + DNS 二次校验总闸 |
| T5 | 提示注入 / XSS 后 webview 直接 invoke 写命令 | Tauri 命令面 | 后端独立 confirmed + 权限门 + 审计日志 |
| T6 | MCP 工具权限逃逸（前缀导致映射落空、L3 被降级） | `mcp_call_tool` | 剥前缀后查映射 + writable 最低 L1 + 审计 |
| T7 | 本地文件被篡改后当作可信输入（undo.jsonl / 台账 / 缓存半截文件） | undo 恢复、回滚、缓存命中 | source 侧保护校验 + blob_hash 比对 + tmp+rename 原子落盘 |
| T8 | UAC 骗取（插件自带 requireAdministrator） | 插件 launch | 插件入口禁 runas |

## 防线分层（代码定位）

### L1 清理执行层（`crates/executor`）

- `protected_path`（[crates/executor/src/protect.rs](../crates/executor/src/protect.rs)）：canonicalize 解析 symlink/junction 后判定 + 词法折叠 fail-closed 兜底；拒绝盘根 / 系统目录 / 用户主目录根；**R505 新增 UNC 网络共享拒绝**。
- `execute` 入口先对计划内每条路径跑 `protected_path`；Delete 分支在删除前再跑一次 TOCTOU 复检；**R505 Quarantine 分支同样复检**，隔离文件名加纳秒后缀防撞车。
- `restore_quarantined`：**R505 对 source 侧跑 protected_path**（防篡改 undo.jsonl 把隔离文件植回系统目录）。
- 默认 Recycle（系统回收站），可选 quarantine；每条写 `undo.jsonl`（原子 tmp+rename）。

### L2 脚本红线（`crates/scaffold` + scaffold-lint）

- `red_line_violations` 是 CI、生成器、运行时共享的同一份锚定清单；命中即硬错误。
- **R505**：`execute_scope` 在真实执行前对 scaffold 再跑一次红线复验（落盘后的 TOML 可能被改动，安装期校验不是运行期唯一防线）。

### L3 命令层确认 + 权限门（`apps/desktop/src-tauri`）

- `confirm_gate`（R508 前）：所有真实写操作要求 `confirmed == Some(true)`，前端两步确认窗通过后才带参。
- **R505 新增后端独立门**：`undo`、`install_scaffold`、`uninstall_scaffold` 后端各自校验 confirmed + 权限（`scaffold.manage`）；`execute_scope` / `recycle_paths` 真实执行要求后端权限 `cleanup.execute`（与前端权限中心同步的 `perm_grants` 二次校验，防 webview 注入绕过）。
- **R508 确认闸门完整方案**：7 个写命令（execute_scope / recycle_paths / undo / install_scaffold / uninstall_scaffold / toolbelt_recycle / toolbelt_run 的 medium+ 分支）统一走 `confirm_nonce::write_gate` = confirmed==true **+ 一次性 nonce 消费（60s 有效、绑定 op、用后即焚，防 invoke 参数重放）** + 频率限制（同 op 1s≤1 次、60s≤10 次）+ 主窗口来源校验（label 必须是 main）+ 签发审计。前端 `withConfirmNonce` 在确认动作时申请 nonce 随命令提交。
- `sync_perms`：**R505 起权限变更全部写审计日志**（granted/revoked 明细），可追溯"谁在何时打开了哪些高权限"。

### L4 插件安装与市场（`crates/toolbelt` + `plugin_registry`）

- **签名链（R504/R505）**：索引信封 Ed25519 强制验签 → 条目 sha256 下载后强校验 → 包内 Ed25519 验签（`plugin_zip_has_ed25519` 严格口径：verify 必须显式 ed25519 且 signature+signer 俱全，空签名拒绝）→ signer 锚定索引声明。
- **解压安全（R505）**：条目数 ≤4096、单条 ≤512MB、总解压 ≤2GB；段级拒绝 `..`、尾空格/点（Windows 规范化成穿越）、含 `:` 的 ADS、保留设备名（CON/PRN/…）；entry 经 `validate_plugin_entry` 校验且解压后二次确认存在且在插件目录内（`ensure_path_under` fail-closed）。
- **回滚安全（R505）**：成功分支不再误删新安装目录；备份 blob 装前强制比对 `blob_hash`，不符回退重下载；下载全程 tmp+rename 原子落盘，缓存命中在索引无 sha256 时要求非空文件。
- **启动安全（R505）**：`toolbelt_launch` 的 exe 经 `ensure_path_under(Tools 根)`；插件入口遇 ERROR_ELEVATION_REQUIRED 一律拒绝 runas（防骗 UAC）。

### L5 出站网络 SSRF 总闸（`apps/desktop/src-tauri/src/ai_ssrf.rs`）

- `ssrf_block_reason` = IP 字面量私网判定 + 域名解析后**全部** A/AAAA 记录须为公网（fail-closed）；拒绝 `localhost.` 尾点变体、IPv6 zone-id。
- 覆盖：AI 代理首跳 + 每跳重定向（`ai.rs`）、`ssrf_safe_get` 首跳 + 重定向（`net_http.rs`）、插件/脚手架社区索引 URL、MCP HTTP 端点（`validate_http_url` 改 async，**每次 connect 重验**，`mcp_bridge.rs` / `mcp_crud.rs`）。
- DNS 解析走 `spawn_blocking`，不卡 async executor。

### L6 MCP 调用侧权限（`mcp.rs` / `mcp_tools.rs`）

- **R505**：调用侧先用 `strip_tool_prefix` 剥掉 `tool_prefix` 得到原始名，再查 `permission_map`（此前带前缀查必然落空，L3 永禁被回退 L0 放行）；转发给服务器也传原始名。
- 不变式：`writable=true` 的服务器即使映射到 L0 也升级至少 L1（需确认）；审计日志带 `raw_tool`。

## 审计与追溯

- MCP 调用审计：`mcp_audit.rs`（op / server / raw_tool / perm / 结果）。
- **R505**：`sync_perms` 权限快照变更写审计（granted/revoked/after）。
- 撤销台账：`~/.diskpilot/undo.jsonl`（原子追加，上限裁剪保留最新 N 条）。

## 已知边界（坦诚说明）

- 本机同用户进程可篡改本地文件（undo.jsonl / 台账 / 缓存）——防线是"篡改后仍不放行危险动作"（L1/L3/L7 校验），不是防篡改本身。
- macOS / Linux 构建未在真机验证；签名/公证未做。
- R508/R509 的 nonce 防**重放**不防实时 XSS：同源 webview 内被注入的脚本仍可自行申请 nonce（Tauri IPC 区分不了脚本与真实点击）——纵深防线是 capability 白名单 + 来源校验 + 频率限制 + 审计追溯，不是 nonce 本身。
- **R509 起写命令 nonce 覆盖率 100%**：19 个写命令（7 清理链 + 10 插件管理 + execute_ai_plan 真执行 + toolbelt_run 的 medium+ 分支）全部要求一次性 nonce + 主窗口来源校验 + 频率限制；execute_ai_plan 的 dry_run 预演除外（纯只读）。
- `sync_perms` 只做审计加固（R505），权限快照本身仍可被 webview 伪造——缓解手段是写命令同时要求 nonce + 来源校验（R508/R509）。
