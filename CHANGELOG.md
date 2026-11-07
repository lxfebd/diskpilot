DiskPilot 变更日志。格式遵循 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，
版本号遵循 [SemVer](https://semver.org/lang/zh-CN/)。

逐轮开发记录（R1–R408，共 300 条）按日期分卷归档在
[docs/changelog/](docs/changelog/README.md)；本文件只保留当前未发布的
近期轮次（R381–R408）与已发布版本摘要。

## [未发布]

### R515: MCP 写工具纳入 nonce 闸——agent_call_tool 的 11 个写工具封死 webview 伪造 confirmed 路径

- **审计结论**：R508/R509 覆盖了清理链 + 插件链 + execute_ai_plan，但 agent-server MCP 桥接（agent_call_tool）是另一条 IPC 路径，11 个写工具（process_kill / service_control / fan_selfheal_fix / fan_control / file_recycle / process_start / scheduled_task_manage / uninstall_app / toolbelt_run / stress_test / stress_test_gpu）此前只有 confirmed==Some(true) 硬校验，webview 内脚本可伪造 confirmed 直接调。
- **改造**：NONCE_OPS 新增 `agent_write`；`agent_call_tool` 命令注入 `window` + `confirm_nonce` 参数，写工具分支在命令层先过 `require_main_window` + `write_gate`（一次性 nonce + 频率限制），通过后把工具名写入审计日志（op=agent_write）；inner 层原 confirmed 硬校验保留作纵深防御；只读工具零影响。
- **前端**：agent.ts 的 callTool 在 confirmed===true 时经 `withConfirmNonce('agent_write')` 申请 nonce 后提交。
- **回归**：`all_whitelisted_ops_can_issue` 自动覆盖新 op；cargo check/tsc 零错误。
- **效果**：现在主窗口发出的**所有**写操作（清理/插件/AI 计划/MCP 工具）都要求一次性 nonce + 来源校验 + 频率限制，webview 伪造 confirmed 的攻击面清零。

### R514: 发布前验收清单固化——scripts/pre-release-check.mjs 一键跑完 14 项门禁（pnpm check:release）

- **新增 `scripts/pre-release-check.mjs`**：按 CI 同口径顺序跑 8 大类门禁——cargo fmt --check → clippy -D warnings → cargo audit（白名单 RUSTSEC-2024-0370/0429，白名单外即红）→ cargo test --workspace → plugins/ 下 7 个插件各自 test+build --release（独立 target dir，不污染主构建）→ 前端 tsc+eslint+vitest → check-changelog + check-docs-links → `diskpilot-desktop --self-test` 无头冒烟。逐项 ✅/❌ + 末尾汇总，任一失败 exit 1。
- **模式**：默认完整跑；`--quick` 跳过 audit/test/self-test（约 30s）；`--skip-plugins` 跳过插件构建。配了 `pnpm check:release` / `check:release:quick` 两个 npm 脚本。
- **修复本机坑**：Windows 下 pnpm 是 pnpm.cmd，spawnSync 无 shell 找不到——前端三项在 win32 下自动走 shell:true（记入脚本注释）。
- **验证**：`--quick` 全模式 14 项全绿（含 7 插件），本机逐门禁与手工跑命令结果一致。
- **docs/README.md** 索引收录该脚本，发布者一眼能找到。

### R513: 无头 self-test 三平台 CI 冒烟——清掉「桌面运行时未验证」已知边界

- **`diskpilot-desktop --self-test`**（R513 新增，main.rs 拦截参数后直接跑 `self_test::run()` 退出，不建窗口）：5 项断言——① scanner 扫描临时目录 files_seen==3；② Ed25519 生成密钥对→签名→验签通过 + 篡改 payload 被拒；③ nonce 签发/一次性消费/重复拒绝/op 不匹配不吞 nonce；④ 频率限制（1s 窗口内连发只放行 1 次）；⑤ write_gate dry_run 放行 + 无确认真写被拒。全部 PASS exit 0，任一 FAIL exit 1。
- **本机验证**：`--self-test` 输出 5×PASS，exit=0（过程中修复两处自身 bug：generate_keypair 返回序是 (公钥,私钥)、rate_limit 断言口径）。
- **CI**：ci.yml 新增 `self-test` job，ubuntu/macos/windows 三平台矩阵，装完 webkit 依赖后 `cargo run -p diskpilot-desktop -- --self-test`。
- **security-model.md 已知边界更新**：macOS/Linux 真机运行时改为「CI 三平台 self-test 覆盖核心安全路径，仅纯 GUI 交互（窗口/托盘点击）未自动化；macOS 签名/公证仍未做」。

### R512: 官方插件第四批——drive-health（磁盘健康审计器），索引仓库发 v1.3.0，CI 绿，线上 7 个插件全量验签通过

- **`plugins/drive-health/`**（独立 workspace，与前三批同款发布配置）：PowerShell 存储模块只读审计——Get-PhysicalDisk 物理盘表（型号/介质/总线/HealthStatus/OperationalStatus/容量，非 Healthy 标红警告）+ Get-Disk 逻辑盘表 + `--smart` 追加 SATA 盘 SMART 高级属性（NVMe 或非管理员下该 cmdlet 不可用，明确标注而非静默跳过）；手写 CSV 解析器（引号/逗号转义）零依赖；4 例单测绿；exe 247KB。
- **只读铁律**：不写盘、不改电源计划、不跑 chkdsk，报告末尾提示走主程序确认链路。
- **实机冒烟**：5 块盘（3 NVMe SSD + 2 SATA HDD）健康表输出正确，全部 Healthy，耗时 1.7s；--smart 模式对 2 块 SATA 盘正确标注「需管理员+ATA/SCSI 驱动支持」。
- **发布**：pub512 打包签名 zip（sha256=`d1080efc6baf348bba6fa9b57c587352eaad5f88f9d9ebfa1540f7d11b9144cc`，126552 B）→ release **v1.3.0**（id 437846765）→ index.json 第 7 条 + 重签信封（签名 `83f2efe27c31cd97…`，commit `5ab23e1a`）→ 本地验签安装通过 → **线上 verify_online：7 个插件全部 信封验签+sha256+包内 ed25519+signer 锚定 通过** → CI validate-index run 42414740264 success。
- **文档**：README.md / README_EN.md 官方插件数 6→7，docs/plugin-market-index.md §4 增 drive-health 条目 + v1.3.0 发布说明。

### R511: 扫描性能专项——walk 压测子命令 + 实机基线建档 + 修复 bench feature 编译断裂

- **`diskpilot-scanner walk <目录> [重复次数]`**（跨平台，bench feature）：每次 run 输出 wall/files/s/GB/s + phase 分解（walk/build_tree/total），末行 BEST/AVG；默认 3 次取最佳，供优化前后回归对比。
- **修复既存断裂**：`--features bench` 构建失败（mft.rs 的 bench 模块漏引 `crate::mft_parse::{fixup_all, parse_record}`），补 use 后 bench/mem/walk 全部可构建。
- **实机基线（24 核 Win11）**：C:\Windows 73k 文件/19.6GB 最佳 0.86s（84.7k files/s，22.6 GB/s）；C:\Program Files 76k/31.9GB 0.83s（90.8k files/s，38.3 GB/s，SSD）；J 盘 90.4 万文件/70.8GB 13.9s（65k files/s，5.1 GB/s，WD20EZRX 机械盘≈介质极限）。**结论：瓶颈在 walk IO（占比 96–97%），build_tree < 4%（此前已消除二次 read_dir），软件层无明显优化空间**。
- **新文档 docs/performance-baseline.md**：基线表 + 压测命令 + 回归口径（BEST wall 回退阈值 10%），docs/README.md 索引收录。
- **验证**：bench 构建零错误、3 组压测完成、cargo fmt/clippy/test 全绿。

### R510: 官方插件第三批——port-hunter（端口占用审计器），索引仓库发 v1.2.0，CI 绿，线上全量验签通过

- **`plugins/port-hunter/`**（独立 workspace，与前两批同款发布配置）：解析 `netstat -ano` 拿 (协议/本地地址/外端/状态/PID)，`tasklist /FO CSV /NH` 批量映射 PID→进程名；按本地端口排序输出 TopN 占用表 + 摘要（TCP/UDP 计数、LISTENING 数、涉及进程数）；参数 `[TopN] [--proto tcp|udp|all] [--listen]`；GBK 输出 lossy 兜底；3 例解析单元测试绿；exe 134KB（opt-level=z+lto）。
- **只读铁律**：不杀进程、不改配置，报告末尾提示走主程序确认链路。Windows 专属（netstat/tasklist）。
- **实机冒烟**：`port-hunter.exe 8 --listen` 0.41s 出 67 条 LISTENING 记录、34 个进程，端口/PID/进程名准确（135→svchost、139/445→System）。
- **发布**：pub510 打包签名 zip（sha256=`ffe7d6d4f1124856518c8e85467174c0559c51cc38f7e3d7331d61132c767737`，134434 B）→ 上传 index 仓库 release **v1.2.0**（id 437805149）→ index.json 追加第 6 条并重签信封（签名 `ed60444cd74cb9a1…`，commit `4de01bfa`）→ `install_plugin_zip` 自验装进临时 Tools 根通过。
- **线上验证**：verify_online 拉 live index（6 条）+ 全部 zip——信封 Ed25519 验签通过，每个插件 sha256 一致 + 包内 ed25519 有效 + signer 锚定官方公钥。CI validate-index run 42408413282 = success。
- **文档**：README.md / README_EN.md 官方插件数 5→6，docs/plugin-market-index.md §4 增 port-hunter 条目 + v1.2.0 发布说明。

### R509: nonce 闸补齐——插件 10 个写命令 + execute_ai_plan 全部纳入（R508 留白清零），写命令覆盖率 100%

- **NONCE_OPS 白名单扩容 12 个**：execute_ai_plan / plugin_uninstall / plugin_install / plugin_install_url / plugin_activate / plugin_deactivate / plugin_export / plugin_registry_install / plugin_registry_update / plugin_registry_rollback / plugin_set_registry_url / plugin_registry_refresh。
- **后端**：上述 11 个命令全部注入 `window: tauri::Window`（来源校验 label==main）+ `confirm_nonce` 参数，原 `confirmed` 门统一替换为 `write_gate`（confirmed + 一次性 nonce + 频率限制三件套），各自的 plugin.manage / cleanup.execute 权限门保持不变。execute_ai_plan 的 dry_run 预演仍纯只读放行，真执行走完整闸门。
- **前端**：system-ext.ts 10 个插件写 api 全部经 `withConfirmNonce(op)` 申请 nonce 后提交；clean.ts 的 executeAiPlan（真执行分支）同口径，executeAiPlanDryRun 预演不申请 nonce。
- **回归测试**：新增 `all_whitelisted_ops_can_issue`——白名单内每个 op 签发+消费全绿，防止后续新增 op 忘记登记。
- **效果**：R508 之后遗留的「webview 可伪造 confirmed 调插件写命令 / AI 计划」两条路径全部封死——现在所有 19 个写命令（7 清理链 + 12 插件/AI）都要求一次性 nonce + 主窗口来源 + 频率限制。
- **验证**：cargo check/clippy 零错误、cargo test --workspace 全绿、前端 tsc + vitest + eslint 全绿。

### R508: 写操作确认闸门完整方案——一次性 nonce（防重放）+ 主窗口来源校验 + 频率限制 + 全量审计，覆盖 7 个写命令

- **新模块 `confirm_nonce.rs`**：后端内存签发一次性确认 nonce（getrandom 32 字节、60s 有效期、绑定 op、消费即失效、表上限 256 自动清旧），白名单 op = execute_scope / recycle_paths / undo / install_scaffold / uninstall_scaffold / toolbelt_recycle / toolbelt_run；频率限制 = 同 op 1 秒 ≤1 次、60 秒窗口 ≤10 次；签发写审计日志。
- **总闸 `write_gate`**：dry_run 永远放行；真实写必须 confirmed==true **且** nonce 消费成功 **且** 频率通过——三者任一失败都拒绝落盘。`consume` 先查后删：op 不匹配/过期不消费 nonce（留给合法调用方）。
- **来源校验**：7 个写命令全部注入 `window: tauri::Window`，拒绝 label != "main" 的调用（防非主窗口/注入 webview 直接 invoke）。
- **命令改造**：execute_scope / recycle_paths / undo / install_scaffold / uninstall_scaffold / toolbelt_recycle 全量接入 `confirm_nonce` 参数；toolbelt_run 对 medium+ 风险工具走完整闸门（fail-closed：算不出风险等级按高风险处理），low 风险只需来源校验 + cli.run 权限。
- **前端**：新增 `api/confirm.ts` `withConfirmNonce(op, run)`——确认动作先申请 nonce 再随命令提交；clean.ts（installScaffold / uninstallScaffold / executeScope 非 dryRun / undo / recyclePaths）与 system-ext.ts（toolbeltRun confirmed 分支 / toolbeltRecycle）全部接入。dry-run 预览不申请 nonce（只读）。
- **已知边界（诚实声明）**：同源 webview 内的实时 XSS 仍可自行申请 nonce（Tauri IPC 无法区分脚本与真实点击），nonce 防的是**重放**；真正防线 = capability 白名单 + 来源校验 + 频率限制 + 审计追溯。execute_ai_plan（AI 路径）保持原 userConfirmed+权限双门，未纳入 nonce（避免破坏 AI 确认流），列为后续项。
- **测试**：confirm_nonce 6 例单元测试（签发/一次性消费/重放拒绝/op 不匹配不消费/白名单拒绝/频率限制）+ write_gate 门测试（dry_run 放行、缺 confirmed 拒绝、缺 nonce 拒绝、合法通过）；executor 旧 confirm_gate 测试迁移到 write_gate 口径。
- **验证**：cargo fmt/clippy -D warnings 干净、cargo audit 仅 2 条已知告警、cargo test --workspace 全绿、前端 tsc + vitest 1305/1305 + eslint 全绿。

### R507: 官方插件第二批上线——temp-hunter（临时目录扫描器）+ log-hunter（大日志扫描器），索引仓库发 v1.1.0，CI 绿

- **新插件**（源码入主仓库 `plugins/<id>/`，纯 std Rust 小 exe，均只读、risk=low，删除统一走主程序确认链路）：
  - `temp-hunter` 临时目录扫描器：扫描 `%TEMP%` + `%SystemRoot%\Temp`，按年龄分桶（>30天/7-30天/<7天）统计 + TopN 最大文件；内置深度 8 层 / 20 万文件上限，超大 Temp 提前收尾并标注"报告为下界"（实测本机 20 万文件 32s 扫完、34GB 合计）。
  - `log-hunter` 大日志扫描器：扫描 `.log/.dmp/.wer/.hdmp/.mdmp/.trace/.etl` 及轮转压缩形态（`app.log.3`、`app.20261101.log.gz`），阈值 + TopN 报告；含 3 例单元测试（常见扩展名 / 轮转形态 / 非日志拒绝）。
- **发布链路（与 R504 同口径）**：`sign_plugin_dir`（Ed25519，私钥 `~/.diskpilot/release-signing/secret.hex` 不入库）→ `export_plugin_zip` → 上传索引仓库 Releases `v1.1.0` → `index.json` 追加两条（signer 锚定官方公钥）→ 信封重签（payload = `serde_json::to_string(&plugins)`）→ push；索引仓库 CI `validate-index` 绿。
- **端到端验证（线上真实数据）**：拉取线上 index.json 信封验签通过（signer=官方公钥 `4eac4122…af00a5`）；5 个线上 zip 的 sha256 与索引逐字节一致；每个包内 Ed25519 有效且包内 signer 锚定索引声明；本地 `install_plugin_zip`（verify=ed25519 强制验签）安装两个新包成功且 entry 存在；两个 exe 实际运行输出正常报告。
- 文档：README 中英文 + `docs/plugin-market-index.md` §4 更新为 5 个官方插件与 v1.1.0 说明。
- 验证：新插件 `cargo test` 3/3 绿、主仓库门禁不受影响（插件独立 workspace）、check-changelog/check-docs-links 通过、前端 vitest 1305/tsc/eslint 全绿。

### R506: 文档轮——README_EN 全量同步到 v0.2.4 最新结构 + 新增 docs/security-model.md 威胁模型文档

- `README_EN.md` 重写对齐中文主 README：Release v0.2.4 徽章、下载表（Win/mac/Linux + .sig + 自动更新）、v0.2.4–v0.2.2 更新日志、三张 Demo 截图、7 大功能块（含 76 个 MCP 工具、硬件中心、插件市场官方索引已上线）、6 层安全模型（含 R505 纵深复验）、隐私、架构、路线图、开发/贡献（此前仍停留在"尚未发布安装包"的旧口径）。
- 中文 `README.md` 同步修正两处过期表述：插件市场段改为官方索引仓库已上线（`lxfebd/diskpilot-plugin-index`、首批 3 个只读诊断插件、完整签名链描述）；安全模型新增第 6 层"后端纵深复验（R505）"。
- 新增 [docs/security-model.md](docs/security-model.md)：8 类威胁（T1–T8）× 主防线对照表 + L1–L6 各层防线代码定位（protect.rs / scaffold 红线 / 确认门 / 插件签名链 / SSRF 总闸 / MCP 权限）+ 审计追溯 + 已知边界（本地篡改防不住但危险动作仍被拒、macOS/Linux 未真机验证、sync_perms nonce 方案待设计）；挂入 docs/README.md 索引与中英文主 README 文档段。
- 验证：check-docs-links 全部命中、check-changelog 通过、前端 vitest/tsc/eslint 全绿（纯文档改动，Rust 侧不受影响）。

### R505: 深度安全审查修复——签名契约收紧、zip 炸弹上限、路径穿越加强、SSRF 加 DNS 二次校验、MCP 前缀权限失配、写操作确认门补洞

- **签名契约收紧（2 个 critical）**：`plugin_zip_has_ed25519` 改为严格口径（verify 必须显式 `ed25519` 且 signature+signer 俱全，堵 `verify="sha256"`+伪造字段绕过远程签名门槛）；`install_plugin_zip` 的 require 分支新增 signature 非空断言（此前空签名静默 Ok = 零校验安装）。
- **zip 炸弹上限**：条目数 4096 / 单条 512MB / 总解压 2GB 三道上限，签名阶段改 `Take` 流式读不再信 size()；解压段拒绝尾空格/点穿越段（`".. "` 经 Windows 规范化成 `..`）、含 `:` 的 ADS 段与保留设备名（CON/PRN/…）。
- **路径穿越加强**：`safe_join`（台账 dir_rel/blob）同样按段 trim 尾点空格后判 `..`/空段/盘符段；`toolbelt_launch` / `toolbelt_recycle` 的 exe/目录经 `ensure_path_under`（canonicalize 后 starts_with Tools 根，fail-closed）；插件 entry 校验（`validate_plugin_entry`）拒绝穿越/绝对/保留名，且解压后二次校验 entry 存在且在插件目录内。
- **rollback 修复**：成功分支此前 `rename(tmp→dir)` 必败被吞后误删刚装好的版本——改为删 `tmp_dir`；失败分支去掉危险的 `remove_dir_all(&dir)`；备份 blob 装前强制比对 `blob_hash`（不符回退重下载）；`sweep_orphaned_tmp_dirs` 改递归两层（tmp 目录实际落在分类层）+ 跳过 symlink。
- **SSRF 加 DNS 二次校验**：新增 `ssrf_block_reason`（字面量 + 域名解析全部 A/AAAA 记录须为公网，fail-closed；拒 `localhost.` 尾点、zone-id IPv6），ai_proxy 首跳与每跳重定向、`ssrf_safe_get`、插件/脚手架索引 URL、MCP HTTP 端点（`validate_http_url` 改 async 且每次 connect 重验）统一走总闸；出站走 spawn_blocking 不卡 executor。
- **MCP 前缀权限失配**：`mcp_call_tool` 调用侧改用剥前缀后的原始名查 `permission_map`（此前带前缀名必然落空、L3 永禁被回退 L0 放行），转发给服务器也改传原始名；`writable=true` 服务器即使映射 L0 也升级至少 L1 要确认；审计新增 `raw_tool` 字段。
- **写操作确认门补洞**：`undo` 命令新增 `confirmed==Some(true)` 门 + 前端 UndoPanel/HistoryPage 改走 ConfirmDialog；`restore_quarantined` 对 source 跑 protected_path 校验（防篡改台账把隔离文件植回系统目录）；`install_scaffold`/`uninstall_scaffold` 新增 confirmed + `scaffold.manage` 双门；`execute_scope`/`recycle_paths` 真实执行新增后端 `cleanup.execute` 权限门 + 执行期红线复验（与 CI lint 同锚定空间）；Quarantine 分支加 TOCTOU 复检 + 隔离文件名加纳秒后缀防撞车；`protected_path` 新增 UNC 拒绝。
- **其他加固**：插件入口（第三方 zip）遇 ERROR_ELEVATION_REQUIRED 一律拒绝 runas（防自带 requireAdministrator 的 exe 骗 UAC）；远程下载改 tmp+rename 原子落盘，索引无 sha256 时缓存命中要求非空文件；`sync_perms` 权限变更全部写审计日志（granted/revoked 明细）。
- **新增回归测试**：尾空格/ADS zip 段拒绝、空 signature 拒绝、safe_join 尾空格段、MCP 前缀剥离后 L3 命中、SSRF DNS 二次校验（localhost./zone-id/元数据）、MCP HTTP 端点拒内网、restore 拒受保护 source、UNC 拒绝——共 10+ 例。
- **验证**：fmt clean、clippy workspace `-D warnings` 0、`cargo test --workspace` 全绿、vitest/tsc/eslint 全过、check-changelog/check-docs-links 通过（见下）。

### R504: 插件市场首批官方内容上线——新建官方索引仓库 + 3 个只读诊断插件（全部 Ed25519 双签），客户端默认拉官方索引

- **官方索引仓库** `lxfebd/diskpilot-plugin-index`（新建，public）：`index.json` 信封带 Ed25519 签名 + CI 门禁（schema=1 / https / sha256 必填 / id kebab / 条目 signer 与信封一致）；插件 zip 上传在仓库 Releases `v1.0.0`。
- **首批 3 个官方插件**（源码入主仓库 `plugins/<id>/`，纯 std Rust 编译的小 exe，全部 **只读诊断**、risk=low，不做任何删除——删除统一走主程序的确认链路）：
  - `bigfile-finder` 大文件扫描器（硬盘工具）：递归扫描按大小列 TopN，跳过符号链接防目录环
  - `dup-finder` 重复文件查找（硬盘工具）：大小分组 + 全量 SipHash 比对，输出标注「疑似重复」需人工确认
  - `startup-audit` 启动项审计（综合检测）：只读列 HKCU/HKLM Run 键 + 启动文件夹
  - 每个包经 `sign_plugin_dir` 签名（包内 `signature`/`signer`/`digest`，verify=ed25519）后 `export_plugin_zip` 打包
- **客户端默认索引**：`load_registry_url` 未配置时回落 `OFFICIAL_REGISTRY_URL`（官方 raw 地址）——用户打开市场即有内容，仍可换自定义社区索引。
- **安全机制（纵深）**：索引信封签名强制验签（篡改保留旧索引）→ 条目 sha256 下载后强校验 → 包内 Ed25519 验签 → R502 起 signer 锚定（包内 signer 必须等于索引声明，冒名自签名被拒）→ 安装路径 safe_join 防穿越 + zip 条目防 `../`。
- **验证（端到端实测线上数据）**：拉取线上 index.json 验信封签名通过/篡改拒绝；3 个线上 zip 的 sha256 与索引逐字节一致；包签名有效且 signer 锚定一致；`install_plugin_zip` 落到 `硬盘工具|综合检测/<id>/` 且 entry 存在；篡改 zip 一字节被内容验签拒绝；安装产物 exe 实际运行正常。新增永久回归测试：信封签名往返+篡改拒绝、默认索引回落。
- 发布私钥保管：`~/.diskpilot/release-signing/secret.hex`（icacls 收紧为仅当前用户，绝不入任何仓库），公钥 `4eac4122…af00a5` 写入信封与条目 signer。
- 文档：`docs/plugin-market-index.md` §4 改为已上线的官方仓库说明，publish-guide 指向真实仓库。
- 验证：fmt clean、clippy `-D warnings` 0、`cargo test --workspace` 全绿（新增 3 例）、vitest 1305/tsc/eslint 全过、check-changelog/check-docs-links 通过。

### R503: 插件市场小修两处——resolve 最新版改按版本号实际比较（不信任索引倒序）+ verify 命令 Ed25519 验签只算一次

- `RegistryPlugin::resolve(None)` 原取 `versions[0]` 当最新（靠"索引倒序"约定）；改为 `cmp_version` 按主.次.修订数字比较取最大（正式版 > 预发布版），乱序索引也正确。新增 2 例测试（版本号比较、乱序选最大）。
- `plugin_registry_verify` 原调了两次 `env.verify()`，合并为一次计算后复用到 `envelope_ok`/`envelope_error`。
- 验证：fmt clean、clippy `-D warnings` 0、`cargo test --workspace` 全绿、vitest 1305 全过。

### R502: 插件市场安全加固——台账路径防穿越（safe_join）+ 注册表安装签名者锚定（包内 signer 必须等于索引声明）

- 巡检插件市场发现两处缺口并修复：
  - **HIGH 路径穿越**：`plugin_registry_update` / `plugin_registry_rollback` 直接把台账（本地 JSON，可被篡改）里的 `dir_rel` / `blob` / `from_ver` 拼进 `root.join(...)` 后做 rename / remove_dir_all，含 `../` 时可删改 Tools 根之外的任意目录。新增 `preg_misc::safe_join`（拒绝空、绝对路径、`..`/根成分），update/rollback 的目录路径与备份包路径全部改走 safe_join；另有单测覆盖。
  - **MEDIUM 签名者未锚定**：注册表安装只验包内 Ed25519 自洽，未校验包内 signer 是否等于索引条目声明的 signer——未签名索引下可冒名自签名。现在 `require_registry_signed` 新增 `expected_signer` 参数（toolbelt 新增 `plugin_zip_signer` 读取包内 signer），install/update/rollback 重下路径强制比对，且索引条目未声明 signer 的一律拒绝远程安装。
  - 顺手：`parse_and_verify_envelope` 逐条校验索引插件 id 为 kebab-case（净化自洽），堵住 id 拼缓存/备份文件名的穿越面；toolbelt 抽出 `read_plugin_meta` 复用清单读取。
- 新测试：safe_join 穿越拒绝 3 例、check_signer_anchor 匹配/不匹配/空 signer 4 例、envelope 非法 id 拒绝 3 例、plugin_zip_signer 读取 2 例。
- 验证：`cargo fmt --all -- --check` 干净，clippy workspace `-D warnings` 0，`cargo test --workspace` 全 suite 绿（含新增 9 例），audit 仅 2 个已知 allowed warnings，前端 vitest 1305 tests / tsc / eslint 全绿。

### R501: 补跑全量门禁发现 R497 测试代码 fmt 不 clean——cargo fmt --all 修复（8 行缩进），clippy -D warnings 全 workspace 重跑通过

### R500: 发布准备——版本号三处 bump 到 0.2.4，CHANGELOG 未发布区封版为 [0.2.4] - 2026-11-02，用户向更新日志草稿 docs/releases/v0.2.4.md

- `Cargo.toml` workspace version、`apps/desktop/package.json`、`tauri.conf.json` 三处 0.2.3→0.2.4 一致；Cargo.lock 内 10 个 workspace crate 版本同步（cargo metadata 刷新）。
- CHANGELOG `[未发布]` 段整体封版为 `## [0.2.4] - 2026-11-02`（R485–R499，15 条），未发布区清空待下一轮。
- 新增面向用户的发布说明草稿 [docs/releases/v0.2.4.md](docs/releases/v0.2.4.md)（亮点：社区脚本闭环 / 键盘可访问性专项 / ? 快捷键面板；工程化：CI 根因修复 + audit 清零 + 例行依赖更新），GitHub Release 描述可直接粘贴。
- 发布流水线核对：`.github/workflows/release.yml` push tag `v*` 触发 5 平台矩阵（win/mac x64+arm64/linux），tauri-action 出 draft release + latest.json（updater 端点 `releases/latest/download/latest.json`，签名公钥已写入 tauri.conf.json，私钥走 Actions Secret `TAURI_SIGNING_PRIVATE_KEY`），SHA256SUMS 按 target 分文件上传。发版动作 = `git tag v0.2.4 && git push origin v0.2.4`。
- 验证：check-changelog / check-docs-links 通过（63 条链接）；前端 91 files / 1305 tests 全绿、tsc + eslint 0；`cargo test --workspace` 46 suite 全 ok。

## [0.2.4] - 2026-11-02

### R499: 文档同步——README 路线图勾选「用户自制清理脚本闭环」（R497 远程索引 + R498 作者签名 + R8 安装验签）

### R498: 作者侧上架工具——scaffold-lint 新增 sign 子命令，给 TOML 做 Ed25519 签名

- 缺口：社区脚本安装强制 Ed25519 签名（R8/R497），但作者没有签名工具，上架链路缺"作者侧"。
- `crates/scaffold-lint` 新增 `sign <file.toml> [--key <secret_hex>]` 子命令（新增 diskpilot-toolbelt 依赖复用同一签名实现）：先跑 lint（不合格脚本拒绝签名），再对 TOML 全文签名（entries 形态 `[("scaffold.toml", 内容)]`，与安装端 `scaffold_registry_install` 验签零漂移）；不带 `--key` 时现场生成密钥对并一次性打印私钥。输出 `signer`/`signature` 填入社区索引条目。
- 纯函数 `sign_scaffold_text` 抽出便于单测：3 个新测试（签名→验签往返、篡改内容验签失败、非法私钥拒绝）。
- 文档：`docs/scaffold-authoring.md` 新增「§5 签名并上架社区索引」（签名命令、私钥保管告警、用户端刷新入口说明），章节号顺延。
- 验证：`cargo test --workspace` 46 个 suite 全 ok（scaffold-lint 11 tests），clippy `-D warnings`、fmt 干净。

### R497: 社区清理脚本索引支持远程刷新——官方索引一键拉取，Ed25519 信封验签，安装链路不变

- 缺口：`scaffold_registry`（R8）只有本地索引 + 手动导入替换，社区脚本永远停留在种子占位——分享渠道缺"远程拉取"这一环。
- 后端（`scaffold_registry.rs`）：新增索引信封 `ScaffoldEnvelope`（schema/source_url/fetched_at/signature/signer + `scaffolds[]`，对齐插件市场 `RegistryEnvelope`）；`parse_scaffold_envelope` 纯函数完成 JSON → schema=1 校验 → 非空 → 条目 id kebab-case 校验 → 可选 Ed25519 验签（签名对象为 `scaffolds[]` 的规范 JSON，复用 `toolbelt::signature::verify_ed25519`）→ 补 source_url/fetched_at。
- 新命令 `scaffold_registry_refresh`（已注册 lib.rs）：不传 URL 时拉官方索引 `lxfebd/diskpilot-scaffolds-index`（与 `docs/index.example.json` 的 source_url 同源）；自定义 URL 同样强制 https + `blocked_private_target` + `ssrf_safe_get`（8MB 上限）；写操作走 confirmed=true + `scaffold.manage` 双硬校验；原子写入 `scaffolds-registry.json`，失败保留旧索引。`load_registry` 兼容新旧两种本地格式（信封优先、裸数组兜底、种子兜底），`scaffold_registry_list` 新增返回 source_url/fetched_at/verified。
- 安装链路零放宽：`scaffold_registry_install` 仍强制条目级 Ed25519 签名 + sha256 + 红线校验后才落盘。
- 前端：`api.scaffoldRegistryRefresh` + mock；Studio 社区仓库区加「刷新索引」按钮与来源/更新时间/未签名提示行（`.studio-community-toolbar`）；i18n 新增 7 键中英成对（英文数字量词走 ICU 复数，过 coverage 守卫）。
- 测试：Rust 侧 6 个新单测（schema 拒绝 / 空索引拒绝 / 非法 id 拒绝 / 未签名 verified=false / 坏签名 Err / 合法签名 verified=true，签名用 `generate_keypair`+`sign_entries` 现场构造）；前端 `commands.test.ts` 三向契约守卫自动覆盖新命令（声明⇄注册⇄invoke）。
- 验证：`cargo test -p diskpilot-desktop` 全绿（scaffold_registry 10 tests）、clippy `-D warnings` 过、fmt 干净；前端 91 files / 1305 tests 全绿，tsc + eslint 0。

### R496: 前端例行依赖更新——10 个包升到 package.json 声明范围内最新，主版本不跨

- 与 R495（Rust 侧）对称的前端例行更新。定向 `pnpm update` 只升 semver 兼容范围内最新：
  `@tauri-apps/api` 2.11.0→2.12.4、`@tauri-apps/cli` →2.12.4、plugin-dialog 2.7.0→2.8.4、plugin-fs 2.5.0→2.7.1、plugin-process 2.3.1→2.4.0、plugin-shell 2.3.5→2.6.0、plugin-updater 2.12.0→2.14.1（恰好与 R495 后 Cargo.lock 里 Rust 侧 tauri 2.12.5 / 各插件 2.x 最新对齐，JS-Rust 协议版本一致）、`@tanstack/react-virtual` 3.14.10→3.14.25、`@typescript-eslint/*` 8.59.1→8.72.1。
- 有意跳过的 major 跳变（需专项评估，不混入例行更新）：`@types/react` 18→19、`@vitejs/plugin-react` 4→6、`eslint` 9→10。
- 本机 store 与全局 config 不一致（node_modules 链接自 `tools/.pnpm-store`），更新时用 `--store-dir` 指回原 store，未动全局配置。
- 验证：`pnpm -C apps/desktop exec vitest run` 91 files / 1305 tests 全绿；tsc -b 与 eslint 0 warning。

### R495: 例行依赖更新——Cargo.lock 113 个包升到兼容范围内最新，构建链（aws-lc-sys/cc/embed-resource 等）精确锁回原版不动

- `cargo update` 全量后，把会触发 C 重编或构建链行为变化的包 `--precise` 回退原版：aws-lc-sys 0.45.0 / aws-lc-rs 1.18.1 / cc 1.4.5 / find-msvc-tools 0.1.12 / embed-resource 3.0.11（winreg 随 embed-resource 回到 0.55.0）——规避本机无 NASM 时 aws-lc-sys 冷构建失败（见 .github/CONTRIBUTING.md 的长路径/MSVC 绕过说明）。
- 其余 100+ 个包升级到 semver 兼容范围内最新（tokio 1.54.1、hyper 1.12.0、uuid 1.29.0 等 minor；bitflags/libc/log 等 patch）。
- 验证：`cargo test --workspace` 46 个测试套件全部 ok；`cargo clippy --workspace --all-targets` 0 warning；`cargo audit` 告警 3 → 2（均为既有 allow 的间接依赖 unmaintained/unsound，无新增高危）。

### R494: 键盘快捷键帮助面板——? 键全局唤出，把散落的快捷键集中成一张可查的表

- 缺口：应用里已有 Esc 关窗栈（R349）、Shift+F10 右键菜单（R368）、搜索框 Esc 清空（R439）、方向键走行/走卡/走块（R491–R493）、死像素空格翻页等快捷键，但没有一处集中可查——键盘用户只能摸索发现，读屏用户更无文字清单。
- 改动（`App.tsx`）：全局 keydown 监听 `?` 键切换帮助面板；输入框 / 可编辑区 / 输入法组合期（`isComposing`，R458 口径）一律放行，那里的 ? 是正常输入。
- 新组件 `KeyboardHelp.tsx`：`modal-bg` + `role="dialog" aria-modal="true"`（同既有弹窗口径），接入 `useEscapeKey`（R349 Esc 栈）与 `useDialogFocus`（焦点进场/陷阱），分 7 组列出全部快捷键（全局 / 浏览与选中 / 菜单与搜索 / 工具卡片 / 设置标签 / 死像素 / AI 对话）。
- 文案：`shell.help.*` 20 个键，中英各一份（i18n parity 守卫自动盯住）。
- 样式（`app-shell.css`）：`.help-modal` 限宽 560px、限高 72vh 可滚动，分组标题 + 条目列表。
- 守卫：新增 `keyboardHelp.test.ts` 三条——① App 的 ? 监听与输入框/IME 豁免；② 面板的 Esc 栈 / 焦点管理 / dialog 语义；③ i18n 键中英成对且非空。
- 验证：`pnpm -C apps/desktop exec vitest run` 90+1 files / 1305 tests 全绿；tsc -b 与 eslint 0 warning。

### R493: Treemap 空间图块可键盘操作——listbox/option + roving tabindex，方向键走块、Enter 选中

- 缺口：`Treemap.tsx` 的块是 `<g onClick>`——纯鼠标。键盘用户看得见空间图、点不了任何块（旁边树视图早已支持键盘，treemap 是主视图之一却是盲区）。
- 改动（`Treemap.tsx`）：svg 改 `role="listbox"`（保留 aria-label），每个块 `role="option"` + `aria-selected` + roving `tabIndex`（`i === focusIdx ? 0 : -1`，Tab 只停一块）+ `data-tm-idx`；svg 上挂 `onKeyDown={onSvgKey}`——方向键按布局顺序环绕（复用 `cycleIndex`，R365 口径）、Home/End 跳首尾、焦点 `blocks?.[next]?.focus()`；Enter/空格走 `isActivationKey` 触发与鼠标单击同源的 `onSelect(path, node)`；`node/width/height` 一变落点回第一块。
- 改动（`layout.css`）：`.treemap g:focus-visible rect { stroke: var(--accent-strong); stroke-width: 2.5px; }`——键盘焦点块加亮描边（CSS 规则优先于 rect 的 presentation attribute，选中白边在聚焦时让位给焦点框）。
- 守卫：`a11y.test.ts` 新增 describe「Treemap 块的键盘操作（R493）」两条——① listbox/option/roving tabIndex/激活键同源/方向键五支与焦点回写/落点重置；② layout.css 的 `:focus-visible rect` 描边规则。
- 验证：`pnpm -C apps/desktop exec vitest run` 90 files / 1302 tests 全绿；tsc -b 与 eslint 0 warning。

### R492: 工具墙卡片可键盘操作——roving tabindex + 网格方向键，Enter 开详情 / Shift+Enter 启动

- 缺口：`ToolCard` 是可点 div（单击开详情、双击启动），但没有 role/tabIndex/onKeyDown——键盘用户 Tab 跳过整面工具墙，90+ 张卡片对他们不存在；就算逐卡可 Tab 也会变成上百个停靠点。
- 改动（`toolwall.tsx`）：① 卡片 `role="button"` + roving `tabIndex`（落点 `cardFocus`，搜索/分类/排序/风险筛/最近列表一变即清空回第一张），`data-focus-key` 供方向键回写；② 两个 `.toolwall-grid` 容器挂 `onKeyDown={onGridKeys}`——左右 ±1、上下 ±列数（列数从 `getComputedStyle(grid).gridTemplateColumns` 实读，读不到按 1 列）、Home/End 跳首尾，环绕语义复用 `cycleIndex`（R365）；③ 键盘与鼠标两路一一对应：Enter/空格走 `activateOnKey(() => onOpenDetail(tool))`（= 单击），Shift+Enter 且可启动走 `onLaunch()`（= 双击）——启动确认门（中/高危就地确认、L2 双确认）在 `onCardClick` 内完全不变。
- 改动（`dark.css`）：`.toolwall-card:focus-visible { outline: 2px solid var(--accent-strong); outline-offset: -2px; }`（R491 同款焦点描边）。
- 文案：`toolbelt.card.clickHint` 中英改为「单击 / Enter 看详情 · 双击 / Shift+Enter 启动」，键盘提示上墙。
- 守卫：`a11y.test.ts` 新增 describe「工具墙卡片的键盘操作（R492）」两条——① role/tabIndex/data-focus-key、Enter 详情与 Shift+Enter 启动分支、网格方向键五支与列数读取、落点重置；② dark.css 的 `:focus-visible` 规则 + clickHint 中英两条都含 Enter。
- 验证：`pnpm -C apps/desktop exec vitest run` 90 files / 1300 tests 全绿；tsc -b 与 eslint 0 warning。

### R491: Steam 盘点游戏行可键盘操作——roving tabindex + 方向键走行，不再几十个 Tab 停靠点

- 缺口：SteamInspector 的游戏行是可点选的 div（role="option"），但没有 tabIndex——键盘用户 Tab 不进列表，想用键盘勾选只能靠鼠标；一装几十个游戏的书库若按原生做法每个行都可 Tab，又会变成几十个停靠点。
- 改动（`SteamInspector.tsx`）：① roving tabindex——行带下标，落点 `tabIndex={i === rowFocus ? 0 : -1}`，Tab 进列表只停一行；② 搜索/透视切换时行集合变化，`useEffect` 把落点收回第一行；③ 方向键（下/上/Home/End）挂在列表容器的 keydown 上（挂点、卸载时 removeEventListener），焦点在哪就在哪落、没聚焦任何行时默认第一行；④ 行自己的 Enter/空格走既有 `activateOnKey(select)`（R314 口径），与鼠标 `onClick` 是同一条 `select` 函数——键盘与鼠标行为一致。
- 改动（`steam.css`）：新增 `.steam-row:focus-visible { outline: 2px solid var(--accent-strong); outline-offset: -2px; }`——键盘聚焦行有可见描边（R348 口径：键盘焦点亮出、鼠标点击仍是背景换色）。
- 守卫：`a11y.test.ts` 新增 describe「SteamInspector 游戏行的键盘操作（R491）」两条——① roving 落点、`activateOnKey(select)` 与 `onClick={select}` 同源、方向键四支与挂点/卸载语句齐全；② steam.css 的 `:focus-visible` 规则含两条 outline 声明（注释剥离后匹配）。
- 验证：`pnpm -C apps/desktop exec vitest run` 90 files / 1298 tests 全绿；tsc -b 与 eslint 0 warning。

### R490: 补齐工具数 76→77 的全部残留硬编码断言与文案（CI test-rust windows 失败根因）

  - R489 的 CI run 41315244461 显示 test-rust(windows) 失败：agent_tests.rs 的 agent_list_tools_returns_76_tools 断言 76，R485 加 observations_overview 后实际 77（该测试仅在 agent-server 二进制存在时跑，本地漏网）。
  - 修复：agent_tests.rs 断言 76→77、测试改名 _77_tools、消息 65 只读→66 只读、抽查名单加 observations_overview；同文件头注释 75→77。
  - 文案同步：agent.rs 头注释 3 处、mcp.rs:7、advisor/tools.ts:1076、crates/agent-server/README.md:18、crates/README.md:9、docs/architecture.md:86、docs/guide.md:93 全部 76→77、65→66。
  - 经验：工具数漂移应以 R484 的 tool_catalog_guard（doc vs router 名单）为唯一真相源；硬编码数字断言（agent_tests、tools.test.ts）是第二漏点，加新工具时必须同步改。

### R489: unix clippy -D warnings 清零——Windows 专属死代码按平台门控

 - 背景：R488 修掉 E0432 后，lint-rust(macos/ubuntu) 暴露下一层：clippy -D warnings 下 unix 侧 13 处死代码（Windows 专属功能的 helper 在 unix 无调用方）。
 - 门控 #[cfg(windows)]：webview.rs watchdog 重建闩（recreate_pending + atomic import）、startup.rs 的 FOLDER_REL、executor.rs 测试里的 Node import；net_http.rs 的 `let mut b` 加 cfg_attr(not(windows), allow(unused_mut))。
 - cfg_attr(not(windows), allow(dead_code))（保留单测全平台跑）：drives.rs 与 agent-server disk.rs 的 parse_subst_line/parse_subst_output/apply_subst_marks、system.rs 的 busy_delta_pct、steam.rs 的 residue_lines、net_proxy.rs 的 not(windows) 回退桩。
 - scan.rs:389 的 unit_arg/clone_on_copy（unix 下 USN 游标退化为 ()）改为语句级 cfg_attr allow，保证 usn_cursors 字段在 unix 仍被读、不引入新 dead_code。
 - 验证：windows 门禁全绿（fmt/clippy -D warnings/test）；unix 以 CI lint-rust(macos/ubuntu) 为准。

### R488: 修复 main CI 三根因——unix 编译门控、node20 无 navigator、cargo audit 15 个漏洞全部清零

 - Unix 编译（lint-rust/test-rust macos+ubuntu，E0432）：crates/psutil/src/lib.rs 的 escape_ps_single_quoted 去掉 #[cfg(windows)] 门控（纯字符串翻倍函数无平台依赖，startup.rs:37 在三平台都被引用）；安全单测同步去门控，全平台跑。
 - frontend（node 20 无全局 navigator，ReferenceError）：apps/desktop/src/advisor/tools.ts:533 与 apps/desktop/src/hwProfile.ts:53 的裸 navigator 访问改为 typeof 守卫（与同文件既有模式一致）。
 - cargo audit 15 个漏洞清零：rustls 0.23.45→0.23.46、rustls-webpki→0.103.16、webpki-root-certs→1.0.11、quinn-proto→0.11.19（cargo update 语义兼容自动取大）；tauri-plugin-updater 2.12.0→2.14.1（其 zip 依赖 ^4→^9，锁里 zip 4.6.1 消失）；workspace 根 Cargo.toml 的 zip "2"→"9"，toolbelt 五处 ZipFile::name() 调用适配 zip 9 新签名（返回 Result<Cow<str>>，解码失败跳过条目）。audit 余 3 个 allowed warnings（unmaintained 提示），0 漏洞。
 - 验证：cargo fmt --check、cargo test -p diskpilot-toolbelt/-p diskpilot-psutil/-p agent-server、cargo audit 全绿；clippy --workspace -D warnings 与 pnpm check 见提交时补跑记录。

### R486: 周报增强——扫描趋势环比 + 观测层异常告警，巡检从单次快照升级为带趋势/异常的洞察

- 后端（apps/desktop/src-tauri/src/weekly_report.rs）：WeeklyReport 新增 #[serde(default)] scan_trends / obs_alerts 两个 Vec<String>（旧归档照常反序列化，前端按字段缺失整块不显示）。生成时 ① scan_summary::read_summaries 按 root 分组取最近两条算环比，格式化「C:\ 文件 +N，体量 ±X（距上次 N 天）」，不足两条或时间倒序的 root 跳过不编造；② observations::build_overview（R482 改 pub(crate) 复用）的 domains 里 corrupt_lines>0 或带 error 的域格式化为告警行。两个数据源失败都降级为空，不让周报整体作废。纯函数 scan_trend_lines / obs_alert_lines + 两条单测（跨周环比取最近两条、corrupt/error 才告警）。
- 前端：api/clean.ts WeeklyReport 加可选 scan_trends / obs_alerts；WeeklyHealthCard 新增「扫描趋势」「观测异常」两个区块（复用 wh-delta 样式，字段存在才渲染）；i18n namespaces/history.ts 中英各加 4 键（coverage 守卫全绿）。
- 验证：cargo fmt --check、cargo test -p diskpilot-desktop、clippy --workspace --all-targets -D warnings、pnpm -C apps/desktop check、check-changelog、check-docs-links 全绿（见下）。

### R485: observations_overview——MCP 侧观测总览只读工具，一眼看尽这台机器被观测成什么样

- 改动：crates/agent-server/src/tools/report/rep_observations.rs 新增（纯函数 build_overview + 入口 observations_overview），复刻桌面端 observations.rs（R482）的域表与族规：jsonl 域 entries/corrupt_lines/latest、state 域 present、hw-history/ 目录快照数+最新文件名；缺文件=真空态、读取失败/全损坏=error（R88 诚实性）。经新域 tool_observations.rs 注册为第 77 个 MCP 工具（纯只读 L0，不采集不写入）。单测覆盖混合域索引与全损坏=error 两条。
- 文档：docs/mcp-tools.md 表格+详述新增 observations_overview；工具计数 76→77、只读 65→66 同步到 docs/health-butler.md 与 website/index.html（R484 守卫测试保证此后名单与文档强一致）。
- 验证：cargo test -p agent-server 全绿（含 tool_catalog_guard 新名单断言与 rep_observations 单测）；clippy --workspace --all-targets -D warnings 干净；cargo fmt --check 干净。

### R484: MCP 工具目录守卫测试——tools/list 与 docs/mcp-tools.md 强一致，76≠75 漂移不再漏网

- 根因：2026-10-13 commit 56142ee 加入第 76 个工具时仓库内没有任何计数断言，CI test-rust（cargo test --workspace）自然拦不住文档漂移。
- 改动：tools/mod.rs 追加 #[cfg(test)] mod tool_catalog_guard——doc_tool_names() 解析 docs/mcp-tools.md 全部表格行，tool_router_matches_mcp_tools_doc 比较 AgentServer::tool_router().list_all() 与文档（先断数量、再断名单，失败信息提示同步更新文档）。
- 验证：cargo test -p agent-server 97 passed（守卫测试绿）。

### R483: 清理 agent-server 过期数据路径文案，统一走 DISKPILOT_DATA_DIR（R482 单根）

- 改动：rep_storage.rs 新增 data_dir_label()（用户可见文案一律用实际解析路径）；rep_report.rs 输出、tool_hw_report.rs 的 #[tool] description（字面量）、report.rs 注释、docs/mcp-tools.md、docs/health-butler.md、website/index.html、smoke_full.py 全部去掉硬编码 %LOCALAPPDATA%/diskpilot/agent-server/ 与 75 计数（改为 76 / 65 只读 + 11 写）。
- 验证：cargo fmt --check 干净；clippy -D warnings 干净（exit 0，仅上游 binrw future-incompat 备注）。

### R477: 搜索结果为空也播报一声——读屏不再「打了字却听不到结果为空」

- 缺口：七个过滤器/搜索框结果为空时只把列表换成一句灰字，没有任何 live region——读屏用户输入关键字、列表清空，屏上没有任何播报，只能靠手动逐行重扫确认「没有匹配」；键盘与触屏用户同样得不到结果反馈。
- 改动：七个空态节点补 `role="status"`（polite 播报，出现即朗读）——`HistoryPage` 的 `history.empty` / `history.emptyFiltered`（无记录与无匹配两条）、`StartupPanel` 的 `system.startup.emptyFiltered`、`SystemTools` 的 `system.driver.noneMatched`、`toolwall` 的 `toolbelt.wall.noMatch`/`catEmpty` 外层容器、`market` 的 `toolbelt.mkt.noMatch`、`FileView` 的 `overview.fileview.empty`、`SteamInspector` 的 `steam.noMatches`。`market` 的 loading 态保持无声（不是结果为空、不该插嘴）。
- 守卫：`a11y.test.ts` 新增 describe「空结果播报（R477）」两条——① 七个站点逐一路径断言精确文本（HistoryPage 两条都在、其余各 1 处）；② market 的 loading 文案 `toolbelt.mkt.loading` 仍是无 `role="status"` 的 `p.muted`。反向自检删 SystemTools 一处 role → 恰好 1 failed 点名 `./components/SystemTools.tsx`，还原。
- 验证：`pnpm -C apps/desktop check` 90 files / 1296 tests 全绿。

### R476: 市场两个 URL 输入框回车即提交——粘贴完按 Enter 不再没反应

- 缺口：市场「URL 直装」输入框与进阶「索引 URL」输入框都没有回车处理（全仓无 `<form>`，不回车不提交）——从插件包地址/索引地址粘贴完按 Enter 毫无反应，必须摸到旁边按钮；更糟的是索引 URL 的保存逻辑整段内联在按钮的 `onClick` 里，无独立函数可复用的。
- 改动（`toolbelt/market.tsx`）：① 把索引 URL 保存逻辑抽成 `saveRegistryUrl` 回调（`useCallback`，含两步确认 `confirmAsk`、`api.pluginSetRegistryUrl`、成功后 `setVerifyOut(null)` 清旧校验结论的 R346 逻辑原样保留），按钮 `onClick` 改调它；② 两个输入框各加 `onKeyDown`：`Enter` 且非 IME 组合中（`!e.nativeEvent.isComposing`，R404 同口径）即提交——URL 直装走 `void installByUrl()`，索引 URL 走 `saveRegistryUrl()`（额外 `!regBusy` 防在途重入）。
- 守卫：`a11y.test.ts` 新增 describe「市场 URL 输入框的回车提交（R476）」两条——① 两个输入框都有 Enter 守卫（`if (e.key === 'Enter' && !e.nativeEvent.isComposing` 恰 2 处）、`void installByUrl();`、`&& !regBusy) { e.preventDefault(); saveRegistryUrl(); }`、`onClick={() => saveRegistryUrl()}`，且旧的整段内联 `onClick={() => { const u = registryUrl.trim();` 写法消失；② `saveRegistryUrl` 切片仍含确认、`pluginSetRegistryUrl`、`setVerifyOut(null)`、`setConfirmAsk(null)`（重构不丢行为）。反向自检删索引框回车行 → 恰好 2 failed 点名还原。
- 验证：`pnpm -C apps/desktop check` 90 files / 1294 tests 全绿。

### R475: 状态色与亮底标签的文字对比度收口——换主题不再读出 1.1:1 的隐形字

- 缺口：`--mint/--sun/--lavender/--accent/--err` 都会当实心底色画在徽标、chip、横幅、气泡与危险按钮上，而此前一律配 `--ink`（5 个深色主题里是白字）或写死的 `#fff`——实测 minimal 用户气泡 1.19:1、motionfit 徽标 1.11:1、banner.preview 1.07:1、trae 危险按钮 3.88:1（WCAG 正文要 4.5）。另：`--ink-3/--muted` 落在 `--paper-1/2/3` 上共 173 对不达标（brand 2.6 / doubao 2.8 / 21th 2.81 等，属浅灰副文案，本轮不改）。
- 改动（`theme.ts`）：新增 `relLuminance()`（WCAG 线性化）与 `contrastRatio()`（1..21）两个纯函数；`--on-accent` 从「按亮度拍脑袋」改为「白字/深字取对比度更优者」，深字候选定为 `#101010`（`#1a1a1a` 在高饱和红/紫上只剩 4.2~4.4:1）；新增 `--on-tint = #101010`（亮底状态字）与 `--on-danger`（`--err` 上白字/深字取优）；vibecamp 与 21th 的 `--err` / `--risk-danger` 从 `#d64527` 压暗到 `#c23a1d`（白字 4.49 → 5.37）。
- 改动（CSS，全部保留 `var(--on-X, var(--ink))` 回退）：用户气泡 → `--on-accent`；系统气泡 / `button.secondary` / `.badge` / `.banner.preview` / `.ok` / `.card-meta strong` / 三处 `.trace-chip` / `.tool-card-icon` → `--on-tint`；`.banner.error` / `.error` → `--on-danger`；`.advice-pill .badge` 显式补 `color: var(--ink)`（卡片底不再继承 tint 暗字）；`.ao-strip-all` → `--on-accent`。危险三按钮（`button.danger` / `button.primary.stop` / `.cleanup-execute.armed`）文字 `#fff` → `var(--on-danger, #fff)`。
- 守卫：新增 `contrast.test.ts` 8 条——17 个预设必需 token 齐全、`--ink` 在 bg/card/paper 上 ≥4.5、`--on-accent`/`--on-tint`/`--on-danger` 全主题 ≥4.5、`applyTheme` 真写出 token、CSS 里不再有「`--ink` 落在实心亮底」组合、`--risk-danger` 底的危险按钮不再写死白字。反向自检：把 `--err` 改回 `#d64527` / 还原 `button.primary.stop` 的白字 → 各 1 failed 点名还原。
- 验证：`pnpm -C apps/desktop check` 90 files / 1292 tests 全绿；`scripts/check-changelog.mjs` 与 `scripts/check-docs-links.mjs` 通过。

### R474: 权限「恢复默认」加确认带——一键不再把用户打开的开关静默关掉

- 缺口：权限中心右上角「恢复默认」点一下立刻把 22 个开关重置为出厂值，而默认表是双向的：`power.plan` / `cleanup.execute` / `plugin.manage` / `scaffold.manage` / `startup.manage` / `fan.adjust` / `fan.control` / `driver.check` / `mcp.manage` / `sys.control` / `file.recycle` / `app.uninstall` 默认关（会静默关掉用户手动打开的），`hw.stress` / `hw.export` / `cli.run` 默认开（会重新打开用户特意关掉的压测、导出与工具执行权限）。没有确认、没有结果反馈、没有撤销，误点一次就要逐项重调。
- 改动：`PermissionCenter.tsx` 加两步确认——图例按钮改为打开确认带（`role="alert"`，正文说明两个方向的影响），确认带里「取消」（autoFocus）与「确认恢复」（danger）；执行后 toast「已恢复默认权限」；取消与执行两条路都把焦点还给「恢复默认」按钮（确认带卸载后焦点会掉到 body）。`dark.css` 新增 `.perm-reset-confirm` / `.perm-reset-text` / `.perm-reset-confirm-actions` 三条规则（`--risk-caution` 描边 + 10% 底色，随主题走）。新增四个文案键：`perm.center.resetConfirm` / `resetCancel` / `resetYes` / `resetDone`（中英成对）。
- 守卫：`a11y.test.ts` 新增 describe「权限恢复默认的两步确认（R474）」两条——① 状态与接线（`confirmReset` 声明、图例按钮走 `setConfirmReset(true)` 且带 `resetBtnRef`、取消 autoFocus、确认走 danger、`role="alert"`、`resetBtnRef.current?.focus();` 恰 2 处、旧的一键 `onClick={reset} title=` 已消失）；② 四个键中英各出现两次。反向自检把图例按钮改回一键 → 恰好 1 failed 点名该断言。
- 验证：`pnpm -C apps/desktop check` 89 files / 1284 tests 全绿。

### R473: 错误提示带上动作上下文——不再只扔一行裸 os error

- 缺口：六处 catch 把后端错误原文直接塞进提示条（`toast(String(e), 'err')`）：工具箱卡片的「读取用量」、CLI 的「运行」、硬件聚合卡片的首次加载与刷新、工具启动、网络检测。失败时用户只看到 `os error 5` / `spawn ENOENT`，不知道是哪一步失败，也分不清是权限、路径还是文件缺失。
- 改动：新增五个文案键（中英成对）并把六处接上——`toolbelt.wall.usageFailed`（读取工具用量失败：{msg}）、`toolbelt.cli.runFailed`（运行工具失败：{msg}）、`toolbelt.hw.loadFailed`（读取硬件信息失败：{msg}）、`toolbelt.wall.launchFailed`（启动工具失败：{msg}）、`toolbelt.net.probeFailed`（网络连通性测试失败：{msg}）。硬件聚合卡片的 effect 依赖数组同步补 `t`（否则 react-hooks 报缺失依赖）。
- 守卫：新增 `apps/desktop/src/toastCopy.test.ts` 四条——① 解析器不空转（非测试源码 >50 个文件、两个 glob 分别命中 `store.ts` 与 `App.tsx`、`toast(` 总数 >50）；② 全仓 `toast(` 的第一个参数不得以裸 `String(` 开头；③ 错误信息必须是表达式而不是被引号包住的字面量（本轮踩坑：写成 `{ msg: 'String(e ...)' }` 会让界面原样显示那行代码）；④ 五个键中英各出现两次。反向自检把 services.tsx 那处还原成裸 `String(` → 恰好 1 failed 点名 `./components/toolbelt/services.tsx:49`。
- 验证：`pnpm -C apps/desktop check` 89 files / 1282 tests 全绿，eslint 0 warning。

### R472: AI 回答里的表格斑马行随主题走——深色主题下正文对比度从 3.16:1 回到 9.39:1

- **缺口**：聊天气泡里的 Markdown 表格隔行底色是写死的 `rgba(255, 234, 242, 0.35)`（浅粉）。浅色主题下叠在近白气泡上是「淡淡的粉」，深色主题下同样这层粉叠在深卡片（`#111112`）上就变成 `#645D60` 的中灰粉——正文用的 `--ink-2`（`#b8b4c4`）压在上面只有 **3.16:1**，低于 WCAG AA 正文门槛 4.5:1；同一张表里没被涂色的行是 9.31:1。等于深色主题下每张表的隔行都变成「糊掉的那一行」。
- **实证**：用真实 `chat.css` + `dark` 预设的 40 个变量搭了一页探针，两张表并排量合成色与对比度——旧写法合成 `#645D60` / 3.16:1，改成 `color-mix(in srgb, var(--accent) 8%, transparent)` 后合成 `16,16,17` / **9.39:1**（浅色主题下仍是淡色调，只是跟着主题 accent 走）。
- **改动**：`styles/chat.css` 斑马行背景换成 `color-mix(in srgb, var(--accent) 8%, transparent)`，并在规则上方记下这两个数（3.16 → 9.39），下次谁想改回去先看见代价。
- **守卫**：`token-guard.test.ts` 的 rgba 基线 `./chat.css` 从 14 降到 **13**（只许减不许增的既有机制，写回去即红）；另加一条定向断言——斑马行规则块里必须是 `color-mix(in srgb, var(--accent) 8%, transparent)`、且不许出现 `rgba(`，防止「换个别的 rgba 顶掉计数」绕过基线。
- **反向自检**：把规则改回 `rgba(255, 234, 242, 0.35)` → 恰好 2 条红（基线 `./chat.css：14 处 > 基线 13` + 定向断言），还原后 6 passed。
- **验证**：`pnpm check` 88 files / 1278 tests 全绿。

### R471: 英文数字文案的单复数收口——「1 files」「3 day ago」不再出现在界面上

- **缺口**：R350 立了 `{n:one|other}` 复数语法（`1 day` / `3 days`），但只有十来个键真的用了；全仓还有两百多处「数字 + 名词」直接拼死，英文界面一旦数量是 1 就出 `1 files` / `3 day ago` / `1 copies in the group` / `Reasoning · 1 steps`。反过来又有一批键写成单数（`{n} match`、`{n} image attached`），数量大于 1 时同样错。
- **更隐蔽的一层**：不少调用方把数字**本地化后**传进来（`file_count.toLocaleString()` → `"1,234"`），`Number("1,234")` 是 `NaN`。旧实现遇到非数字参数会把 `{n:file|files}` **原样漏到界面上**，一换语法就变成满屏花括号。
- **改动**：13 个命名空间的 `en` 表共 116 行改成复数语法，含多词形式——`{n:sample point|sample points}`、`{n:built-in cleanup rule|built-in cleanup rules}`、`{n:hardware snapshot|hardware snapshots}`、`{n:cleanup scope|cleanup scopes}`、`{n:MCP tool|MCP tools}`、`{n:duplicate entry|duplicate entries}`、`{n:startup item|startup items}`、`{n:Workshop item|Workshop items}`、`{n:more category|more categories}`；中文表一字未动（中文不分单复数）。
- **core.ts**：`t()` 的复数分支改为——参数没传仍保留 `{n:x|xs}` 原样（开发期一眼看见漏传）；参数传了但 `Number()` 出来不是有限数（如 `"1,234"`）则按**复数**渲染成 `1,234 files`，绝不再把原始语法漏到界面。
- **守卫**：`coverage.test.ts` 新增一条——英文表里「占位符 + 数量名词（短语最多 3 个词）」不许裸写，命中即指出键名与建议写法；`core.test.ts` 把「非数字参数保留占位符」的旧断言改成新口径（`many files` / `1,234 files`），并保留「缺参保留原样」一例。
- **反向自检**：把 `cleanup.done` 还原成 `{n} files` → 守卫 1 红点名该键；把 `core.ts` 改回非数字返回原串 → core 用例 1 红（`expected {n:file|files} to be many files`）；把多词的 `shell.footer.rules` 还原 → 守卫同样点名（短语规则没有只认单个词）。
- **验证**：`pnpm check` 88 files / 1277 tests 全绿。

### R470: 两处「有类名没样式」补齐——空态副标题与风险提示小字不再继承正文字号

- **缺口**：`App.tsx` 的空态副标题写着 `className="empty-sub"`、提醒设置里未知脚手架的提示写着 `className="muted xsmall"`，但 `styles/*.css` 里**根本没有这两个类**。于是空态副标题与 15px 的标题同处一段却按正文 Default 字号渲染、拉不开层级；「未知脚手架」那枚警示小字继承正文字号，比紧挨着的 11px 字节数**还大**——提示反而比正常信息更抢眼。
- **改动**：`styles/layout.css`（紧挨既有 `.empty-title` 与 `.muted.small`）补两条声明——`.empty-sub { font-size: 12.5px; line-height: 1.65; color: var(--ink-2); }`、`.xsmall { font-size: 10.5px; }`（`small` 是 11px，`xsmall` 正好再小一号）。全部走既有语义 token，不引入裸 hex / 裸字号以外的颜色。
- **守卫**：新增 `apps/desktop/src/classParity.test.ts`（5 条）——扫全仓 TSX 里字面写出的 `className` 类名，每个都必须在 `styles/*.css` 里有定义，或在 `ALLOW_UNSTYLED` 里登记为「有意不写样式」的结构钩子（`.ao-card` / `.ghost.small` 这类基础类承担视觉的包装层）。另附三条：解析器自己不能空转（CSS ≥13 个文件、TSX ≥40 个文件、类名池 ≥800 / ≥500）、登记表不许过期（登记了就必须还有人在用且确实没样式）、注释里的类名不算定义。
- **反向自检**：把 `.xsmall` 规则改名 + 把 `App.tsx` 的 `className="empty-sub"` 改成 `empty-subx` → 恰好 2 条红并逐条点名 `empty-subx @ ./App.tsx:949`、`xsmall @ ./components/ReminderSettings.tsx:625`；还原后 5 passed。
- **价值**：以后 `className="mutd"` 这类手滑不再静默丢样式——CI 直接红，并指出是哪个文件哪一行。

### R469: 提示条按钮点完焦点不丢——「去调散热」/关闭后不再掉到页面开头

- **缺口**：提示条的「去调散热 / 查看周报」动作按钮与关闭按钮点完整条就卸载——若焦点正在该条里（键盘 Tab 过去 / 正在读错误原文），卸载后焦点掉到 `<body>`，键盘用户要从文档开头重新 Tab 一遍（R462 清空按钮同款问题，toast 一直没处理）。动作按钮打开设置窗时更糟：焦点先掉 body，窗自己再抢，中间凭空多一次跳动。
- **改动**：`a11y.ts` 新增 `focusMainIfFocusLost()`——`setTimeout(0)` 等 React 真正卸载完按钮后再看 `document.activeElement`，已掉到 body（说明没有别的控件接走焦点）就把焦点交给页面主体（跳过链接落点 `#main-content`，R419 已有可见焦点描边）；`App.tsx` 提示条两个按钮都在 `dismissToast` 后调它。动作若开了窗，窗会自己抢焦，检测到就不会抢，互不打架。
- **守卫**：`a11y.test.ts` 新增 4 条——行为三连（焦点落 body 时交 main / 别的控件已接走时不抢 / 没有 document 或没有 main-content 都静默不抛）+ 接线断言（toast 栈内 `focusMainIfFocusLost();` 恰好 2 处）。
- **反向自检**：删掉关闭按钮那处调用 → 恰好 1 条红（接线断言 expected 1 to be 2），还原后 130 passed；`pnpm check` 87 files / 1270 tests 全绿。

### R468: 活动会话草稿——托盘退出 / 进程被杀时，没归档的对话不再整段丢

- **缺口**：活动会话的 turns 平时只在归档（`archiveActiveChat` / `newChat` / `switchChat`）时才进历史列表落盘；`beforeunload` 只在刷新/真正关窗时触发，而默认「关闭窗口进托盘」只是 `window.hide()`、托盘「退出」直接 `app.exit`，进程被杀更不用说——这些路径上未归档的活跃对话整段消失（连「中断」提示都留不下）。
- **改动**：`store.ts` 新增活动草稿键 `diskpilot.activeChatDraft`（`{ id, turns }`，最多 200 条，与历史会话同口径），订阅 `chat.turns` / `activeChatId` 变化做 500ms 节流写盘（流式回复每几十毫秒变一次，debounce 会永远等不到空闲）；启动恢复改走纯函数 `activeChatRestore(activeId, historyHit, draft)`——草稿 id 与当前活动会话一致且非空时优先用它（比归档副本新，pending 回合顺手净化），否则回退历史命中，再否则空会话。另导出 `flushActiveChatDraft()`，`App.tsx` 的 `beforeunload` 处理器在归档后立刻再写一次，最后 500ms 内敲的那句也不落后。
- **守卫**：`store.test.ts` 新增 6 条——500ms 后落盘含当前 activeChatId、窗口内多次变化合并成一次写（patch 三次只写一条最新文本）、草稿截断到 200 条、`flushActiveChatDraft` 立刻落盘、`activeChatRestore` 四分支（命中优先 / id 不匹配回退历史 / 草稿空回退历史 / 全空返回空）、init 确实调 `activeChatRestore(active, hit, loadActiveChatDraft())`；`a11y.test.ts` 新增 2 条钉住 `beforeunload` 里归档后接 `flushActiveChatDraft();` 与 store 的订阅/节流骨架。
- **反向自检**：删掉 `App.tsx` 的 `flushActiveChatDraft();` → 恰好 1 条红（点名 beforeunload 断言），还原后 126 passed；`pnpm check` 87 files / 1266 tests 全绿。

### R467: toast 倒计时可以暂停——悬停或聚焦时不再「读着读着就消失」

- 缺口：toast 的自动关闭是创建时一次性 `setTimeout`（`apps/desktop/src/store.ts` 的 `toast()` 末尾），R342 按级别分到 5–16 秒，但**倒计时无法暂停**：鼠标悬在提示条上读异常原文、或键盘 Tab 到「去调散热」「查看周报」按钮准备按下去时，条子照样到期消失——带动作按钮的提示等于白给，长错误串也来不及读完（WCAG 2.2.1：限时内容要能暂停/延长）。
- 改动：`store.ts` 增加 toast 句柄表 `toastTimers` 与统一的 `scheduleDismiss(id, ms)`（登记 `dueAt`/`remaining`）；新增 `pauseToast(id)`（`clearTimeout` 后按 `dueAt - now` 记下剩余时长、置 `paused`）与 `resumeToast(id)`（按**剩余**时长重排，不是从头重数）；`dismissToast` 手动/到期关闭都一并 `clearTimeout` 并删登记，不留悬空句柄。`App.tsx` 的提示条挂 `onMouseEnter` / `onMouseLeave` / `onFocus` / `onBlur` 四个钩子（focus/blur 冒泡覆盖条内动作按钮与关闭按钮），指针停在条上或焦点在条里时不倒数。
- 守卫：`store.test.ts` 新增 describe「toast 倒计时可暂停（R467）」4 条——到期自动关闭、暂停后越过到期仍保留且续上后按剩余时长正好走完、重复暂停与未登记 id 静默不抛、手动关闭后 `vi.getTimerCount()` 归零且不再触发；`a11y.test.ts` 新增 describe「toast 倒计时的悬停 / 聚焦暂停（R467）」2 条——App.tsx 四个钩子逐字、store.ts 的 `scheduleDismiss` / 剩余时长计算 / `paused` 置位 / `dismissToast` 清理句柄。
- 反向自检：删掉 `pauseToast` 里的 `entry.paused = true;` → 恰好 1 failed 点名该守卫断言，已还原；pnpm check 87 files / 1258 tests 全绿。

### R466: 扫描完成也播报——完成条补上 live region，读屏用户不必盯着进度条

- 缺口：扫描结束在界面上只是「进度条消失 + 换成完成条」，而完成条（`apps/desktop/src/App.tsx` 的 `.scan-bar-done`）没有 live region——读屏用户扫完之后收不到任何结果播报，只能自己去摸页面；R465 刚给取消条加了 `role="status"`，于是口径反过来变成「取消有播报、正常完成没有」。
- 改动：完成条根节点补 `role="status"`（隐式 `aria-live="polite"`），文本「本次扫描 {files} 个文件 · {bytes}」在出现时播报一次；完成条只在扫描结束那一刻挂载、挂载后文本不再变，不会重复打扰。
- 守卫：a11y.test.ts 的「扫描中断态不再冒充完成（R465）」第 1 条追加断言，钉住完成条开标签必须是 `<div className="scan-bar scan-bar-done" role="status">`。
- 反向自检：删掉完成条的 `role="status"` → 恰好 1 failed 点名该断言，已还原；pnpm check 87 files / 1252 tests 全绿。

### R465: 扫描中断态不再冒充完成——取消 / 失败后进度条不再报「本次扫描 N 个文件」

- 缺口：`apps/desktop/src/App.tsx` 的 `ScanBar` 只要 `!scanning && progress` 就渲染「完成」条（满 100% 底 + `shell.scanbar.done`「本次扫描 {files} 个文件 · {bytes}」）。但取消扫描走 `api.isScanCancelledError(e)` 分支直接 `return`、失败走 `setErr` 分支，两条路都不碰 `progress`——`scanning` 一置 false，进度条就把一次被取消/失败的扫描显示成「完成」，看着像成功扫完了。
- 改动：`ScanBarHandle` 增加 `interrupt: (kind: 'cancelled' | 'failed') => void`；`scan()` 的取消分支与失败分支分别调 `interrupt('cancelled')` / `interrupt('failed')`。`ScanBar` 内新增 `interrupted` 状态（`begin` 时复位），完成条条件收紧为 `interrupted === null`；取消态改渲染中性条 `.scan-bar-cancelled`（`role="status"`，文案 `shell.scanbar.cancelled`「扫描已取消 · 已看 {files} 个文件 · {bytes}」，样式退到 `var(--ink-3)` 且不铺满底色）；失败态不额外渲染（错误横幅已经在报）。i18n 中英成对新增 `shell.scanbar.cancelled`；样式补 `.scan-bar-cancelled .scan-bar-label`。
- 守卫：a11y.test.ts 新增 describe「扫描中断态不再冒充完成（R465）」3 条——接口签名与 `interrupted` 状态声明、完成条门控与取消条条件、两条中断调用各 1 次且 `setInterrupted(null);` 恰 1 处（begin 复位）、i18n 中英成对 + `layout.css` 取消态用 `var(--ink-3)`。
- 反向自检：把完成条条件还原成 `{!scanning && progress && (` → 恰好 1 failed 点名该断言，已还原；pnpm check 87 files / 1252 tests 全绿。

### R464: 社区插件搜索防抖与乱序守卫——打字快时不再用旧结果盖掉新结果

- 缺口：市场页社区 tab 的搜索框每次按键都 `setCQuery`，effect 因此逐键发一次远端索引请求（`pluginRegistrySearch`）：列表在结果之间来回跳、服务端被逐键请求打；更要紧的是没有请求序号——先发的慢响应后到时会无条件 `setCommunity`，把「ab」的搜索结果换成「a」的（输入框写着 ab、列表却是 a 的结果）。失败分支同理，旧请求的 catch 也会把新请求的加载态清成失败。
- 改动：`apps/desktop/src/components/toolbelt/market.tsx` 的 `loadCommunity` 加请求令牌 `communitySeq`（`const seq = ++communitySeq.current;`，then 与 catch 各一道 `if (seq !== communitySeq.current) return;`，只认最后一次请求）；加载 effect 改为 250ms 防抖（`window.setTimeout` + cleanup `clearTimeout`），停手再查，重试按钮等直接调用路径不受防抖影响。
- 守卫：a11y.test.ts 新增 describe「社区搜索的防抖与乱序守卫（R464）」2 条——`loadCommunity` 体内令牌声明 + 两道守卫恰好 2 处；防抖 effect 含 `window.setTimeout(` / `}, 250);` / `return () => window.clearTimeout(id);`。
- 反向自检：把两道 `if (seq !== communitySeq.current) return;` 都删掉 → 恰好 1 failed（guards 0 ≠ 2），已还原。

### R463: 更新下载失败回到待装态——进度钳在 100 以内，错误可见也能立刻重试

- 缺口：设置页在线更新，`installUpdate` 点「下载并安装」后进入 `updatePhase = 'downloading'`。下载失败（网络中断、文件损坏）时 catch 只写 `updateErr` 不复位 phase——进度条永远停在下载态，红字报错旁边却没有「下载并安装」按钮；再点「检查更新」也不会复位 phase，用户只能重启应用才能重试。另外进度 `Math.round((downloaded / total) * 100)` 未钳制：内容长度与实际下载量不一致时 `aria-valuenow` 可能超过 `aria-valuemax=100`，宽度也会溢出。
- 改动：`apps/desktop/src/components/Settings.tsx` 的 `installUpdate` catch 里先 `setUpdatePhase(null)` 再写 `updateErr`——回到待装态，安装按钮恢复、「正在下载」进度条消失、错误红字保留；进度写回改 `Math.min(100, Math.round((downloaded / total) * 100))`，`aria-valuenow` 与宽度都不会超过 100%。
- 守卫：a11y.test.ts 新增 describe「更新下载失败后回到待装态（R463）」2 条（安装流程失败分支存在 `setUpdatePhase(null);` 且先于 `setUpdateErr(`；进度写回必须是钳制式，且不再有裸 `setUpdateProgress(Math.round(`）。
- 反向自检：删掉 catch 里的 `setUpdatePhase(null);` → 恰好 1 failed 点名该断言，已还原。

### R462: 清空按钮点完把焦点还给搜索框——清完还能接着打字

- 缺口：R461 给四个搜索框补的清空按钮是「有词才渲染」的。键盘用户 Tab 到它、按 Enter（或鼠标点）清空后输入变空、按钮随即卸载，焦点掉到 `<body>`——想接着改搜索词得从文档开头重新 Tab 一遍。工具墙与 Steam 检查器里两个更早的清空按钮同样如此（R439 的 Esc 路径没这问题：焦点本来就在输入框里）。
- 改动：`apps/desktop/src/a11y.ts` 新增 `clearSearchOnClick(clear)`——先执行清空，再把焦点交给按钮父容器里的输入框（父容器就是搜索框容器：`label.hp-search` / `div.toolwall-search` / `div.steam-search` 等，邻居就是要的那个 input）。六个站点全部改接该助手：巡检记录、启动项、驱动、文件视图，以及工具墙、Steam 检查器（含各自的 lucide 导入与 a11y 导入更新）。
- 守卫：a11y.test.ts 新增 describe「搜索框清空按钮的焦点归还（R462）」2 条（清空后 focus 被调用一次、父容器拿不到输入框时不炸且清空照常执行）与「清空按钮清完把焦点还给输入框（R462 接线）」2 条（a11y.ts 提供助手与 `parentElement?.querySelector('input')?.focus();`；六个站点各恰好出现 1 次 `clearSearchOnClick(`）。
- 反向自检：把 SteamInspector 的清空按钮还原成内联 onClick → 恰好 1 failed 点名 ./components/SteamInspector.tsx（计数 0 ≠ 1），还原后 a11y.test.ts 115 passed。全量 pnpm check 87 files / 1245 tests 全绿。

### R461: 四个搜索框补清空按钮——鼠标/触屏不必记 Esc

- 缺口：R439 给六个搜索框接上了「按 Esc 一键清空」，但只有工具墙与 Steam 检查器两个搜索框有可见的清空按钮（`ghost icon` + X）。巡检记录、启动项、驱动、文件视图这四个搜索框只有图标在左、输入框在右：鼠标用户想清空只能全选删除，触屏连 Esc 都没有——同一个界面里六个搜索框两种交互。
- 改动：`apps/desktop/src/i18n/namespaces/common.ts` 新增 `common.searchClear`（zh 清空搜索 / en Clear search，作为四个按钮共用的可访问名）；四个组件各在输入框后补一个有词才出现的按钮——`{query !== '' && (<button type="button" className="search-clear" aria-label={t('common.searchClear')} onClick={() => setQuery('')}><X size={12} /></button>)}`（SystemTools 用 `drvQuery`、FileView 走 `onQueryChange('')`），并各自在 lucide 导入里加 `X`。`apps/desktop/src/styles/layout.css` 新增 `button.search-clear` 紧凑样式（padding 2px、透明底、`--ink-3` 字色、`border-radius: 999px`、`flex-shrink: 0`）——不复用 `ghost.icon` 的 8px 内边距，否则会把 28px 左右的搜索行撑高；hover 换 `--ink` + `--paper-3` 底，`:focus-visible` 补 2px `--accent-strong` 描边（沿用 R348/R405/R431 的键盘焦点口径）。
- 守卫：a11y.test.ts 新增 describe「搜索框的清空按钮（R461）」3 条——四个站点各断言含 `common.searchClear`、`<X size={12} />`、条件前缀 `!== '' && (<button`，且文件里 `search-clear` 恰好出现 1 次（防空转）；`layout.css` 含紧凑类与 `:focus-visible` 描边；`common.searchClear` 中英成对。
- 反向自检：删掉 FileView 里那行按钮 → 恰好 1 failed（断言点名 ./components/FileView.tsx），还原后 a11y.test.ts 111 passed。全量 pnpm check 87 files / 1241 tests 全绿。

### R460: Rust 命令面与前端 invoke 的契约守卫——漏挂 handler / 拼错命令名不再等到点下去才炸

- 缺口：三张清单（`#[tauri::command]` 声明 117 条 / lib.rs 的 `generate_handler!` 注册 / 前端 api.ts + api/*.ts 的字面量 invoke）此前没有任何交叉校验。加命令忘了挂进 handler 时宏仍生成包装函数，cargo 编译通过，用户点下去才吃到 command not found；前端拼错命令名同理；Rust 侧改名后没人调则留下死命令。
- 改动：新增 apps/desktop/src/commands.test.ts（4 条）——声明解析会跳过 `#[allow(...)]` / `#[cfg(windows)]` 这类夹在标记与 fn 之间的属性行和注释行；注册清单按 `generate_handler![` 后的 `[` `]` 深度配平取体、条目取最后一段路径（`scan::scan_path` → `scan_path`）；前端用泛型容忍的正则抓字面量命令行（`[^()]` 吃掉嵌套尖括号，如 `invoke<Record<string, SpacePoint[]>>`）。源文件一律用 `import.meta.glob` 的 `?raw` 取文（src 下的测试被 `tsc -b` 纳入类型检查，不能引 node:fs）。
- 断言：解析器不空转（api 文件 ≥6、声明 ≥100、注册数 === 声明数、含 `scan_path`、invoke ≥100）；声明 → 注册无遗漏；invoke 名都在声明表里；声明都被前端用上（有意只给托盘或后台线程的命令写进 `ALLOW_UNUSED` 并注明原因，当前为空）。
- 反向自检：往 api.ts 临时插一行 `invoke("r460_probe_typo")` → 恰好 1 failed 点名该名字；再新建一个只带 `#[tauri::command]` 的临时 .rs → 3 failed（计数不等 + 漏挂 + 未使用）都点出 `r460_probe`，两处已还原。全量 pnpm check 87 files / 1238 tests 全绿。

### R459: toast 同屏上限 4 条——批量失败时不再用提示条糊满整个窗口

- 缺口：R364 只做了「同一句话反复喊」的去重（文本 + 级别 + 动作按钮同键原位替换）。互不相同的提示仍会同时在场：批量安装插件逐项失败、清理清单多条路径各自报错、多盘体检同时超温——`.toast-stack`（overview.css）只有 fixed + flex column，既不能滚也不封顶，攒到十几条就从中下往上糊满窗口，每条 `pointer-events: auto`，下面的按钮点不到。
- 改动：apps/desktop/src/store.ts 新增导出常量 `TOAST_MAX_VISIBLE = 4`，toast() 追加分支在推入后按上限裁掉最早的几条（被丢掉的 id 稍后它的 dismiss 定时器会扑空，无副作用；去重的原地替换分支不动，替换不挤掉别人）。
- 守卫：apps/desktop/src/store.test.ts 新增 describe「toast 同屏上限（R459）」3 条——六条不同提示只留最近四条（顺序即 `c,d,e,f`）；满员后重复触发同一条仍原地替换、四条不变；上限常量就是 4。
- 反向自检：删掉裁剪那行 → 恰好 1 failed（expected length 4 but got 6），还原后 store.test.ts 37 passed。

### R458: 搜索框 Esc 清空在输入法组合期让路——撤销组字不再把整段搜索词清掉

- 缺口：R439 给六个搜索框（巡检记录 / 启动项 / 驱动 / 工具墙 / 文件视图 / Steam 检查器）接上「按 Esc 一键清空」，但中文/日文/韩文输入法组合期（拼音串、候选词）按 Esc 的本意是「取消组字」：原实现把这个按键当作清空指令，组好的整段搜索词被一次清掉，还 preventDefault + stopPropagation 把事件吞了，输入法收不到。
- 改动：apps/desktop/src/a11y.ts 的 clearSearchOnEscape 参数类型补 isComposing / nativeEvent{isComposing,keyCode}，函数体最前放行组合期——与 R404（聊天框 Enter 不发送）同一个三路口径：合成事件自带 isComposing，部分浏览器在组合结束的那一下 keydown 只留 nativeEvent.isComposing 或 keyCode 229，三路都认。六处调用点无需改动。
- 守卫：a11y.test.ts 新增 describe「搜索框 Esc 清空的输入法组合守卫（R458）」4 条——组合中不清空、不吞事件；只留 nativeEvent 两路信号也算组合期；组合期之外仍照常清空（R439 行为不回归）；a11y.ts 三路判定与类型字段静态断言（新增 RAW_A11Y glob 读 .ts 原文）。
- 反向自检：守卫行改成 if (false) return; → 恰好 3 failed 点名 R458 三条，还原后 a11y.test.ts 108 passed。

### R457: toast 长文本折行——Windows 长路径不再糊出卡片外

- 缺口：`apps/desktop/src/styles/overview.css` 的 `.toast-text` 只有 `flex: 1; min-width: 0`，没有任何断行规则。toast 里常带无空格断点的内容（`reveal.ts` 的数据目录失败、`clipboard.ts` 的复制失败原文、`store.ts` 把 `String(e)` 拼进错误文案），Windows 路径一长就整段横向溢出：卡片 `max-width: 84vw` 拦不住，文字糊到页面上、压住下层内容（`.toast` 也没有 `overflow: hidden`，本就直接画出去）。
- 实证：1280×800 页面里 340px 宽的探针卡，同一段 126 字符路径——加规则前 `scrollWidth 500 / clientWidth 266`（溢出 234px、明显越出卡框），加规则后 `266 / 266`（收进卡内、多折一行）。
- 改动：`.toast-text` 补 `overflow-wrap: anywhere;`（`min-width: 0` 保留，flex 收缩逻辑不变；`pointer-events` 链未动，操作/关闭按钮照常可点）。
- 守卫：`apps/desktop/src/styles/token-guard.test.ts` 新增 describe「toast 长文本不溢出卡片（R457）」——切 `.toast-text` 规则块断言 `overflow-wrap: anywhere` + `min-width: 0`，并顺带钉住 `.toast { pointer-events: auto }`（R103 操作按钮可点的契约）。
- 反向自检：删掉 `overflow-wrap` 行 → 恰好 1 failed 点名「expected … to contain overflow-wrap: anywhere」，还原后 5 passed。
- 验证：`npx vitest run src/styles/token-guard.test.ts` 5 passed；`pnpm -C apps/desktop check` EXIT=0；`node scripts/check-changelog.mjs` 与 `check-docs-links.mjs` EXIT=0。

### R456: 确认弹窗小窗不撑破视口——长正文时按钮不再被顶出屏外

- 缺口：`ConfirmDialog`（`.confirm-dialog-mask` + `.cli-confirm` 卡片）既没有 `max-height` 也没有 `overflow`，遮罩只设 `padding-top: 15vh` 且自身不滚动。小窗（150% 缩放的笔记本）或长正文（错误文案、长标题等调用方自定义 `body`）时卡片底部超出视口，确认 / 取消按钮被裁到屏外，用户只能按 Esc 脱身。
- 改动：`apps/desktop/src/styles/chat.css` 的 `.confirm-dialog-mask .cli-confirm` 补 `max-height: calc(100vh - 15vh - 24px)` + `display: flex` + `flex-direction: column` + `overflow: hidden`；新增 `.confirm-dialog-mask .cli-confirm-head, .confirm-dialog-mask .cli-confirm-actions { flex-shrink: 0; }` 与 `.confirm-dialog-mask .cli-confirm-body { overflow-y: auto; min-height: 0; }`——头尾固定、正文自己滚动，按钮永不被顶出屏外。全部作用域限定在遮罩之下，聊天里内联的 `.cli-confirm` 卡片不受影响。
- 守卫：`apps/desktop/src/styles/token-guard.test.ts` 新增 describe「确认弹窗小窗不撑破视口（R456）」——用 `indexOf` 切片 `.confirm-dialog-mask .cli-confirm {` 到首个 `}`，断言含 `max-height: calc(100vh - 15vh - 24px)` 与 `overflow: hidden`，并钉住三个 scoped 规则原文。
- 反向自检：临时删掉 `max-height` 行 → 恰好 1 failed 点名「expected … to contain max-height: calc(100vh - 15vh - 24px)」，还原后 4 passed。
- 验证：`npx vitest run src/styles/token-guard.test.ts` 4 passed；`pnpm -C apps/desktop check` EXIT=0（86 files / 1226 tests）；`node scripts/check-changelog.mjs` 与 `check-docs-links.mjs` EXIT=0。

### R455: 台账刷新闸门带一次补跑——在途时点「查看全部」不再白点

- **缺口**：R441 的刷新闸门是「在途就丢弃」。历史页点「刷新」后立刻点「查看全部」时，上限从 500 放宽到 5000 触发的那次刷新被拦下丢掉，在途请求仍按 500 条落盘——`historyTruncated(entries, limit)` 变成 `500 >= 5000` 为假，截断横幅连同「查看全部」按钮一起消失，用户看到 500 行却以为已经看全，也没有入口再放宽（只能手动再点一次刷新）。
- **改动**：新增 `apps/desktop/src/refreshGate.ts`（`createRefreshGate()`：`tryStart()` 在途时记一次补跑并返回 false；`finish()` 返回 true 表示期间有被拦下的刷新）；`HistoryPage.tsx` 用它替换 `refreshingRef` 布尔闸门，`.finally` 里 `finish()` 为真时通过 `refreshRef.current?.()` 用最新 `limit` 闭包立刻补拉一次，spinner 不断档。
- **守卫**：新增 `apps/desktop/src/refreshGate.test.ts` 6 条——闸门纯逻辑 5 条（空闲可开始 / 在途拦下并记补跑 / 补跑标记用过即清 / 收尾后可重开 / 实例互不影响）+ 接线 1 条（HistoryPage 含 `createRefreshGate` 导入、`tryStart` / `finish` / 补跑调用与 `refreshRef.current = refresh;`，且 `refreshingRef` 已消失）。R441 那条 a11y 断言同步改为新闸门文本。
- **反向自检**：删掉 `refreshRef.current?.();` 一行 → `refreshGate.test.ts` 恰好 1 failed 点名接线断言，还原后 6 passed。
- **验证**：`pnpm -C apps/desktop check` 86 files / 1225 tests；`node scripts/check-changelog.mjs` / `check-docs-links.mjs` 均通过。

### R454: 切会话不再串台——草稿（文字 + 待发附件）跟着会话各存一份

- 缺口：聊天框的输入草稿与待发附件（拖入的路径 / 图片药丸）都是组件级单例状态：在会话 A 打了一半的字、拖了两个附件，随手点开到会话 B，草稿和附件原样跟过来——按发送就发进 B 的上下文（半句话问错对话、附件串台），切回 A 时 A 的草稿已经没了。
- 改动：新增 apps/desktop/src/components/chatDraft.ts（`ChatDraft` + `swapChatDraft(store, from, to, current)`：切走时非空草稿归档到离开的会话名下、空草稿删除条目、切回时取回自己的草稿并逐项浅拷贝，`EMPTY_CHAT_DRAFT` 兜底）；ChatPanel 持 `draftsRef` + `prevChatIdRef`，`activeChatId` 变化时一次性交换文字 / 路径 / 图片三样状态。
- 守卫：apps/desktop/src/components/chatDraft.test.ts 7 条——纯函数 6 条（归档 + 取回、空草稿不落条目、目标会话无草稿则清空、同会话不搬动、仅附件也算非空、返回值与库内互不影响）+ 接线 1 条（import.meta.glob 读 ChatPanel raw，断言两个 ref / swapChatDraft 调用 / 三个 setter 写回）。
- 反向自检：临时删掉 effect 里的 `setInput(swapped.next.text);` → 接线断言恰好 1 failed，还原后 7 passed。
- 验证：pnpm -C apps/desktop check EXIT=0（85 files / 1219 tests）；check-changelog 与 check-docs-links EXIT=0。

### R453: 切换会话后回到该会话的底部——不再继承上一个会话的滚动位置

- 缺口：R406 的贴底跟随只在「回合数变化 / 忙碌态变化」时触发，且要求 `stickRef.current` 为真。用户在上一个会话往上翻过历史，再切到另一个会话：turns 整个换掉、滚动位置却留在原地——长对话→长对话落在中间，短对话→长对话卡在顶部，想看最近消息得先自己滚到底。
- 改动：apps/desktop/src/components/ChatPanel.tsx 新增 `useEffect(..., [activeChatId])`——切换会话时先把 `stickRef.current` 复位为 true（新会话视为「贴底」），再 `scrollTo` 到 `scrollHeight`（沿用 `scrollBehavior()`，系统开了减少动态效果就瞬时到位）。原 R406 的回合增长跟随逻辑不变，只是切会话这一次强制回底。
- 守卫：apps/desktop/src/a11y.test.ts 尾部 describe「切换会话回到底部（R453）」1 条——切 `R453：` 注释到 `}, [activeChatId]);` 之间的 effect 体，断言含 `stickRef.current = true;`、`scrollTo({ top: scrollerRef.current.scrollHeight`、`behavior: scrollBehavior() });`。
- 反向自检：删掉 `stickRef.current = true;` → 恰好 1 failed 点名该断言，还原后 104 passed。
- 验证：pnpm -C apps/desktop check EXIT=0（84 files / 1212 tests）。

### R452: 会话列表的内存上限与写盘对齐——第 21 条会话不再「看得见、重启就消失」

- 缺口：`persistChatSessions` 落盘时把列表截到 20 条，但内存里的 `chatSessions` 不截——`newChat` / `archiveActiveChat` 只做 `[新归档, ...旧列表]`。一次会话里聊出超过 20 段对话时，侧边栏照常显示第 21 条起的历史（可点、可切换），下一次落盘却写不进去，重启后这些会话静默消失：看得见、用过、却没了。
- 改动：apps/desktop/src/store.ts 导出 `MAX_CHAT_SESSIONS = 20` 作单一上限；`persistChatSessions` 与 `loadChatSessions`（旧存值可能超限）都按它截断；`newChat` / `archiveActiveChat` / `switchChat`（含在飞归档分支）构造完列表后同样 `slice(0, MAX_CHAT_SESSIONS)` 再落盘并写回内存——UI 显示的上限与落盘的上限从此是同一个数。
- 守卫：apps/desktop/src/store.test.ts 追加 describe「会话列表上限（R452）」2 条——新建 21 段会话后内存与 localStorage 都恰好 20 条、最新在前、最旧被丢，且两侧 id 序列一致；已有 20 条时归档新对话同样只留 20 条、最旧被挤掉。
- 反向自检：撤掉 `newChat` 的内存截断 → 恰好 1 failed（expected 21 to be 20），还原后 34 passed。
- 验证：pnpm -C apps/desktop check EXIT=0（84 files / 1211 tests）。

### R451: 清空对话后不再「删了又复活」——activeChatId 同步归零

- 缺口：聊天气泡头的 X（清空对话）走 store 的 `resetChat`：它把活动会话从 `chatSessions` 移除、清空 turns、落盘列表，但既不把 `activeChatId` 归 null，也不同步 `persistActiveChatId`。残留的 id 让之后任一处归档（关窗 `archiveActiveChat` / 新建 `newChat` / AI 在飞时切走 `switchChat`）都用旧 id 把刚落盘的会话写回列表——用户刚清空的对话，关个窗又出现在历史里；重启后 localStorage 里还留着指向已删会话的 activeChatId。
- 改动：apps/desktop/src/store.ts 的 `resetChat` 在移除列表项后补 `persistActiveChatId(null)`，并把返回值里的 `activeChatId` 置 null——与 `deleteChat` 删除活动会话时的口径一致；之后的新回合归档会拿到一个全新 id，而不是复写旧会话。
- 守卫：apps/desktop/src/store.test.ts 追加 describe「resetChat（清空对话）不复活已清掉的会话（R451）」2 条——清空后 activeChatId 为 null 且持久化键被移除；清空后产生新回合并关窗归档时，旧 id 不再出现在列表里、新回落在新 id 下。
- 反向自检：撤掉 `persistActiveChatId(null)` 与 `activeChatId: null` → 恰好 2 failed（expected 'chat_a' to be null / 旧会话被复活），还原后 32 passed。
- 验证：pnpm -C apps/desktop check EXIT=0（84 files / 1209 tests）。

### R450: 聊天框混入的非图片文件给出提示——不再无声消失

- 缺口：聊天框只收图片，但「选择文件」对话框能切到「所有文件」、拖入也能带进 PDF / zip——这些文件在 `addImageFile` 里被 `if (!file.type.startsWith('image/')) return;` 静默丢弃：没有药丸、没有提示、没有系统消息，用户以为点了没反应。
- 改动：apps/desktop/src/components/ChatPanel.tsx 新增 `skipNonImages`（最多列 3 个文件名的系统消息，超过补省略号）；`addImageFile` 遇非图片改为调用它而不是裸 return；`onDrop` 把 `dataTransfer.files` 分成图片 / 非图片两批，图片照收、其余一并说明；文件选择器仍把每个文件交给 `addImageFile` 统一判定。
- i18n：chat.ts zh / en 各加 `chat.sys.imageNotImage`（只支持图片（PNG / JPG / WebP / GIF），已忽略非图片：{names} / Only images are supported (PNG / JPG / WebP / GIF); ignored: {names}）。
- 守卫：新增 apps/desktop/src/components/chatDrop.test.ts 3 条（统一提示且静默 return 消失 / 拖入按图片分流 / 名称最多三条与选择器通路）；反向自检还原静默 return → 恰好 1 failed，还原后 12 passed。
- 验证：pnpm -C apps/desktop check EXIT=0（84 files / 1207 tests）。

### R449: 英文表不再悄悄留中文——未翻译的键逐项钉进允许清单

- 缺口：键集一致的守卫（coverage.test.ts）只保证 zh / en「都有这条键」，不保证英文真的翻译过——新键若把中文原样抄进 en，英文界面会直接显示中文，测试仍然全绿。
- 改动：apps/desktop/src/i18n/coverage.test.ts 新增「英文表不再留下未翻译的中文」用例——en 值含 CJK 表意区 / 标点 / 全角字符的键必须落在 `EN_CJK_ALLOW`（14 项：多盘合并虚拟根 path、mock 冒充后端说明、喂给 AI 的会话上下文模板、提示用户照打的确认词、中文产品名「图吧工具箱」的目录与提示、作者名等专名）；允许清单自身也逐项核验（必须恰好存在于某个命名空间、且确实还含汉字），条目不再需要就删除，不许当免罪金牌。
- 反向自检：把 `cleanup.skipAll` 的 en 临时改成「全部跳过」→ 恰好 1 failed 点名该键，还原后 9 passed。
- 验证：pnpm -C apps/desktop check EXIT=0（83 files / 1204 tests）。

### R448: AI 清单回收结果就地反馈——一项都没成时不再无声关窗

- 缺口：清理提案窗的执行按钮走 store 的 `aiRecyclePaths`，返回值 `{ done, failed, bytes }` 一直被丢掉——不管成没成、甚至「执行权限未开」，`doRecycle` 都是 `setBusy(false); close();`。失败细节只存在于 store 弹出的 toast，窗口一关就没了，用户回到总览也查不到「刚才那批到底动了没有」；而且 `doRecycle` 没有 try/finally，将来任何一次 reject 都会把执行按钮永久锁在「回收中…」。
- 改动：apps/desktop/src/cleanup/model.ts 新增 `recycleOutcome(res)`（纯函数）——有成功项就关窗（部分失败仍由 store 的 toast 报告），一项都没成则不关窗并把失败条数带出来；apps/desktop/src/components/CleanupProposalDialog.tsx 的 `doRecycle` 改为 `try { const res = await …; const outcome = recycleOutcome(res); if (outcome.close) close(); else setRecycleErr(t('cleanup.recycleAllFailed', { n: outcome.failed })); } finally { setBusy(false); }`，窗内新增 `role="alert"` 错误位（`proposal` 变化时清空），apps/desktop/src/styles/overview.css 补 `.proposal-err` 间距。新增 i18n 键 `cleanup.recycleAllFailed`（zh/en 成对）。
- 测试：新增 apps/desktop/src/cleanup/proposalRecycle.test.ts 7 条——纯函数四组（有成功项关窗 / 全失败不关窗并带条数 / 空批次与 null、undefined 不钉住窗口 / 负数小数 NaN Infinity 归一）；接线三条（`outcome` 判定、`setBusy(false)` 落在 `finally` 里、窗内 `role="alert"` 错误位与 proposal 变化清空）。
- 反向自检：把 `if (outcome.close) close();` 换成 `close();` → 接线用例恰好 1 failed 点名该断言，还原后 7 passed。
- 验证：pnpm -C apps/desktop check EXIT=0（83 files / 1203 tests）。

### R447: 启动项确认条用过即收——成功后不再反着问「确认恢复？」

- 缺口：启动项面板的开关是两步确认（点开关 → 出现「确认禁用 / 确认恢复」条）。确认后 `toggleStartup` 只重载列表、从不收起 `pendingToggle`：成功后该行 `enabled` 已翻转，确认条却还在，文案变成**反着问**的「确认恢复？」——看起来像刚才那一击没生效；再点一次就把它拨回去了。删除同理（行虽消失，但确认条状态残留）。在途时确认按钮也不禁用，连点会就同一项发两次相反请求。
- 改动：apps/desktop/src/components/StartupPanel.tsx 新增 `mutatingRef`（防重入锁）+ `mutatingId`（在途行 id）：`toggleStartup` / `removeStartup` 入口加 `if (mutatingRef.current) return;`，最后 `finally` 里 `setPendingToggle(null)` / `setPendingDel(null)` 收起确认条；四个确认 / 取消按钮改为在途时 `disabled={mutatingId !== null}`（任何一行在途时全禁用，避免「点了没反应」的静默拦截）。
- 测试：新增 apps/desktop/src/startupToggle.test.ts 2 条（源码切片断言）——两个操作都有锁、结束后各自收起确认条（`toggleStartup` 切片的 `setPendingToggle(null)`、`removeStartup` 切片的 `setPendingDel(null)`）；四个按钮都在途禁用。
- 反向自检：删掉 toggle 失败/成功路径的 `setPendingToggle(null)` → 恰好 1 failed，还原后 2 passed。

### R446: 提醒设置的写请求串行化——连点分段控件不再「最后点的被前一次覆盖」

- 缺口：提醒设置的间隔 / 阈值 / 若干分段控件每点一下就把一个 patch 发给后端，但 save() 直接并发：快速连点「6 小时」再点「3 天」时两个请求同时在飞，后端按到达顺序处理，最终落盘的可能是**先点的那次**；界面这边也会被先发的旧响应 `setCfg` 回写，高亮跳回旧选项。`saving` 指示同样按「任一次完成」清除，在途请求还在时指示已消失。
- 改动：新增 apps/desktop/src/saveQueue.ts —— `createSaveQueue()` 把写请求排成一条链，上一个完成（成功或失败）才发下一个，前一个失败不阻断后续（`tail.then(task, task)`）。apps/desktop/src/components/ReminderSettings.tsx 的 save() 改为经 `saveQueue.current(() => api.setReminderConfig(patch))` 串行发送；`pendingSaves` 计数保证队列里还有在途任务时 `saving` 指示不闪（计数归零才收起）。用户最后一次点击必然最后落盘、界面也不会被旧响应回写。
- 测试：新增 apps/desktop/src/saveQueue.test.ts 5 条——按提交顺序执行（后发的不会插到先发前）、前一个失败不卡住队列、任务不并行（峰值并发 1）、每个队列实例互不影响，以及接线断言（ReminderSettings 必须走队列 + 计数、旧的直接 `await api.setReminderConfig(patch)` 写法必须消失）。
- 反向自检：把组件里的队列调用还原成直接 await → 恰好 1 failed 点名接线断言，还原后 5 passed。

### R445: 「当前项」补上语义——会话列表 / 市场分页读屏能听出谁选中

- 缺口：全仓「选中 / 当前」状态大部分已带 aria（aria-pressed / aria-selected / aria-current 都有守卫），但两处漏网：① 会话列表的活动会话只在容器 div 的 className 加 active（纯视觉），切换按钮本身没有记忆点，读屏听不出当前会话；② 市场页的「内置 / 社区」两个分页按钮用 `className={tab === ... ? 'active' : ''}`（active 不带前导空格），旧守卫只认 `' active'` 写法直接绕过，也没有任何 aria 状态。
- 改动：apps/desktop/src/components/ChatPanel.tsx 的会话切换按钮补 `aria-current={sess.id === activeChatId ? 'true' : undefined}`（当前会话读作「第 N 项，当前」）；apps/desktop/src/components/toolbelt/market.tsx 两个分页按钮补 `aria-pressed={tab === ...}`（与仓内 view 切换按钮同一口径）。
- 守卫：apps/desktop/src/a11y.test.ts 新增 describe「当前项的语义（R445）」3 条——① 通用扫描器改为认 className 里的 `active` 字面量（带不带前导空格都算），凡 <button> 含它必须带 aria-pressed / aria-selected / aria-current / aria-checked 之一，否则按 path:行号 点名（这是旧守卫的漏洞：无空格写法直接绕过）；②③ 钉住会话按钮的 aria-current 与市场两按钮的 aria-pressed。
- 反向自检：删掉 market 社区按钮的 aria-pressed → 恰好 2 failed（扫描器点名 ./components/toolbelt/market.tsx:464 + 站点断言），还原后 103 passed。

### R444: 检查结果接上活区播报——读屏按下「立即检查」能听到结果

- 缺口：提醒设置的「立即检查」「启动项对比」「磁盘预测」三个结果区块都是普通 div，点击按钮后焦点不会移动，读屏用户听不到任何结果播报——只能盲目再按一次或切到别处碰运气。
- 改动：apps/desktop/src/components/ReminderSettings.tsx 三处 `<div className="reminder-result">` 补 role="status"（隐式 polite 活区 + 隐式 aria-atomic）：结果一出现，读屏立即播报「上次检查 共 2.3 GB / 新增 2 项 · 消失 1 项 / 预计 7 天放量」等。视觉不变。错误分支保持 role="alert" 原样（错误要打断、结果不打断）。
- 守卫：apps/desktop/src/a11y.test.ts 新增 describe「检查结果的活区播报（R444）」2 条——① 用花括号配平取完整开标签，凡 className 含 reminder-result 的 <div> 必须同时带 role="status"，否则按 path:行号 点名；② 钉住 ReminderSettings 恰好 3 处已接入。
- 反向自检：删掉 startup 区块的 role="status" → 恰好 2 failed（扫描器点名 ./components/ReminderSettings.tsx:483 + 计数 2≠3），还原后 100 passed。

### R443: 开关按钮补 role="switch"——读屏才念得出「开 / 关」

- 缺口：全仓只有权限中心与启动项面板两个开关是自绘的 <button className="switch">，状态挂在 aria-checked 上；但 aria-checked 在 role=button 上不合法，NVDA / JAWS 不会念出开 / 关，用户只能靠视觉判断，键盘 / 读屏用户无法确认当前权限或启动项是否启用。权限中心的 L0 常开开关（禁用态）连 aria-checked 都没有。
- 改动：apps/desktop/src/components/PermissionCenter.tsx 的 L1/L2 开关补 role="switch"；L0 常开禁用开关补 role="switch" + aria-checked={true}（读屏读作「开关，开，不可用」）。apps/desktop/src/components/StartupPanel.tsx 的启动项开关补 role="switch"。视觉样式不变（样式由 className="switch" 承载，与角色无关）。
- 守卫：apps/desktop/src/a11y.test.ts 新增 describe「开关按钮的 role="switch"（R443）」2 条——① 用花括号配平取完整开标签，凡非 <input 的标签带 aria-checked 必须同时带 role="switch"，否则按 path:行号 点名（原生 checkbox 自带 role=checkbox 豁免）；② 钉住权限中心两处、启动项一处已补齐。
- 反向自检：删掉 StartupPanel 的 role="switch" → 恰好 2 failed（扫描器点名 ./components/StartupPanel.tsx:281 + 站点断言），还原后 98 passed。

### R442: AI 在飞时切换会话不再丢对话——在飞回合先净化归档再切走

- 缺口：store 的 switchChat 把当前 chat.turns 整个丢弃（不归档），且强制 busy:false；AI 流式在飞时点会话列表切走，在飞回合既不入库也没有落盘点，回复到达时 patchChatTurn 在新会话里找不到 turnId 静默丢弃，整段对话蒸发；切走再点自己还会拿旧存档覆盖实时 turns，对话「倒退」到上次存档。
- 改动：apps/desktop/src/store.ts switchChat ① 目标 id 等于当前 activeChatId 时直接空操作（实时 turns 不再被旧存档覆盖）；② s.chat.busy 且有 turns 时，按 newChat/archiveActiveChat 口径先把在飞 turns 净化归档回 chatSessions（pending → needsResume / 中断文案，保留 overviewPath 供续跑），再切换目标会话。回复到达时 patchTurn 在新会话 no-op 无害，回切旧会话仍可见「待续跑/中断」。
- 测试：apps/desktop/src/store.test.ts 新增 describe「switchChat（切会话）在 AI 在飞时的归档（R442）」3 条——在飞切走归档净化（pending→needsResume、保留 overviewPath）、非在飞切换不写归档、点自己空操作不覆盖实时 turns。
- 反向自检：把在飞归档分支换成 if (false) → 恰好 1 failed 点名「AI 生成中切走」，还原后 30 passed。

### R441: 台账两处刷新按钮给出进行中反馈——连点不再并发重复请求

- 缺口：巡检记录页的「刷新」按钮与 Studio 侧栏「最近清理」的「重试」按钮都没有进行中状态：点下去毫无动静（后端慢时更明显），按钮也不禁用，连点会并发发出多个 listUndo 请求；还原动作触发的刷新若撞上在途加载，同样会叠一次请求。
- 改动：HistoryPage.tsx 与 UndoPanel.tsx 各加 refreshing 状态与 refreshingRef 重入闸门——refresh 开头遇到在途加载直接返回，请求链 finally 里复位；刷新 / 重试按钮 disabled={refreshing}，aria-busy 随状态暴露给读屏，图标在 RotateCcw 与转圈的 Loader2 之间切换，点下去当即有回应。
- 守卫：a11y.test.ts 新增 describe「台账刷新按钮的进行中反馈（R441）」2 条——按 const refresh = useCallback( 到回调结尾切片，断言重入闸门、setRefreshing 置位与 finally 复位都在回调体内；按钮侧断言 disabled / aria-busy / 图标三目，并钉住旧的恒 RotateCcw 写法已消失。
- 反向自检：临时删掉 UndoPanel 的重入闸门行 → 恰好 1 failed 点名该断言，还原后 96 passed（全量 80 files / 1179 tests）。

### R440: 截断的名字补悬停提示——磁盘型号 / 主板型号 / 清理规则名被省略号切掉后悬停能看到全文

- 缺口：全仓 46 条 CSS 规则用 text-overflow: ellipsis 把长文本切成省略号，其中四处是动态名字却没有 title：硬件面板的磁盘型号名（hw-name）与主板型号、Studio 的 winapp2 清理规则名（winapp2-item-name）、周报的磁盘名（wh-disks-name）。省略号切掉的正是「这块盘是什么型号」「这条规则清理什么」，鼠标悬停也拿不回全文。其余 40 余处要么自带 title、要么父级容器已带 title（如文件视图行、预览清单行、规则卡），已逐一核验不是缺口。
- 改动：HwPanels.tsx 把磁盘显示名提成 hwName 常量，<span> 文本与 title 同源（避免两处表达式漂移），主板型号 span 补 title；Studio.tsx 的 winapp2-item-name、WeeklyHealthCard.tsx 的 wh-disks-name 各补 title，与显示文本同源。
- 守卫：a11y.test.ts 新增 describe「截断文本的悬停提示（R440）」2 条——HwPanels 里 <span className=hw-name title= 必须恰好 3 处（内存/磁盘/主板）且磁盘分支用 hwName 同源变量；Studio 规则名与周报磁盘名的 title 片段逐字断言。
- 反向自检：临时删掉 Studio 的 title → 恰好 1 failed 点名该断言，还原后 94 passed。

### R439: 搜索框 Esc 一键清空——肌肉记忆不再落空，弹窗 Esc 不再误伤

- 缺口：六个搜索框（巡检记录 / 启动项 / 驱动表 / 工具墙 / 文件视图 / Steam 搜索）都没有「按 Esc 清空」的行为；toolwall 与 Steam 搜索有视觉清除按钮，其余四个连清除按钮都没有。用户停在搜索框里按 Esc 是肌肉记忆，当前只会冒泡——要么没效果，要么把桌面上的弹窗一并关掉（R349 弹窗栈）。
- 改动：apps/desktop/src/a11y.ts 新增 clearSearchOnEscape(clear) 助手——仅在输入框有文字时按 Esc 才 preventDefault + stopPropagation + 清空；空输入时放行，Esc 继续走弹窗栈。六个搜索框各接一行 onKeyDown={clearSearchOnEscape(() => setX(''))}，让清空动作与局部状态解耦（FileView 走 onQueryChange 回调）。
- 守卫：apps/desktop/src/a11y.test.ts 新增 describe「搜索框 Esc 清空（R439）」4 条——有文字时 Esc 清空并吞事件（不冒泡去关弹窗）/ 空输入放行不调 clear 不调 stopPropagation / 其它键不动 / 六个搜索框全部接上（逐路径断言）。
- 反向自检：临时删掉 HistoryPage 的 onKeyDown 行 → 恰好 1 failed 点名「六个搜索框都接上」，还原后 92 passed。

### R438: 唯一表格补 <caption> 标题——读屏进参数表先听到「脚本参数表」

- 缺口：全仓唯一的表格（工具墙详情卡的参数表）没有任何标题；读屏进入时只会报「表格，4 列 × N 行」而没有内容语境。列头虽已由 R432 补上 scope="col"，整张表仍缺一个名字。全仓也没有 .sr-only 工具类，想补标题没有现成的视觉隐藏手段。
- 改动：apps/desktop/src/styles/tokens.css 新增 .sr-only 屏幕阅读器专用类（视觉隐藏但读屏可读：absolute + 1px + clip + nowrap）；apps/desktop/src/components/toolbelt/toolwall.tsx 参数表在 <thead> 前补 <caption className="sr-only">{t('toolbelt.detail.paramsCaption')}</caption>；i18n 新增键 toolbelt.detail.paramsCaption（zh 脚本参数表 / en Script parameters），与表头键同区放置。
- 守卫：apps/desktop/src/a11y.test.ts 新增 describe「表格的标题（R438）」2 条——caption 必须位于 <table className="tdm-params"> 之后、<thead> 之前且引用新键；.sr-only 块必须含 position absolute / clip / nowrap / border 0。反向自检：临时删除 caption 行 → 恰好 1 failed 点名该断言，还原后 88 passed。

### R437: AI 配置保存前归一化空白——粘贴带尾随空格不再「连接失败」查不出原因

- 缺口：Settings 的保存与试连只用 trim 做非空校验，落盘存的仍是原样（含首尾空白、多余尾斜杠）；而 provider.ts 消费端只 replace(/\/$/, '') 剥一个斜杠、不剥空白。从文档或链接里粘贴 ' http://127.0.0.1:11434/ ' 这种值校验能过，实际请求却是带空格的非法 URL，报「连接失败」根本查不出是空格；model 带空格则 404 找不到模型。
- 改动：apps/desktop/src/advisor/provider.ts 新增 export function normalizeSettings(s)——model/apiKey 去首尾空白、baseUrl 去首尾空白并剥掉全部尾斜杠；saveSettings 落盘前过它（单点收口，所有调用方都拿到 canonical 值）；loadSettings 返回前也过它，旧存值在下次读取时自动痊愈（与方法内 provider 重判同样的痊愈口径）。apps/desktop/src/components/Settings.tsx 的 save() 与 testConn() 先归一化再 saveSettings/setBaseline，并把 n.baseUrl/n.model/n.apiKey 写回输入框——输入框立即变成 canonical，所见即所存，也避免 dirty 判断与基线错位。
- 守卫：apps/desktop/src/advisor/provider.test.ts 新增 describe（4 条）——首尾空白与多余尾斜杠收口、canonical 值原样保留、saveSettings 落盘的是归一化值、loadSettings 读回旧存值时自动痊愈。apps/desktop/src/a11y.test.ts 新增 describe「AI 配置保存的空白归一化（R437）」2 条——Settings 的保存与试连两处必须是 const n = normalizeSettings( 且 setBaseUrl/n.model/n.apiKey 各写回 2 次、不再出现裸 saveSettings({ provider, model, apiKey, baseUrl；provider.ts 必须含 normalizeSettings 定义与 return normalizeSettings(parsed) / JSON.stringify(normalizeSettings(s))。
- 反向自检：把两处写回临时改回 raw（setBaseUrl(baseUrl)）→ R437 静态守卫恰好 1 failed，还原后 86 passed。
- 验证：pnpm check EXIT=0，80 files / 1169 tests（1163+6）；check-changelog / check-docs-links EXIT=0。

### R436: localStorage 键读写对偶守卫——「只读不写」的死开关当场红

- 缺口：apps/desktop/src/components/Studio.tsx 读 localStorage 键 diskpilot.hideStudio === '1' 决定整页隐藏，但全仓（含测试）没有任何地方写过这个键——该分支自公开仓库首发（57ae3bb）起就不可达，studio.hidden（已隐藏/Hidden）两条 i18n 键连同整段早退渲染是纯死代码；「键只读不写」这类缺陷此前零守卫，未来新增这样的开关会静默失效。
- 改动：删掉 Studio.tsx 的 hidden IIFE、allCards 里的 if (hidden) return [] 与整段 hidden 早退块（46-64 行），allCards 直接 collectScaffoldCards(root, scaffolds)（deps [scaffolds, root]）；删 studio.ts zh/en 各一条 studio.hidden 键；新增 apps/desktop/src/storageParity.test.ts（38 行 / 2 条）——扫描全部非测试 ts/tsx 的 localStorage 字面键，凡 getItem 的键全仓必须有同键 setItem 或 removeItem（ALLOW_GET_ONLY 例外清单暂空），并断言 Studio.tsx 不再含 hideStudio / studio.hidden。
- 反向自检：临时把 getItem('diskpilot.hideStudio') 探针行插回 Studio.tsx → 恰好 2 failed（对偶守卫点名该键 + Studio 断言），还原后 2 passed。
- 验证：pnpm check EXIT=0，80 files / 1163 tests（1161+2）；check-changelog / check-docs-links EXIT=0。

### R435: 性能档位判定的文档与实现对齐——「任一命中即降档」不再是注释与代码打架

- 缺口：apps/desktop/src/hwProfile.ts 的档位注释写「弱(≤4核 且 ≤8GB) / 中(≤8核 且 ≤16GB)」，但 detect() 实现是「核数或内存**任一**命中即降档」（4 核配 64 GB 内存仍是弱机）——注释撒谎会误导后续维护者把 OR 改成 AND，弱机会被误判成强机、AI 建议直接崩。该模块（106 行，决定 Ollama 本地/云端建议）此前零测试。
- 改动：hwProfile.ts 档位注释改为「弱(≤4 核 或 ≤8 GB) / 中(≤8 核 或 ≤16 GB) / 强(其余)——核数或内存任一命中即降档」，与实现一致；新增 apps/desktop/src/hwProfile.test.ts（137 行 / 12 条）：tierLabel 中英映射、recommendLocal 三档建议、后端探针四组边界（16核/32G→strong、4核/64G→weak、16核/6G→weak、6核/14G→medium）、探针失败回退浏览器近似值（fromBackend=false）、非 Tauri 用 hardwareConcurrency/deviceMemory、全空→strong 且 label 用 ?、无 force 复用缓存只探针一次、perfContextLine 按档位给中文 prompt（@i18n-keep 不译）。
- 反向自检：把 hwProfile.ts 的 cores<=4 放宽到 cores<=3 → 恰好 2 failed（后端判定 + 浏览器近似值两条弱档用例），还原后 12 passed。
- 验证：pnpm check EXIT=0，79 files / 1161 tests（1149+12）；check-changelog / check-docs-links EXIT=0。

### R434: 脚本命中收集纯函数补单测——DFS 深度上限与 R228 嵌套归属从此有据可查

- 缺口：apps/desktop/src/cleanup/matches.ts（91 行）被 Studio 工作台卡片与 CleanupPage 脚本列表共用，含真实规则（回退名字命中的大小写/深度上限、R228 嵌套 detect 各算各的、命中优先+体量降序排序），但零测试——改动 DFS 或排序口径没有任何网兜住。
- 新增 apps/desktop/src/cleanup/matches.test.ts（125 行 / 14 条）：fallbackByNameContains 6 条（空 name_contains 不查、大小写不敏感子串、命中即不再下钻、root 第 0 层不参与、非目录跳过、多关键字）；collectScaffoldCards 8 条（root null 全量按名排、重复 id 只留首张、detect 命中含 root 自身、R228 嵌套各算各的、detect 无命中才走回退、detect 命中不叠加回退、合计体量/文件数求和且 size 降序、最终排序命中优先→体量→名字）。
- 反向自检：把 matches.ts 的 depth 上限从 4 放宽到 5 → 恰好 1 failed（点名「下钻深度上限」用例），还原后 14 passed。
- 验证：pnpm check EXIT=0，78 files / 1149 tests（1135+14）；check-changelog / check-docs-links EXIT=0。

### R433: 图标搜索框补可访问名——三个只靠 placeholder 的搜索框读屏能报出名字

- 缺口：HistoryPage / StartupPanel / SystemTools 三个搜索框包在 label 里但 label 只有 Search 图标、input 只写 placeholder——图标不是可访问名，placeholder 也不是可靠的可访问名来源（FileView / toolwall / SteamInspector 都有显式 aria-label，这三处漏了）。
- 改动：三处 input 各加 aria-label，复用各自命名空间新键 history.searchAria / system.startup.searchAria / system.driver.searchAria（zh/en 成对，键集一致性由既有 i18n 守卫兜底）。
- 守卫（apps/desktop/src/a11y.test.ts）：新增 describe「搜索框的可访问名（R433）」2 条——① 全仓扫 placeholder 输入框（花括号配平取完整开标签，避开 onChange 箭头里的 >），必须有 aria-label / aria-labelledby，或所在 label 带真实文字（t() 或文字节点），否则点名；② 三个图标搜索框都有显式 aria-label 断言。
- 反向自检：临时删 HistoryPage 的 aria-label → 恰好 2 failed（扫描器点名 ./components/history/HistoryPage.tsx:174 + 断言），还原后 a11y 84 passed；全量 pnpm check 77 files / 1135 tests 绿。

### R432: 参数表表头补 scope="col"——读屏把列头与数据格绑起来

- 缺口：全仓唯一数据表（toolwall 的 CLI 参数表）四个 <th> 都没有 scope——读屏把列头当游离文本、不关联对应列的数据格（只有这一张表，所以一直没暴露）。
- 改动（apps/desktop/src/components/toolbelt/toolwall.tsx）：tdm-params 表头行四个 th 加 scope="col"，并在 <table> 前补 R432 注释。
- 守卫（apps/desktop/src/a11y.test.ts）：新增 describe「表格表头的 scope 绑定（R432）」2 条——① 全仓扫任何 <th> 必须带 scope= 属性（正则加 <th(?=[\s>]) 边界，避免把 <thead> 当表头），新增表格忘写直接红；② toolwall 参数表恰好 4 个 scope="col"。
- 反向自检：临时删掉一个 scope → 恰好 2 failed（扫描器点名 + 计数断言 3≠4），还原后 a11y 82 passed；全量 pnpm check 77 files / 1133 tests 绿。

### R431: 搜索框键盘焦点补描边——Tab 进三个搜索框不再只剩边框换色

- 缺口：layout.css:226 有全局 input/select:focus-visible 描边（R348）+ textarea:focus-visible（R405），但更晚加载的样式文件用等特异性的 outline: none 把它覆盖掉——.hp-search input（history.css:96）、.fileview-search input（settings.css:466）、.toolwall-search input（dark.css:364）同为 0,1,1 且排在 layout.css 之后，键盘焦点只剩容器边框换色（R348 判过这种提示不够）；.cp-search 仅 0,1,0、全局 ring 本已压过它，为显式一致也补同款。
- 改动（4 个样式文件）：history.css / settings.css / dark.css / cleanup.css 各加一条 :focus-visible 规则——outline: 2px solid var(--accent-strong) + outline-offset: -2px，鼠标点击仍是边框换色、Tab 到的亮出描边（R348/R405 口径），并配 R431 注释。
- 守卫（apps/desktop/src/styles/token-guard.test.ts）：新增 describe「键盘焦点描边（R431）」——扫描每个样式文件，凡选择器点了 input/textarea 的规则块里写了 outline: none，同文件必须另有 :focus-visible 规则且含 2px accent-strong 描边，否则点名。
- 反向自检：临时删掉 history.css 的 .hp-search input:focus-visible → 恰好 1 failed 点名 ./history.css：.hp-search input，还原后 token-guard 3 passed；全量 pnpm check 77 files / 1131 tests 绿。

### R430: 背景滚动锁可逐点关掉——右键菜单不再把滚轮锁死

- 缺口：R429 把「打开期间锁住背景滚动」直接接进了 useReturnFocus / useDialogFocus，等于所有调用点都锁——但右键菜单（ContextMenu）是瞬态浮层不是模态：它本来靠在 window 上捕获 scroll 来自动关闭（滚轮一动菜单就收），锁住祖先 overflow 后滚动事件根本不发生，滚轮落在菜单上变成完全没反应，只能 Esc 或点空白处才能脱身。
- 改动（apps/desktop/src/dialogFocus.ts）：新增 export type DialogFocusOptions = { lockScroll?: boolean }，useReturnFocus / useDialogFocus 加第二参数 options 并在 effect 外取 const lockScroll = options.lockScroll !== false（默认开，模态弹窗口径不变），解锁改成 `lockScroll ? lockAncestorScroll(...) : () => {}`，依赖数组同步为 [open, lockScroll]。
- 改动（apps/desktop/src/components/ContextMenu.tsx）：调用点改为 useReturnFocus(state !== null, { lockScroll: false }) 并加 R430 注释——菜单不是模态，滚动照旧可滚、滚动关闭菜单的口径保持。
- 守卫：a11y.test.ts 新增 describe「弹窗滚动的可配置锁（R430）」2 条——dialogFocus.ts 断言类型声明 / 默认开判定 / 两处 lockScroll ? … : () => {} / [open, lockScroll]；ContextMenu.tsx 断言显式传 { lockScroll: false } 且 window scroll capture 监听还在。R425 那条「保留 useReturnFocus」的断言同步改成新调用文本。
- 反向自检：把 ContextMenu 的 opt-out 去掉 → 恰好 2 failed（R425 断言 + R430 守卫），还原后 98 passed。

### R429: 弹窗打开时锁住背景滚动——滚轮不再穿透遮罩把背景页滚走

- 缺口：弹窗遮罩（.modal-bg 等）是 position:fixed 全屏，但它挂在各 view 的滚动容器里；滚轮落在遮罩上时浏览器去找「最近的可滚动祖先」——弹窗内容短时背景页跟着滚，正在看的内容被滚走（清理页 / 优化器 / 历史页的滚动容器还不一定是 main，各 view 各一套，没法用一条 CSS 锁死）。
- 改动（apps/desktop/src/dialogFocus.ts）：新增 lockAncestorScroll——从弹窗自身的父链向上找可滚动祖先（computed overflowY auto/scroll/overlay）逐个 overflow:hidden，只改祖先不碰弹窗子树所以 modal-body 照常滚；按元素引用计数（Map 计数 + data-scroll-locked 标记），嵌套弹窗全部关掉才恢复原值；useReturnFocus 与 useDialogFocus 打开期间加锁、清理时解锁。
- 守卫：dialogFocus.test.ts 新增 3 条单测（只锁可滚祖先不碰弹窗自身 / 嵌套引用计数内层先解仍锁 / 无弹窗或无 window 时 no-op 不抛）；a11y.test.ts 新增 describe「弹窗打开锁背景滚动（R429）」2 条（lockAncestorScroll 关键实现断言 + 两个钩子各含 lockAncestorScroll( 与 unlockScroll()）。
- 反向自检：把 entry.count += 1 改坏 → 恰好 2 failed（引用计数单测 + 静态守卫点名），还原后 96 passed。

### R428: 创意工坊弹窗补上可访问名——读屏进入弹窗先念出「创意工坊 · 游戏名」

- 缺口：SteamWorkshopModal 的 role=dialog 是全仓 19 处弹窗里唯一没有 aria-label / aria-labelledby 的（审计发现：SteamInspectorModal、DryRunPreviewDialog、ConfirmDialog、Settings、Studio、bench×3、market、toolwall×5、HwPanels、CleanupProposalDialog、Disclaimer 都有名字，唯独这个创意工坊子模态裸奔）——读屏用户进入弹窗后无从知道自己在哪。
- 改动（apps/desktop/src/components/SteamWorkshopModal.tsx）：dialog 容器补 aria-labelledby="steam-workshop-title"，指向头部标题 div 的 id="steam-workshop-title"（标题文本 = workshopTitle · 游戏名，中英随 i18n 动态变化，用 labelledby 引用而非 aria-label 写死）。
- 守卫：apps/desktop/src/a11y.test.ts 新增 describe「弹窗的可访问名（R428）」2 条——① 站点断言：SteamWorkshopModal.tsx 同时含 aria-labelledby 与 id；② 全仓扫描器：逐行找 role=dialog / role=alertdialog（跳过 role= 出现在 < 之后的注释行），按 R427 同款「逐行抽开标签到闭合」逻辑，无 aria-label / aria-labelledby 则记入 offenders。
- 反向自检：临时删掉 aria-labelledby → 恰好 2 failed（站点断言 + 扫描器点名 ./components/SteamWorkshopModal.tsx:164），还原后 76 passed。

### R427: 提醒设置两个下拉框补上读屏名——视觉提示与可访问名终于对上了

- 缺口：设置页「自动进白名单」的列表选择与阈值下拉只有视觉提示（R301 补的 span 文案），读屏软件读出来是没有名字的下拉框——视障用户不知道这个下拉是干什么的、也读不到阈值含义；R301 的词条 autoListThreshold 写了却从没被任何 aria 属性引用。
- 改动（apps/desktop/src/components/ReminderSettings.tsx）：列表选择下拉补 aria-label=autoListPick；阈值下拉给提示 span 加 id="reminder-threshold-hint" 再用 aria-labelledby 关联（读屏名直接来自可见文案，词条终于被用上）；两个下拉都补 autoComplete="off"。
- 守卫：a11y.test.ts 新增 describe「下拉框的可访问名（R427）」2 条——① ReminderSettings 站点断言 aria-label 与 aria-labelledby 都在；② 全仓扫描器：任何 <select> 若无 aria-label / aria-labelledby / title 且不在 <label> 上下文中就点名报错（SystemTools / HwPanels / bench / FileView 四处均有名，基线干净）。

### R426: 弹窗初始焦点避开关闭按钮——打开弹窗按 Enter 不再立刻关掉它

- 缺口：R424/R425 的 useDialogFocus 取「最上层弹窗第一个可聚焦元素」，但所有弹窗 DOM 顺序第一个可聚焦元素就是右上角的关闭 X——键盘用户打开弹窗后按 Enter/空格 = 立刻关掉刚打开的弹窗（破坏性默认），读屏用户也被引导到「关闭」。
- 改动：useDialogFocus 进场时先找头部（.modal-head / .steam-modal-head），优先头部之外第一个可聚焦元素（取消 / 主操作 / 搜索框等真正要交互的元素）；整个弹窗只剩头部有可聚焦元素时才回退头部第一个，再回退容器本身。
- 守卫：a11y.test.ts 新增 describe「弹窗初始焦点避开关闭按钮（R426）」1 条（头部选择器 + filter 表达式），R424 原断言同步改为匹配新回退链 ?? list[0] ??。反向自检：临时去掉头部过滤 → 2 failed 点名 R424 + R426 两条，已还原。

### R425: 挂载式弹窗补进场焦点——预览窗 / 提案窗 / 安装窗打开即在弹窗里

- 缺口：R424 给内联弹窗接了进场 + 归还一体的 useDialogFocus，但 7 处挂载式弹窗里有 3 处只挂了只管归还的 useReturnFocus——打开瞬间焦点还留在触发按钮上（DryRunPreviewDialog 预览窗 / CleanupProposalDialog AI 提案窗 / Studio InstallScaffoldModal 安装窗），键盘 Tab 第一步就走出弹窗。
- 改动：三处的 useReturnFocus(...) 换成 useDialogFocus(...)（进场与归还一体）——预览窗 useDialogFocus(true)、提案窗 useDialogFocus(proposal !== null)（常驻挂载，提案一出现即进场）、安装窗 useDialogFocus(true)；注释同步更新。
- 保留：ConfirmDialog（cancelRef 初始焦点）/ ContextMenu（首项 focus）/ Settings（dlgRef）/ HwPanels（autoFocus 确认词输入框）四处已有自己的初始焦点，继续用 useReturnFocus，口径不变。
- 守卫：a11y.test.ts 新增 describe「挂载式弹窗的进场焦点（R425）」4 条——三处站点断言含 useDialogFocus 且不再含 useReturnFocus，第四条钉住保留 useReturnFocus 的四处不被误改。反向自检：提案窗还原成 useReturnFocus → 恰好 1 failed 点名该条，已还原。

### R424: 弹窗焦点进场与归还——打开即在弹窗里，关闭回到触发按钮

- 缺口：R344 的 useReturnFocus 只负责「关闭后归还」，且只接了 7 处挂载式弹窗；内联在父组件里的 11 处弹窗（工具箱详情 / 导出 / 回收 / 启动确认 / AI 工具详情、跑分三确认、插件市场写操作确认、Steam 详情与创意工坊）完全没有焦点处理——打开后焦点还留在触发按钮上，键盘用户按 Tab 第一步就走出了弹窗，读屏用户也不知道弹窗已打开。
- 改动：dialogFocus.ts 新增 useDialogFocus(open)（进场 + 归还一体）：打开时先记 opener（focusReturnTarget(document.activeElement)），再取 MODAL_SEL 中 DOM 顺序最后一个 = 最上层弹窗，聚焦 focusableWithin(top)[0]（容器可聚焦时回退容器），关闭 / 卸载时归还 opener；useReturnFocus 的过期注释（「留给后续抽出统一弹窗组件」）同步更新为 R424 口径。
- 接入 11 处：toolwall.tsx（ToolDetailModal：useDialogFocus(true) + exportAsk / recycleAsk；CatalogWall：confirmLaunch / agentDetail）；bench.tsx（confirmStress / confirmDiskRun / confirmDonut）；market.tsx（confirmAsk !== null）；SteamInspectorModal.tsx / SteamWorkshopModal.tsx（条件挂载，useDialogFocus(true)）。
- 守卫：a11y.test.ts 新增 describe「弹窗焦点进场与归还（R424）」6 条——dialogFocus.ts 单独 glob 断言 useDialogFocus / focusableWithin(top)[0] / prev.current?.focus?.() / MODAL_SEL 查询；五个组件文件逐站点断言钩子调用文本。反向自检：删 market 的钩子 → 恰好 1 failed 点名该断言，已还原。

### R423: 主题系统的四张事实表对偶守卫——17 个预设删掉一个语义键当场红，theme.ts 从零测试到 22 条

- 缺口：theme.ts（579 行，启动即跑）此前零测试。它内部有四张互相对偶的事实表——ThemeId 联合（17 项）、THEME_FONTS（17 键）、PRESETS（17 块色板）、DARK_IDS（5 个深色主题）——任何一张漏配都不会编译报错（对象字面量缺键只是静默回退 vercel），这正是 R420 修过的「换主题漏配」事故的复发通道；loadTheme/saveTheme/applyTheme 的行为契约（坏 JSON 回落、accent 合法性、fontScale 钳制、深浅色 data-mode/colorScheme、getKeepFiles 钳制）也全无测试。
- 改动：无生产代码改动，纯测试落地。
- 守卫：新增 theme.test.ts（22 条）——四表对偶（THEME_IDS==THEME_FONTS 键集==PRESETS 块逐键相等、无重复）；公共语义键交集必须覆盖 R420 语义链（--bg/--surface/--text/--border/--accent 族/--risk-* 族/--ok 等 15 个关键键，且每个预设必须全含交集键）；圆角口径分册（minimal 0.75rem / doubao 1.2rem / barbie 1.8rem / brand 2rem / 21th 0 等 9 个自带 --radius，其余 8 个明确回退 tokens.css 的 1rem）；DARK_IDS 恰 5 个且都在 ThemeId 内；loadTheme 六种边界（无存值/坏 JSON/未知 id/非法 accent/非法 fontScale/合法透传）；applyTheme 深浅色与自定义 accent 两档（提亮深字+状态色跟随、压暗白字+状态色保持）+ fontScale 钳制 0.8–1.3；saveTheme 写盘+应用+广播 diskpilot:theme-changed；initTheme 启动路径；getKeepFiles/setKeepFiles/pref 契约；存储抛异常静默回落。
- 实现细节：PRESETS 未导出，用 import.meta.glob ?raw 抓源码静态解析（与 a11y.test.ts 同款）；theme.ts 是 CRLF，行解析先剥行尾 \r 再上正则锚点；该文件 ?raw 内容 35306 字符可直接 glob 取到。
- 反向自检：向关键链注入幽灵键 --bogus-probe → 1 failed 正点交集用例，还原后 22 passed。
- 验证：pnpm -C apps/desktop check EXIT=0，77 files / 1108 tests。

### R422: 权限表的防漂移守卫与读盘布尔卫生——手写的 "false" 字符串不再被误读成「开启」

- 缺口：permissions.ts 的 loadPermConfig 用 Boolean(存值) 收布尔——Boolean("false")===true，手写/异构存值（"false" 字符串、0/1）会把这个「关闭」误读成「开启」；且 PERMS（22 项权限定义）与 DEFAULT_ENABLED（22 项默认开关）是两处互相依赖的表，加权限只改一张不会编译报错，isPermEnabled 的 L0 恒开 / L3 恒关短路与逐键合并就可能在静默里漂移。

- 改动：loadPermConfig 只认字面布尔 true（=== true），异构值一律按关（与 L3「门没有锁孔」同口径——关是安全基线，宁可错关不可错开）。

- 守卫：新增 permissions.test.ts（13 条）——表一致性（PERMS id 集 == DEFAULT_ENABLED 键集、无重复；labelKey/descKey 按 perm.<id>.label/.desc；L0 恰为三项只读、L3 恰为四项永久禁止）；isPermEnabled 短路契约（L0 显式 false 仍开、L3 显式 true 仍关、L1/L2 默认与逐项覆盖）；读盘卫生（无存值/坏 JSON 回落默认、未知键与异构形状丢弃、字符串 "false"/0/1 一律按关、L3 存 true 仍被 isPermEnabled 拒绝）；savePermConfig 往返 / 写盘失败静默 / 会话授权 grant-has-clear。

- 反向自检：还原 Boolean(存值) → 1 failed 正点中「"false" 字符串」用例（expected true to be false），还原后 13 passed。

- 验证：pnpm -C apps/desktop check EXIT=0，76 files / 1086 tests。

### R421: 压测与体检报告的卸载守卫——切走 tab 不再让长任务往已卸载面板落地

- 缺口：BenchPanel 挂在 Toolbelt 的 view 条件渲染下（Toolbelt.tsx:118），切到别的 tab 直接卸载。压测 runStress 是 5–300s 的同步 call，期间 liveTimer 每 2s 打一次后端 sensor_trend，run() 的 await 返回后还会 setResults / setBusy(null) 落到已卸载组件；体检报告 runReport 的 setReport / setBusy(null) 同理。生产代码没有崩（React 18 只是警告），但轮询白打、状态错挂，违背仓内 R382/CleanupPage/Studio 一以贯之的 alive 房规。

- 改动：bench.tsx 加 aliveRef + 卸载 effect——setup 置 true（main.tsx 有 StrictMode，重复挂载会重跑 setup，不能只在 useRef 初值置一次），cleanup 置 false 并掐掉实时温度轮询；run()/runReport 的 setResults、setReport、两处 finally 的 setBusy(null) 全部先查 aliveRef；poll() 在发请求前与响应落地前各查一次；runStress 等 run() 返回后先查 alive 再写实时温度。persistHistory（localStorage）与 toast 是全局副作用，卸载后照发，不加守卫并注明。

- 守卫：a11y.test.ts 新增 describe「长任务的卸载守卫（R421）」4 条——aliveRef 声明 + setup/cleanup 置真置假 + clearInterval；poll 两处 R421 注释锚 + setLiveTemp 受守；逐行扫 bench.tsx 里所有 setBusy(null) / setResults((prev) => ({ ...prev, [key]: / setReport 行必须带 aliveRef.current；runStress 的「卸载后实时温度与曲线不再落地」锚。

- 反向自检：拆掉 run() finally 里一条 setBusy 守卫 → 1 failed 点出该行缺 aliveRef.current，还原后 61 passed。

- 验证：pnpm -C apps/desktop check EXIT=0，75 files / 1073 tests。

### R420: CSS 裸 hex 语义色收口到 token——危险红 / 健康绿 / 分隔线随主题走，换主题不再漏配

- 缺口：主题机制是 applyTheme() 把整套语义 token 写成 <html> 内联变量（theme.ts），CSS 里写死的非主题 hex 换主题时必然漏配——settings.css:52 进度条失败底 #d94d6a、:477 fileview-stats 分隔线 #f0d8e4（浅粉，深色主题下不变量）、:814/:823 button.danger #e5484d/#d13438、:841 sys-tag-green #16a34a、cleanup.css:407-415 armed 确认红 #c43358/#8a1d3a + 脉冲 rgba。已有守卫（token-guard rgba 基线 / inlineColor R407 内联 style hex）都不拦主题无关 CSS 文件里的裸 hex。

- 改动：上述全部改走语义 token + color-mix：进度失败底 → var(--risk-danger)；fileview 分隔线 → var(--border)；button.danger → var(--risk-danger)，hover → color-mix(in srgb, var(--risk-danger) 85%, #000)（深浅主题都朝暗的方向）；sys-tag-green → var(--risk-safe) + color-mix 底/边（settings.css rgba 5→3）；cleanup armed → var(--risk-danger) + color-mix 边框，脉冲 keyframes → color-mix(in srgb, var(--risk-danger) 45%, transparent) / transparent（cleanup rgba 2→0）。刻意保留的固定色入册说明：chat 代码块深色 editorial 卡（#1a1220/#f5e8ee）与工具墙 GPU donut 仪表仪器色（#080612/#7dfc9f/#ff5252/#ff8080）——主题无关，与 rgba 基线里的 donut 黑场同口径。

- 守卫：token-guard.test.ts 新增 describe「裸 hex 语义色基线（R420）」——去注释 + 循环剥 var(--x, #y) 回退与 color-mix(...) 调用后，非白名单文件的裸 hex 除 #fff/#ffffff 外必须落在 HEX_KEEP 册（按文件分册）；新增裸语义色直接红，先改语义 token，确属固定色才允许进册并注明理由。rgba 既有基线不动（settings 5→3、cleanup 2→0 只减）。

- 反向自检：往 steam.css 注入 .re402-probe { color: #123456; } → 1 failed 点出「./steam.css 出现未入册裸 hex：#123456」，还原后 2 passed。

- 验证：pnpm -C apps/desktop check EXIT=0，75 files / 1070 tests。

### R419: 跳过链接落点的焦点可见——键盘回车跳到 main 不再丢位置

- 缺口：app-shell.css main:focus 与 main:focus-visible 都是 outline: none。main 正是 R236 跳过链接（skip-link → #main-content，App.tsx tabIndex={-1}）的落点——键盘用户 Tab 到跳过链接回车跳转后焦点落在 main 上，却没有任何可见指示，下一步 Tab 进哪完全靠猜，跳过链接等于白做。

- 改动：按仓内既有「:focus 抹掉描边必须给替身」口径（R405 聊天输入框 / R348 modal-tab）补 main:focus-visible { outline: 2px solid var(--accent-strong); outline-offset: -2px; }——鼠标点击不画框，键盘跳转画内部描边；main:focus 保留 outline: none（程序性 focus 不画）。

- 守卫：a11y.test.ts 新增 describe「跳过链接落点的焦点可见（R419）」2 条——① main:focus-visible 切块内不得再有 outline: none、必须有 2px accent-strong 描边与 -2px offset；② 落点锚仍存在（.skip-link:focus-visible + App.tsx 的 href="#main-content" / id="main-content" / tabIndex={-1}），避免落点没了守卫空转。

- 反向自检：main:focus-visible 改回 outline: none → 1 failed，还原后全绿。

- 验证：pnpm -C apps/desktop check EXIT=0，75 files / 1069 tests。

### R418: 保留天数输入钳制回显——清空框不再瞬变「保留 0 天」，上限当场可见

- 缺口：CleanupBody 的「保留最近 N 天」是 type=number 且 onChange 里直接 Number(e.target.value) 写回 store（setDays → sanitizeRetentionDays 钳 [0,3650]）：① 清空框的瞬间 sanitizeRetentionDays(0)=0，「保留 0 天」= 什么都能删，是删得最狠的值却随手可达；② 框里显示 5000 时 store 已钳到 3650——用户看到的和实际执行的是两个数，方向是「以为更保守、实际更激进」；③ 输入框没有 max/step，浏览器微调按钮也停不到 3650。

- 改动：① model.ts 导出 MAX_RETENTION_DAYS = 3650，sanitizeRetentionDays 的钳制改用该常量——输入上界与钳制上限同源，改上限只改一处；② CleanupBody 改「草稿 + 失焦提交」：daysDraft 只记本地字符串，onBlur / Enter 提交时非数字（含空串）回退该 scope 的 prompt.default（不是 0）、越界走 sanitizeRetentionDays 钳制，再 setDays 回显钳制后的真值并清 draft；③ 输入加 max={MAX_RETENTION_DAYS} step={1}，微调按钮同样停在上限。

- 守卫：a11y.test.ts 新增 describe「保留天数输入框的钳制回显（R418）」2 条——① model.ts 必须导出 MAX_RETENTION_DAYS = 3650、sanitize 用常量钳制且无裸 3650 上限、CleanupBody 必须 max={MAX_RETENTION_DAYS}；② 输入框区域（keepRecent…daysUnit 切片）必须是 daysDraft 草稿值、有 onBlur 与 Enter 提交、空串回退 fallback、走 sanitizeRetentionDays(parsed)、提交后清 draft，且不得再出现 setDays(scope.id, Number( 直写。

- 反向自检：把 max 改回 9999 → 1 failed（报上限常量那条），还原后全绿。

- 验证：pnpm -C apps/desktop check EXIT=0，75 files / 1067 tests。

### R417: 挂载期拉取补卸载守卫——迟到的响应不落地到已卸载组件

- 缺口：4 个「仅挂载时拉取」的 effect 直接在 .then/.catch 里 setState，组件卸载后迟到的响应仍会落地：SpaceTrendCard 的 getSpaceHistory、设置页的 getHwProfile、工具墙 CatalogWall 的 toolbeltManifests 与 listTools（后者 catch 里还嵌套 agentReady().then）、Toolbelt 的 toolbeltCatalog。其余拉取（CleanupPage/Studio/DryRun/ReminderSettings/AssetOverview 等）早有 alive/cancelled/req token 守卫，这 5 处是漏网的。

- 改动：5 处 effect 统一补 let alive = true + 回调首查 if (!alive) return + return () => { alive = false; }；listTools 的 catch 内嵌套 agentReady().then 也查 alive。同步部分（Toolbelt 的 setCatalogErr(null)、toolwall 的 !isTauri early-return）保持不变。

- 守卫：a11y.test.ts 新增 describe「挂载期拉取的卸载守卫（R417）」3 条——显式站点清单（AssetOverview/Settings/toolwall 两处/Toolbelt）按 marker 定位所在 useEffect 块，断言块内含 let alive = true、清理函数与 if (alive)/if (!alive) 守卫；设置档位与目录、清单与嵌套探活各有精确文本断言。

- 反向自检：删 Settings 的 if (alive) setHw(null) → 1 failed（报 R417「设置页性能档位与工具墙目录」那条），还原后全绿。

- 验证：pnpm -C apps/desktop check EXIT=0，75 files / 1065 tests。

### R416: 跑分历史的展开态不再跟着新记录漂——index key 换时间戳身份

- 缺口：bench.tsx 的两处跑分历史列表（通用跑分 + 压测）都是「新记录插最前」（persistHistory 尾插 + 渲染 reverse），却用 key={i}。跑完新一次后所有条目位移一格，React 按 index 复用 <details> DOM——用户展开的旧记录展开态漂到别的记录上，读到的是另一份结果。
- 改动：两处 <details> 的 key 改为 h.at（persistHistory 写入的 Date.now() 时间戳，每条唯一）；map 回调去掉不再使用的 i；第一处 map 箭头前补注释说明 R416 缺口。
- 守卫：a11y.test.ts 新增 describe「跑分历史的条目身份（R416）」1 条——bench.tsx 必须恰有 2 处 <details key={h.at}>、不得出现 <details key={i}>、两处 reverse().slice(1) 列表的 map 回调不得再带 index 参数。
- 反向自检：把 key 还原成 {i} → 1 failed，还原后全绿。中途踩坑：JSX 注释放在 map((h) => ( 的箭头括号内是语法错误（TS1005），注释须提到 map 调用之前。
- 验证：pnpm -C apps/desktop check EXIT=0，75 files / 1062 tests。

### R415: AI 配置持久化接上 safeStorage——存储异常不再冒充「连接失败」

- 缺口：advisor/provider.ts 的 saveSettings / clearSettings 是裸 localStorage.setItem / removeItem——R395 的兜底只覆盖 safeStorage 自身与 4 个启动文件。隐私模式 / 配额满时：Settings「保存 / 试连」包了 try/catch，存储异常被拼进「连接失败：<错误>」误导排查方向；「清除配置」按钮没包 try，异常直接炸掉事件处理器。
- 改动：provider.ts 三处读写全部改走 safeStorage（saveSettings→safeSetItem、clearSettings→safeRemoveItem、loadSettings 的读→safeGetItem）——写失败静默丢弃（与 R395「存储不可用不影响主流程」口径一致），save/clear 不再抛。
- 守卫：safeStorage.test.ts 的裸调用黑名单加入 ./advisor/provider.ts；provider.test.ts 新增 describe「设置持久化走 safeStorage（R415）」2 条——抛异常的存储 stub 下 saveSettings/clearSettings 不抛、正常存储下写入落盘清除可读回。
- 反向自检：把 saveSettings 还原成裸 localStorage.setItem → 1 failed（报 advisor/provider.ts:61），还原后全绿。
- 验证：pnpm -C apps/desktop check EXIT=0，75 files / 1061 tests。

### R414: 工具墙弹层 Esc 一致性——导出 / 回收确认框也能按 Esc 取消

- 缺口：R142 给工具墙补了三个弹层的 Esc（启动确认 / exe 详情 / AI 工具详情），但 ToolDetailModal 里后加的导出确认（exportAsk）与回收确认（recycleAsk）两个确认框没有 Esc——同一界面里「有的弹窗能 Esc、有的不能」，且这俩恰是写操作确认。
- 改动：把两条 useEscapeKey 挂进 ToolDetailModal（exportAsk 声明之后）：setExportAsk(null) 受 exportAsk !== null && !exporting 约束、setRecycleAsk(false) 受 recycleAsk && !recycling 约束——R349 的栈只把 Esc 交给最深一层，所以按 Esc 只取消当前确认、不连带关掉背后的详情弹窗；进行中的导出/回收不响应（避免半途关窗看着像成功）。CatalogWall 注释同步指回 ToolDetailModal。
- 守卫：a11y.test.ts 新增 describe「工具墙弹层 Esc 取消（R414）」2 条——① 第一个到最后一个 useEscapeKey( 的切片必须含 setConfirmLaunch(null)/setDetail(null)/setAgentDetail(null)/setExportAsk(null)/setRecycleAsk(false) 五个 setter；② 两条新钩子的精确文本必须在场。反向自检：删 setExportAsk 行 → 2 failed（报 setter 缺失），还原全绿。
- 验证：pnpm -C apps/desktop check EXIT=0，75 files / 1059 tests。

### R413: 凭据与代码类输入框的表单卫生——API key 不再被密码管理器认领，代码框不再画红波浪线

- 缺口：全仓没有任何 autoComplete / spellCheck 声明。设置页的 API key 输入（默认 type=password）没有显式 autoComplete，浏览器的密码管理器会把它当成站点登录口令跨站保存、并在别处自动填充；8 个 textarea 只有 3 个关了拼写检查，MCP 表单的 args/env/headers（-- 参数、API_KEY=sk-xxx、Authorization: Bearer …）和工具箱的 .tb-args CLI 参数框都会被浏览器画红波浪线；baseUrl / model 也会收到无关的自动填充建议。
- 改动：Settings.tsx 的 key 框加 autoComplete="new-password" + spellCheck={false} + autoCapitalize="off" + autoCorrect="off"；baseUrl / model 加 autoComplete="off" + spellCheck={false}；McpServers 的 args/env/headers 等代码类 textarea 与 name/command/url/cwd 输入补 autoComplete="off" + spellCheck={false}；toolwall 两处 .tb-args 参数框加 spellCheck={false}。
- 守卫：新建 apps/desktop/src/formHygiene.test.ts（import.meta.glob 读 TSX raw）三条硬规则——① 密码类输入框必须显式声明 autoComplete 且不许是 "on"（应为 new-password 或 off）；② 除聊天输入（chat.input.aria / chat-input 类名）外，所有 textarea 必须 spellCheck={false}；③ .tb-args 输入框必须 spellCheck={false}。外加「设置页 key 框四属性」区域断言与假数据自检（bad 例 2+1+1 命中 / ok 例全绿）。
- 反向自检：删掉 headers 框的 spellCheck={false} → Tests 1 failed（报 McpServers.tsx:859 带行号），还原后全绿。
- 验证：pnpm -C apps/desktop check EXIT=0，75 files / 1057 tests（1051 + 6）。

### R408: AI 侧栏记住的宽值有了上界——窄窗不再被挤爆中栏保底

- 缺口：App.tsx 的 aiWidth 初始化只判 Number.isFinite(v) && v >= 260，没有上界；fit 兜底（窗口缩放/AI 栏开合后保中栏 MIN_CENTER）只收缩左/右栏、从不收 AI 侧栏。大屏拖宽后记住的值（如 800）在窄窗上会把工作台中栏挤到保底以下。
- 改动：新增 apps/desktop/src/panelWidths.ts 的纯函数 clampAiWidth(v, winW, sidebarFootprint)——上界 = 窗口宽 − 侧栏占位 − AI_MAX_BUDGET(480)，下界 260，非法值回默认 300；nav.ts 新增 sidebarFootprintAt(winW, preferredExpanded)（含窄窗自动折叠的占位）。
- 接线：aiWidth 初始化走 clampAiWidth；dragAi 的手写 Math.max(260, …) 收进同一函数（口径不再两处）；fit 兜底在左/右收到底仍超预算时收 AI 侧栏；Splitter 的 min={260} 改用 AI_WIDTH_MIN。
- 测试：panelWidths.test.ts 8 条——clampAiWidth 四类取值 + R408 守卫（App.tsx 初始化块必须含 clampAiWidth/sidebarFootprintAt 且不含旧的裸 isFinite 判定；fit 兜底必须收 AI 侧栏；nav.ts 必须导出 sidebarFootprintAt）。反向自检：删掉初始化里的 clampAiWidth → Tests 1 failed，还原后全绿。
- 验证：pnpm -C apps/desktop check EXIT=0。

### R407: 内联样式的语义色收口——3 处写死 hex 改走 token + 源码守卫

- 缺口：MCP 审计行的「已拒」徽标 borderColor: '#e05c51'、可写警告条 background: '#fff3cd' + borderLeft: '4px solid #ffc107'、ReminderSettings 的未知脚手架提示 color: '#b45309'——内联样式绕开 tokens.css，深色主题下不跟着反相，也不受设计系统统一调整（仓库对 CSS 文件已有硬编码色值禁令，TSX 内联对象此前没人看）。
- 改动：三处改用 var(--risk-danger) / var(--warn-soft) + var(--risk-caution) / var(--risk-caution)；lucide 图标的 color prop 同步换 token。
- 守卫：新建 apps/desktop/src/inlineColor.test.ts——扫所有 TSX 的 style={{…}} 对象，出现裸 hex 整值（: '#hex'）或 border/outline 简写里的 hex 即违规；var(--token, #fallback) 回退形式、调色板数据（categoryColor/文件类型色/主题色板/图表系列色，都在对象外）不受影响。例外只有 gpu-donut.tsx（坏点测试必须渲染纯白/纯黑/纯 RGB 才能判死点，注释写明且不在断言里引用那些字面量）。3 条用例含假数据自检。
- 反向自检：把 McpServers:401 还原成写死 hex → Tests 1 failed（报 文件:行号 写死颜色），还原后全绿。
- 验证：pnpm -C apps/desktop check EXIT=0。

### R406: 聊天区翻历史时不再被新消息拽回底部——贴底才跟随

- 缺口：ChatPanel 在 turns.length / chatBusy 变化时无条件 scrollTo(scrollHeight)——用户往上翻历史，一条新回复（或流式增量）直接把滚动位置拽回底部，正在读的内容飞走。
- 改动：新增 apps/desktop/src/stickBottom.ts 的纯函数 stickNearBottom(el, thresholdPx=80)（scrollHeight - scrollTop - clientHeight <= 阈值算贴底；null/undefined 按跟随处理；NaN 不跟随）；ChatPanel 新增 stickRef（初始 true，首屏仍滚到最新），chat-scroll 容器加 onScroll 实时维护 stickRef，原有滚动 effect 开头加 if (!stickRef.current) return;。
- 测试：stickBottom.test.ts 6 条（贴底/过冲/翻历史/阈值边界 80/空元素/NaN）。
- 守卫：a11y.test.ts 新增 describe「聊天区贴底跟随（R406）」2 条——ChatPanel 必须同时含 onScroll + stickNearBottom(e.currentTarget) + 滚动 effect 的 stickRef 守卫，且守卫出现在 scrollTo 调用之前。反向自检：临时删掉守卫行 → Tests 2 failed，还原后全绿。
- 顺手：scripts/check-changelog.mjs 补一条 2.5 规则——## [未发布] 必须出现在所有 ### 条目之前（R405 落地时把新条目写到了标题上面，原有守卫「恰好一个 [未发布]」「标题前有空行」都照样通过，只有这条能抓住）。
- 验证：pnpm -C apps/desktop check EXIT=0，72 files / 1040 tests（1032 + 8）；check-changelog / check-docs 均 EXIT=0。

### R405: 键盘 Tab 到自定义输入框时焦点描边不再被吞——4 处 outline:none 补 :focus-visible 替身 + CSS 守卫

- 缺口：layout.css 的全局 input/select 只在 R348 补了 :focus-visible 描边，textarea 与自绘控件不在覆盖范围——Studio TOML 导入框（.scaffold-toml-input）、清理提案高危确认词（.proposal-danger-input）、工具箱 CLI 参数（.tb-args）、聊天输入框（.chat-input textarea）都在 :focus 里 outline: none，只剩 1px 边框变色，键盘用户看不出焦点在哪。
- 改动：四处各补 :focus-visible（2px var(--accent-strong) + outline-offset:-2px）——layout.css / overview.css / toolwall.css / chat.css；:focus 的边框变色保留给鼠标（点击时不画突兀的框），Tab 到的才亮描边。
- 守卫：focusVisible.test.ts 新增 describe「焦点描边被抹掉的控件要有替身（R405）」——解析全部 CSS 块，凡选择器含 :focus\b（排除 :focus-visible / :focus-within）且 body 含 outline: none 的块，其选择器键（去 :focus、逗号拆开）必须在某条 outline 非 none 的 :focus-visible 规则里出现过；main 豁免（程序化移焦刻意不画框）。假数据自检：裸 .x:focus{outline:none} 报红、配对 :focus-visible 后变绿、main 豁免；反向自检删 .tb-args:focus-visible → focusVisible.test.ts Tests 1 failed（6 中 1 红），还原后全绿。
- 验证：pnpm -C apps/desktop check EXIT=0，71 files / 1032 tests（1031 + 1）。

### R404: 输入法组合期间的 Enter 不再把打一半的句子发出去

- 缺口：聊天输入框（ChatPanel.tsx:922）onKeyDown 里 Enter 且非 Shift 就 preventDefault + askFollowUp，没有任何输入法组合（IME composition）守卫——中文/日文/韩文用户选候选词时按 Enter 是「确认候选字」，会被直接当成「发送」，把打一半的句子发出去。
- 改动：新增 composingRef 组合状态位（onCompositionStart 置 true / onCompositionEnd 置 false），onKeyDown 第一行 `if (composingRef.current || e.nativeEvent.isComposing) return;` 双保险（覆盖部分浏览器最后一次 keydown 仍带 isComposing 的情况）；注释写明 R404 缘由。
- 守卫：a11y.test.ts 新增 describe「输入法组合期间的 Enter 不许发送（R404）」，断言 ChatPanel.tsx 源码含 onCompositionStart / onCompositionEnd / e.nativeEvent.isComposing / 组合状态位短路。反向自检：临时删掉 isComposing 条件 → Tests 1 failed（46 中 1 红），还原后全绿。
- 验证：pnpm -C apps/desktop check EXIT=0，71 files / 1029 tests。

### R403: 自定义可聚焦控件的焦点样式对账守卫——没有 :focus-visible 的就报红

- 背景：原生 button/input 有浏览器默认焦点环（app-shell.css 也统一过 :focus-visible 口径），但 role="button" / tabIndex={0} 的 div/span/li 什么都没有——键盘 Tab 过去时屏幕上看不出焦点在哪。R314/R322/R368/R371 都是逐个补的焦点样式，没有机器守卫，新控件很容易漏。
- 新增 apps/desktop/src/focusVisible.test.ts：扫全部 TSX（剥 JSX/行注释防注释冒充标签，花括号深度 + 引号跟踪取完整标签），凡带 tabIndex={0} 或 role="button" 的标签，取其静态类名（没有就回看紧邻父层 className，如 li 在 ul[className] 里），必须在某个 CSS 的 :focus-visible 选择器里出现过，否则报 文件:行号 + 类名。
- 自检两道：① 扫到的源码 >20 份、covered 类 ≥5 个（守卫不是空转）；② 假数据（只有 .other:focus-visible 的样式 + class="no-focus" 的 button）必须命中「没有 :focus-visible 规则」。
- 当前全仓 15 个 covered 类 / 0 违规（ao-top-row、ao-foot-refresh、fv-ext-row、fileview-row、splitter、studio-detail-path、studio-children、tree-row、banner-dismiss、ctxmenu-item、chat-session-main、skip-link 等）。
- 验证：pnpm -C apps/desktop check EXIT=0，71 files / 1028 tests。

### R402: 删掉 5 个零引用的 Rust 公共项——死代码不再挂在公共 API 上

- 缺口：workspace 内 Rust 公共项全量扫描（1037 个 pub fn/struct/const 等，跳过测试模块）发现 5 个名字除定义外全仓库零引用：usn.rs 的 UsnChange::is_delete 与 is_rename_old 方法、winapp2.rs 的 reg_keys_summary（文档写明「供 UI 展示」但 UI 从未调用）、steam-inspector 的 discover_steam_root（只能在 Windows 上可能有用但无人用）、toolbelt manifest.rs 的 manifest_index（文档写「供前端/AI 注入用」同样无人用）。
- 改动：逐个删除（含各自的 doc 注释），并清理因此变未使用的 use std::collections::HashMap 两行（winapp2.rs / manifest.rs）。保留了仍在用的 USN_REASON_RENAME_OLD_NAME 常量（usn_tests.rs 与 usn_tree.rs 都还在引用）。
- 验证：重扫确认死项归零；cargo fmt --all -- --check EXIT=0（删完先跑了一次 fmt 修正空行）；cargo clippy --workspace --all-targets -- -D warnings EXIT=0（仅 binrw 依赖的 future-incompat note）；cargo test -p diskpilot-scanner -p diskpilot-scaffold -p diskpilot-steam-inspector -p diskpilot-toolbelt --no-fail-fast 全部 suite ok、TEST=0。

### R401: 聊天流式更新期间声明 aria-busy——读屏不再把拼到一半的半句话念完

- 缺口：R399 给对话区加了 role=log + aria-live=polite，但 AI 回答逐字流式写入时读屏会把每次增量都当独立更新播报，把半句中文/半截工具名念出来；aria-busy 是 live region 的标准「更新进行中，等一等再念」信号，全仓此前只有 HwPanels 的 spinning 用过。
- 改动：ChatPanel 的 chat-scroll 加 aria-busy={chatBusy || undefined}（打字/工具调用进行中为 true，空闲不写该属性）。
- 守卫：a11y.test.ts 的 R399 断言里补一条「chat-scroll 必须带 aria-busy={chatBusy || undefined}」（沿用剥注释再断言的写法，防注释冒充）。自检：临时删掉 aria-busy → 报 AssertionError expected ... to contain aria-busy=、Tests 1 failed、EXIT=1，还原后全绿。
- 验证：pnpm -C apps/desktop check 70 files / 1025 tests / EXIT=0。

### R400: 截断文字悬停能看到全文——8 处省略号文本补 title

- 缺口：从 CSS 里反向扫描 46 条 text-overflow:ellipsis 选择器，逐个到 TSX 核对引用点，找出「文字会被省略号截断、但元素或所在行没有 title 兜底」的 8 处：硬盘型号（Hardware 面板）、Steam 游戏主/别名与创意工坊条目标题、网络测速 URL、系统信息 User-Agent、工具箱 AI 关联列表、总览空间大头行（title 原只有提示没带目录名）、聊天头部标题、清理作用域名。
- 改动：HwPanels（hw-name）、SteamInspector（steam-row-name-primary/alt）、SteamWorkshopModal（steam-workshop-item-title）、services（tb-ping-url / tb-sys-ua）、toolwall（toolwall-ai-related）、AssetOverview（ao-top-row title 追加目录名）、ChatPanel（chat-title）、CleanupBody（cleanup-row-label）补 title={原文}。
- 已核实不是缺口（跳过）：FileView / TreeView / CleanupBody 路径行 / DryRunPreviewDialog 路径行的外层早已有 title={path}；AssetOverview 分类条的 button title 已含 scaffold.name；CleanupPage 脚本按钮 title 已含脚本名；Studio 子行 title 已含完整路径。
- 验证：pnpm -C apps/desktop check 70 files / 1025 tests / EXIT=0。

### R399: AI 对话记录能被读屏播报——chat-scroll 加 role=log，输入中提示补 role=status

- 缺口：ChatPanel 的对话区（div.chat-scroll）流式渲染 AI 回答、工具调用轨迹、输入中提示，但没有任何 live region——读屏用户全程听不到：AI 有没有在回、回了什么、还在不在想。chatBusy 的「正在打字」也没有 role=status。
- 改动：chat-scroll 加 role="log" aria-live="polite" aria-label={t(chat.transcriptAria)}（R399 注释说明流式动态内容的播报意图）；chat-typing 加 role="status"。i18n chat.ts zh/en 各加 chat.transcriptAria（zh「AI 对话记录，新消息会自动播报」/ en "AI chat transcript — new messages are announced automatically"）。
- 守卫：a11y.test.ts 新增「聊天记录是动态内容，要被读屏播报（R399）」——取 RAW_TSX 的 ChatPanel.tsx，先剥 JSX 注释再断言必须含 class 与 role 的真实标签（第一版守卫没剥注释、自检时删掉 role 仍通过——注释里的字面量冒充了断言，修正后自检报错红）。自检：临时把 role=log 行改回裸 div → 报 AssertionError expected ... to contain <div className=chat-scroll role=lo...、Tests 1 failed、EXIT=1，还原后全绿。
- 验证：pnpm -C apps/desktop check 70 files / 1025 tests（1024+1）/ EXIT=0。

### R398: 最近清理面板不再整卡消失——台账为空时留头给诚实空态

- 缺口：UndoPanel（Studio 侧栏「最近清理」）在台账为空时直接 return null 整卡消失——同一个台账的整页 HistoryPage 有明确的空态文案（history.empty「还没有清理记录。执行清理后，每次操作都会记录在这里」），面板侧却什么提示都没有，且空卡消失会让侧栏下方组件布局跳动。
- 改动：UndoPanel.tsx 把 return null 改为空态分支——保留「最近清理」面板头（History 图标），内容行渲染 cleanup.undoEmpty（muted small）；settings.css 加 .undo-empty 规则（padding + var(--muted)）。
- i18n：cleanup.ts zh/en 在 cleanup.recent 后各加 cleanup.undoEmpty（zh「还没有可撤销的清理记录。执行清理（回收站 / 隔离 / 删除）后，最近操作会出现在这里。」/ en "Nothing to undo yet. Recent cleanups (recycle / quarantine / delete) will show up here."）。
- 验证：pnpm -C apps/desktop check 70 files / 1024 tests / EXIT=0。

### R397: MCP 传输方式切换不再留一个空列——seg-2 网格规则补上 + 守卫

- 缺口：layout.css 的基础 .seg 是 3 列网格，seg-3/seg-4/seg-5 变体都有各自的 grid-template-columns 规则，唯独 MCP 服务器表单里传输方式（stdio / http 两项）用的 seg-2 没有任何规则——两个按钮按 3 列排布，各占 1/3 宽，右边空出一个带外框的空列。
- 改动：layout.css 加 .seg-2 { grid-template-columns: repeat(2, minmax(0, 1fr)); }，并把各分段控件按钮尺寸注释补上「传输方式(2)」。
- 守卫：新增 apps/desktop/src/segGrid.test.ts——扫 TSX 里出现的每个 seg-N，layout.css 必须有对应的 .seg-N 规则（基础 .seg 是 3 列，N 不为 3 时缺规则必留空列/换行）。自检：临时删掉 .seg-2 规则 → 报 ./components/McpServers.tsx:803: seg-2、EXIT=1，还原后全绿。
- 验证：pnpm -C apps/desktop check 70 files / 1024 tests / EXIT=0。

### R396: 右键菜单触发器声明会开菜单——读屏听得到 4 处右键入口；顺手删死类型 ToolModule

- 缺口：全仓 4 处上下文菜单触发器（树视图行 TreeView.tsx、文件行 FileView.tsx、工作室路径/子项 Studio.tsx 两处）虽有 R368 的菜单键/Shift+F10 键盘入口，但元素没声明 aria-haspopup，读屏软件不知道「这里能唤出菜单」。
- 改动：4 处触发器各加 aria-haspopup="menu"（R396 注释）；删掉 toolbelt/shared.ts 里零引用的死接口 ToolModule（连带清掉只被它用的 LucideIcon import）。
- 守卫：a11y.test.ts 新增「右键菜单触发器要声明会开菜单（R396）」——逐文件对每个 onContextMenu 定位所在标签（lastIndexOf 找 < 起点 + 花括号/引号跟踪找 > 结尾），标签内必须有 aria-haspopup="menu"；ContextMenu.tsx 豁免（菜单本体，onContextMenu 只是吞原生菜单）。自检：删 TreeView 的 aria-haspopup → 报 ./components/TreeView.tsx:308、EXIT=1，还原后全绿。
- 验证：pnpm -C apps/desktop check 69 files / 1023 tests / EXIT=0。

### R395: 启动/渲染路径的本地存储读写全部兜底——存取出错不再能让整个应用崩掉

- 缺口：App.tsx 的 10 处 useState 初始化器/effect 与 LeftPanel、Settings（跳过更新版本）、toolwall（隐藏推广）共 15 处直接调 localStorage.getItem/setItem/removeItem 且没包 try/catch。其它路径（store/theme/i18n/permissions 等）都遵循「存储不可用就静默降级」，唯独启动引导路径例外：隐私模式/存储被禁用的浏览器预览里，读取抛错会让整个应用渲染失败（连到根 ErrorBoundary），点「同意」写失败会让事件处理直接崩。
- 改动：新增 apps/desktop/src/safeStorage.ts（safeGetItem / safeSetItem / safeRemoveItem 三个 try/catch 兜底函数），App.tsx（aiOpen/aiWidth/sidebarExpanded/leftWidth/rightWidth 的读写）、LeftPanel（leftView 读写）、Settings（skipRec 读/写/删）、toolwall（hidePromo 读写、readRecent/pushRecent 收口）全部改走 safe*。
- 测试：新建 safeStorage.test.ts 2 条单测（直通 / 读失败回 null·写删失败静默）+ 1 条迁移守卫（App.tsx / LeftPanel / Settings / toolwall 四个启动路径文件不许再出现裸 localStorage.getItem|setItem|removeItem）；自检：临时把 App.tsx 一处改回裸调用 → 报 ./App.tsx:159、EXIT=1，还原后全绿。
- 验证：pnpm -C apps/desktop check 69 files / 1022 tests / EXIT=0。

### R394: 工具箱页 / AI 助手 / 设置窗补上错误兜底——懒加载不再能让整棵树崩掉

- 缺口：App.tsx 里 Toolbelt、ChatPanel、Settings 三个懒加载挂载点只有 Suspense 没有 ErrorBoundary。Suspense 只接 loading 不接错误：chunk 取不到、渲染抛错时错误会一路冒到根，整个应用树被 React 卸载（其它页面早在 709/762/770/778/786/822/831 行都有兜底，这三处一直漏着）。
- 改动：三处各包一层 ErrorBoundary（AI 侧栏 ChatPanel、view===tools 的 Toolbelt、showSettings 的 Settings 弹窗），新增 i18n 键 shell.boundary.toolbeltFailed / chatFailed / settingsFailed（zh/en）。
- 守卫：新建 lazyBoundary.test.ts——对 AssetOverview/Toolbelt/CleanupPage/OptimizerPage/HistoryPage/ChatPanel/Studio/Settings 每个挂载点取之前 400 字符窗口，必须有 ErrorBoundary；自检（临时摘掉 ChatPanel 的兜底）报 ChatPanel @631 不在兜底内、EXIT=1，还原后全绿。
- 验证：pnpm -C apps/desktop check 68 files / 1019 tests / EXIT=0。

### R393: winapp2 导入解析完又改内容？旧预览不再冒充现值

- 缺口：Studio 的 winapp2 导入面板解析成功后预览一直挂在那儿——用户再往输入框加删内容，预览和「安装 N 条」按钮仍是按旧文本算的，选中的勾也不作数；解析出 0 条时预览区只剩一条空态说明，「解析预览」按钮却再不出现，没别的入口。
- 改动：新增导出纯函数 `winapp2Stage(ini, items, parsedIni)` → idle / ready / preview / stale 四阶段；组件记住解析时的文本快照，输入一变就进 stale——预览上方挂一条说明（旧预览按旧内容算），底部按钮从「安装 N 条」换成「重新解析」，不让旧选择误装。
- i18n：新增 `studio.winapp2Stale`（输入已改动…）与 `studio.winapp2Reparse`（重新解析 / Re-parse），zh/en 同步。
- 测试：新建 Studio.test.ts 5 条（idle/ready/preview/stale/0 条仍算 preview）。
- 验证：pnpm -C apps/desktop check 全绿。

### R392: CHANGELOG 结构修整——重复的 [未发布]、错位文件头、9 处标题前缺空行

- 缺陷：清单中段出现第二个 `## [未发布]`（R382 起整段被错误地再分一层）；文件头简介两行被挪到了中段；9 处 `###` 标题前没有空行。Markdown 渲染不报错，但结构索引、diff 和脚本解析都会踩。
- 修复：文件头简介移回第 1 行；删掉第二个 `## [未发布]`；9 处标题补空行。全文只保留一个 [未发布] 区，`## 较早版本` / `## 格式说明` 结构不变。
- 守卫：新增 `scripts/check-changelog.mjs`（零依赖 node ESM）——简介只在第 1 行出现一次、恰好一个 [未发布] 区、`###` 标题前必须有空行、标题文本不得重复、文件以单个换行结尾；`pnpm check:docs` 与 CI 的 frontend 文档步骤串上这道检查。
- 验证：node scripts/check-changelog.mjs 通过；node scripts/check-docs-links.mjs 30 条链接全过。

### R391: 字段级校验错误贴到出错的输入框上，读屏也能念到

- 缺口：MCP 服务器表单的名字/命令/URL 是三个字段级校验错误，但它们和「保存失败」「权限不足」一起挤在表单顶部的大横幅里——用户填错命令时，红色横幅离出错的输入框好几行远；读屏软件只会念到一句横幅，输入框本身不带「这一项有错」的标记（全仓 `aria-invalid` 此前为 0）。
- 改动：`McpServers.tsx` 新增导出 `McpFieldErr` 类型与纯函数 `mcpFieldError(name, transportType, command, url)`（保持原校验顺序：name → 当前传输方式的 command/url）；表单的字段错误改由 `fieldErr` 状态承载，文案贴在对应输入框正下方（`id="mcp-err-name|command|url"`），输入框加 `aria-invalid` 与 `aria-describedby` 指向该条文案，改动该字段或切换传输方式即清除；顶部横幅只留权限不足/保存失败这类整体错误。
- `Studio.tsx` 的 TOML 安装与 winapp2 导入两个 textarea 同理：`aria-invalid={!!err}`、`aria-describedby` 指向已有的错误块（补 `id="studio-toml-err"` / `id="studio-winapp2-err"`）。
- CSS：`layout.css` 新增 `.field-err`（与 `.install-err` 同款风险配色，内联小块）。
- 守卫：`a11y.test.ts` 新增 2 条——带 `aria-invalid` 的输入框必须同时带 `aria-describedby`；`aria-describedby="<字面 id>"` 指向的 id 必须在同文件存在。
- 测试：`McpServers.test.ts` 新增 6 条 `mcpFieldError` 单测（空白名字、stdio 缺命令、http 缺 url、交叉不误报、全填好为 null）。
- 验证：`pnpm -C apps/desktop check` 全绿。

### R390: 清理面板不再把「最近 50 条」说成「共 50 条」——碰上限就明说还有更早的

- 缺口：`UndoPanel` 只拉最近 50 条（`api.listUndo(50)`），但头部计数文案 `cleanup.entriesCount` 直接填 `entries.length`——台账超过 50 条时（R388 后上限 5000）面板会显示「50 条」，把截断后的列表当成了全部，和 R310 修过的台账页是同一族「数据不自曝截断」问题。
- 改动：`UndoPanel` 新增导出 `UNDO_PANEL_LIMIT = 50` 与纯函数 `undoCountInfo(entries, limit)`（复用 `historyTruncated` 的截断判定，碰上限保守视为截断）；头部计数改为 `truncated ? cleanup.entriesTruncated : cleanup.entriesCount`——截断时明说「显示最近 {n} 条（更早的在台账页）」，旁边的「查看全部」按钮天然对上了。
- i18n：cleanup.ts zh/en 各加 `cleanup.entriesTruncated`（含 {n}，与 entriesCount 占位符一致，过 R313 守卫）。
- 测试：新增 `components/UndoPanel.test.ts` 5 条（null/undefined→0 条不截断、不足上限如实报、恰好 50 保守截断、200 条如实报 n 且截断、limit=0 不算截断）。
- 验证：`pnpm -C apps/desktop check` 全绿。

### R389: 扫描失败的红横幅能手动关掉了——旧错误不再一直挂着

- 缺口：扫描失败时 App 顶部的错误横幅（`banner error`）只在「下一次扫描开始」时被 `setErr(null)` 清掉。用户不想立刻重扫时，旧错误会一直挂在页面顶上，既没有关闭入口，也没有任何说明（这个横幅不会被任何页面切换收起）。
- 改动：错误横幅改为横排（flex），左侧错误文本（`.banner-msg`，flex:1 自动换行），右侧加 X 关闭按钮——`onClick={() => setErr(null)}` 立刻收起，`aria-label` 复用 `common.close`（R319 键），`:focus-visible` 有可见描边（读屏用户照常收到 `role=alert` 播报，之后能用键盘把横幅收掉）。
- 守卫：`a11y.test.ts` 新增 describe「错误横幅要能手动关闭（R389）」2 条——App.tsx 的 `banner error` 块内必须含 `setErr(null)`、`banner-dismiss` 与关闭 `aria-label`；layout.css 必须含 `.banner-dismiss:focus-visible` 焦点样式（不许再退化成无入口的常驻错误）。
- 验证：`pnpm -C apps/desktop check` 全绿。

### R388: undo.jsonl 不再无限膨胀——台账固定保留最新 5000 条

- 缺口：撤销台账是纯追加（write_log 逐条 append），长期使用会撑到任意大；前端每次打开台账页，后端 `list_undo` 都是「读整个 jsonl 再 take」，文件越大每次打开越慢。而 R310 的「查看全部（最多 5000）」上限实际是前端自设的——台账里可能还有更早记录没进前端（HistoryPage 注释里曾让人以为后端自带裁剪，实为笔误，R384 已修注释）。
- 改动：`crates/executor` 新增 `UNDO_LOG_MAX = 5000` 与 `trim_log(undo_log, max)`——台账超过上限时原子重写（同目录临时文件 + rename，与 `remove_restored` 同一口径，崩溃不截半截）只留最新 N 条，返回裁掉的条数；不足上限或台账不存在零开销不动文件；坏行（无法反序列化）超限随重写丢弃（list_undo 本来就跳过它们）。`execute()` 每轮清理结束统一调用一次（只记 warn 日志，裁剪失败不阻塞清理）——五个执行入口（清理 scope/glob/AI 计划/工具箱/插件）自动受益。`remove_restored` 顺手抽出共享的 `rewrite_log` 原子重写。
- 前端：HistoryPage 注释更新——「查看全部」看到的就是台账里真实存在的全部记录（ALL_LIMIT 与 UNDO_LOG_MAX 同值），不再有隐性缺口。
- 测试：`crates/executor/tests/executor_ops.rs` 新增 4 条——超限裁到指定条数且最新在前、不足上限不重写不动文件（含不存在的台账 / max=0）、坏行随重写丢弃、execute 自动把超限台账收口到 UNDO_LOG_MAX（预置 5002 条再执行一次清理）。
- 验证：cargo test -p diskpilot-executor 27 passed；cargo clippy -p diskpilot-executor --all-targets -- -D warnings 干净；`pnpm -C apps/desktop check` 65 files / 998 tests 全绿。

### R387: Tauri 事件监听不再有取消竞态——迟到的解绑立即生效，三处监听收进 useTauriEvent

- **修复：Tauri 事件监听不再有取消竞态泄漏。** 全仓三处监听（硬件烤机进度 hw-test-progress、插件下载进度 plugin-download-progress、风扇熔断 fan-temp-breaker）是老写法 `let un; listen(..).then(fn => un = fn); return () => un?.()`——组件卸载或 effect 依赖重跑发生在 `listen()` 的 Promise settle 之前时，解绑函数还没拿到，监听器就留在那里：卸载后仍收事件（可能对已卸载组件 setState）、market 的 `[refresh, t]` 依赖重跑会叠出第二份回调。R328 在 `useAgentToolProgress` 修过一份，本轮收成可复用基建。
  - 新增 `apps/desktop/src/tauriEvent.ts`：`ListenerGuard<T>`（`attach()` 挂监听、`cancel()` 幂等解绑——迟到 settle 的解绑函数会被立即调用；cancel 后回调不再透传）+ `useTauriEvent<T>(event, handler, enabled)`（handler 永远取最新闭包，deps 只有 event/enabled）。
  - 三处迁移：`components/HwPanels.tsx`（isTauri）、`components/toolbelt/market.tsx`（isTauri，去掉原 [refresh, t] 叠挂隐患）、`components/SystemTools.tsx`（fanCtrlPerm && isTauri，删掉直接 import 的 listen）。
  - 新增 `apps/desktop/src/tauriEvent.test.ts` 5 条：正常 attach→cancel 解绑一次；cancel 先于 attach 完成→迟到解绑立即调；cancel 后回调不透传；重复 cancel 幂等（修掉首次实现 cancel 未清 unlisten 的缺陷）；未 attach 就 cancel 后再 attach 不残留。
  - ScanProgressBar（App.tsx）与 useAgentToolProgress 已各自安全，未动。

### R386: 市场页说「存哪」也给条路——社区插件说明旁加「打开数据目录」

- 缺口：R385 在设置页加了「打开数据目录」，但插件市场社区 tab 的说明仍只显示 `<code>app_data_dir</code>` 这个变量名 —— 用户正是从这一页看到「索引与下载包缓存在 app_data_dir」，却没有任何入口过去，还得回头翻设置页。
- 改动：① `reveal.ts` 新增 `openDataDirWithToast()`（取 appDataDir 后复用 `revealWithToast` 定位；路径为空或取路径失败都 toast 原因），设置页改为调用它（去掉 R385 的重复实现，`api.appDataDir` 调用只留一份）；② 市场社区 tab 说明下新增「打开数据目录」按钮（同样的失败回音口径，不静默）。
- i18n：settings.ts 加 `settings.general.dataDirMissing`（后端没返回路径时的原因文案），toolbelt.ts 加 `toolbelt.mkt.openDataDir`（zh/en）。
- 测试：`reveal.test.ts` 新增 describe「打开应用数据目录的回音（R386）」3 条 —— 成功定位不打扰、后端返回空路径时说明原因、取路径抛错时 toast 带原因。
- 验证：`pnpm -C apps/desktop check` 全绿。

### R385: 设置页加「打开数据目录」入口——备份/清理不再靠手敲路径

- 缺口：后端把撤销台账（undo.jsonl）、隔离区（quarantine）、提醒（reminder.json）、硬件与传感器历史、插件互查缓存全存应用数据目录（app_data_dir），但前端没有任何命令暴露这个路径；插件市场的「存哪看哪」提示只把 app_data_dir 字符串展示出来，用户想备份或手动清理数据得自己翻 %LOCALAPPDATA%。
- 改动：① 后端新增 `#[tauri::command] app_data_dir`（general_tray.rs，`app.path().app_data_dir().to_string_lossy()`），lib.rs 的 generate_handler 注册；② api/system-ext.ts 加 `appDataDir()`（isTauri ? invoke : Promise.resolve 兜底，同 generalConfig 族）；③ Settings 通用 tab「版本更新」组后新增「数据目录」组：组标题 + 说明（撤销台账、隔离区、提醒与硬件历史都存在这里）+「打开数据目录」按钮——拿到路径走 revealWithToast 在资源管理器定位，取不到路径或打开失败都弹 err toast（含原因），不再静默。
- i18n：settings.ts 加 settings.general.dataDir / dataDirDesc / dataDirOpen / dataDirFail 四键（zh/en）。
- 顺手修正：CHANGELOG 误入的一行孤立字符 g（R383 条目后）已删。
- 验证：pnpm -C apps/desktop check 64 files / 990 tests 全绿；后端 cargo check 通过。

### R384: 错误横幅能被读屏念出来——32 处错误文本补 role=alert + 源码守卫

- 缺口：`role="alert"` 此前全仓为零（R359 只给「成功提示」补了 `role="status"`，错误是反过来的缺失）。扫描失败（App 顶部 banner）、台账加载失败（HistoryPage/UndoPanel）、连接/测试失败（McpServers/Settings/PowerPlanCard）、散热与驱动控制失败（SystemTools/StartupPanel）、安装失败（Studio）、市场加载失败（market/services）、趋势读取失败（bench/HwPanels）、auto_list 规则检查失败（ReminderSettings）——全都只是红色小字，读屏软件不主动念，错误发生时用户可能完全没察觉。
- 改动：32 处错误文本容器补 `role="alert"`（断言式播报，出现即念）：App banner error、AssetOverview ao-diff-err / ao-clean-preview-note err、HistoryPage ×2、HwPanels ×3、McpServers ×4、PowerPlanCard、ReminderSettings ×3、Settings、StartupPanel、SystemTools ×4、Studio ×3、bench ×2（longNote 静态说明用 `role="note"` 区分，不按错误念）、UndoPanel ×2、market ×2、services、CleanupBody。
- 守卫：a11y.test.ts 新增 describe「错误横幅要能被读屏念出来（R384）」——扫 RAW_TSX，className 含 `err`/`error`/`-err` 词元的 div/p/span 必须带 `role="alert"`、`role="note"` 或 `aria-hidden`（自检：删任意一处即红）。
- 顺手修正：HistoryPage.tsx 头部注释「undo.jsonl 自带裁剪」与事实不符（后端是读整个 jsonl 再 take，台账没有裁剪；R310 的截断横幅才是真实口径）。
- 验证：`pnpm -C apps/desktop check` 64 files / 990 tests 全绿。

### R383: 扫描不再能并发跑两份——加单飞闸门，页脚「重扫」不再踢起第二个扫描

- 缺口：`scan` 没有任何并发守卫，而扫描入口有五处（总览盘卡片、盘条「扫描全部」、体检红盘格、救援横幅、页脚「重扫」）。前四处带 `disabled={scanning}`，唯独页脚重扫（R314 改成可聚焦的 `role="button"` span）没有——扫描中点它就会再起一份扫描：两份同时读盘、`scan-progress` 事件互相覆盖，先结束的那份 `finally` 把 `scanning` 置回 false，界面显示「未扫描」而后端还在遍历。后端 `scan.rs` 也没有「已有扫描在进行」的拦截，靠 UI 的 disabled 拦不住所有路径（R314 之后新增的入口都可能漏）。
- 改动：新增 `apps/desktop/src/scanGate.ts` —— `ScanGate`（`tryEnter()` / `leave()` / `get busy()`）单飞闸门，风格同 `flash.ts`、`escapeKey.ts` 这类小工具。`App.tsx` 里 `const scanGate = useRef(new ScanGate()).current`，在**缓存命中判定之后、`setScanning(true)` 之前**取闸：命中缓存的「秒开」路径不受影响（扫描期间点盘符仍能看已扫好的树），取不到闸就 `toast(t('shell.scan.busy'))` 并放弃本轮；`finally` 里 `scanGate.leave()` 与 `setScanning(false)` 并列，取消（`scan:cancelled:` 分支的 return）也走 `finally`，闸门必释放。
- i18n：`shell.ts` 加 `shell.scan.busy`（zh「扫描正在进行中，等这一轮扫完再试」/ en "A scan is already running — wait for it to finish"）。
- 测试：新增 `apps/desktop/src/scanGate.test.ts` 3 条（第一份取到、第二份被拒且 busy 保持；放闸后可再取；没取就放/多放一次不出错且不误放别人的闸）。
- 验证：`pnpm -C apps/desktop check` 64 files / 989 tests 全绿。

### R382: 快速切盘/重扫时迟到的旧响应不再盖住当前盘——异步加载加请求序号守卫

- 缺口：总览三条异步加载（建议清理项 loadClean、跨次环比 loadSuggDiff、dry-run 预览 togglePreview）都没有「迟到响应作废」逻辑。快速切盘或扫描完成重拉时，先发的请求可能后返回：旧盘的建议清理项/环比会写进当前盘，dry-run 预览同 key 跨盘串数据显示旧盘文件（R244 的 lastClean 台账也一起错）。
- 改动：loadClean/loadSuggDiff 各持请求序号 ref（++ 后写入前比对，过期即丢）；togglePreview 记录发起时的盘路径 reqPath，写入前核对 selDriveRef 镜像（切走后旧结果不落盘）。
- 守卫测试：新增 AssetOverview.race.test.ts 2 条源码静态断言（三条加载函数必须保留序号/镜像比对，删守卫就红）。
- 验证：pnpm check 63 files / 986 tests；无 i18n/样式/Rust 改动。

### R381: 删掉死命令 get_auto_list_config——注册了但前端零调用

- 命令面对账（正则支持嵌套泛型 invoke<Record<number,string>>('cmd')）：前端 116 个 invoke 全部后端注册、0 错拼；后端 117 个注册里唯一没被调的是 auto_list::get_auto_list_config。
- 前端全部走 get_auto_list_config_with_status（内部直接调 config_at(&app)，不经过该命令）；全仓 TS/RS 裸名残留 0。
- 删 auto_list.rs 的 #[tauri::command] fn 定义与 lib.rs generate_handler 注册行（留着会因 dead_code 挡 -D warnings）。
- cargo check/clippy --all-targets -D warnings 与 pnpm check 均通过。

## [0.2.3] - 2026-10-07

- AI 实事求是约束 + 守卫只读盘根放行 + 会话续跑。
- 并发会话遗留批量收尾（winapp2 预览 / Studio 重构 / McpServers 重排 /
  只读工具切 check_read / 样式 token）。
- 深色主题 5 主题全修复 + 布局/设置控件统一（见历史提交）。

## 历史逐轮记录

| 期间 | 条目数 | 卷 |
|---|---|---|
| 早期汇总（跨轮汇总） | 4 | [early-archive.md](docs/changelog/early-archive.md) |
| 2026-10-13 | 22 | [rounds-2026-10-13.md](docs/changelog/rounds-2026-10-13.md) |
| 2026-10-14 | 18 | [rounds-2026-10-14.md](docs/changelog/rounds-2026-10-14.md) |
| 2026-10-15 | 32 | [rounds-2026-10-15.md](docs/changelog/rounds-2026-10-15.md) |
| 2026-10-16 | 75 | [rounds-2026-10-16.md](docs/changelog/rounds-2026-10-16.md) |
| 2026-10-17 | 28 | [rounds-2026-10-17.md](docs/changelog/rounds-2026-10-17.md) |
| 2026-10-22 | 1 | [rounds-2026-10-22.md](docs/changelog/rounds-2026-10-22.md) |
| 2026-10-23 | 6 | [rounds-2026-10-23.md](docs/changelog/rounds-2026-10-23.md) |
| 2026-10-24 | 86 | [rounds-2026-10-24.md](docs/changelog/rounds-2026-10-24.md) |

## 格式说明

- `[未发布]`：当前工作区 HEAD 之后的累积变更（含已提交的 5 轮审查修复）。
- 严重度标注：`高/中/低` 遵循审查报告分级；修复均附回归测试。
