# DiskPilot 功能全拆解地图（2026-10-01 产 · 2026-10-02 复核）

> 由 6 个只读 subagent 并行拆分 + 交叉验证产出。粒度 = 功能点级（前端到组件/文件，后端到工具/函数），
> 每个功能点标注状态与入口，末尾汇总全部发现的问题（按严重度）。**本文件只做盘点，改动以问题清单为准。**

## 目录

- A. 前端：磁盘扫描与清理（9 功能点）
- B. 前端：外壳 / 设置 / 系统工具 / 优化（8 功能点）
- C. 前端：AI 助手 / 工具墙 / 权限（A12 / B15 / C7 子项）
- D. 后端：scanner / executor / scaffold / toolbelt（15 功能点）
- E. 后端：agent-server MCP 工具（11 域，75 工具）
- F. 脚手架 / 打包 / 工程门禁（5 块）
- G. 问题总账（按严重度，附状态）
- H. 与记忆对比的差异点

---

## A. 前端：磁盘扫描与清理

| # | 功能点 | 入口 | 说明 | 状态 |
|---|--------|------|------|------|
| A1 | 磁盘扫描流程 + 进度条 | `App.tsx:295`、`api/scan.ts:17`、`store.ts` scanCache | USN 增量优先 → 失败回退全盘；按盘缓存树（LRU 6 盘）；进度条用 ref 隔离高频事件；scan-stats 只打日志；并行扫盘虚拟根 | ✅ 完成 |
| A2 | 总览页 + 磁盘条 | `AssetOverview.tsx`、`DriveStrip.tsx` | 一键扫描/逐盘/强制重扫、使用率环图、红盘救援横幅、分类占用榜、建议清理清单、重复文件扫描、空间趋势迷你曲线（R6）、Top10 目录 | ✅ 完成 |
| A3 | 清理页（P2a） | `CleanupPage.tsx`、`CleanupBody.tsx`、`cleanup/matches.ts`、`useCleanupStore.ts` | 左侧脚本列表（collectScaffoldCards）+ 右侧 scope 大小探测（300ms 防抖）+ 保留期调整；执行链 = 预备→dry-run 预览→再预备→真删；P2b preflight 预检 | ✅ 完成 |
| A4 | dry-run + 回收站隔离 | `useCleanupStore.ts:423/549`、`DryRunPreviewDialog.tsx`、`ProgressButton.tsx` | dry-run 只探测样本路径不落盘；真删强制 confirmed + 后端 confirm_gate 二次校验；一律 Recycle + undo.jsonl | ✅ 完成 |
| A5 | 目录树 / Treemap / 工作台 | `TreeView.tsx`、`Treemap.tsx`、`Splitter.tsx`、`FileView.tsx` | 树虚拟滚动 + 截断「+N」+ tree_subtree 按需展开 + 右键回收；Treemap d3 矩形树图前 300 项；三栏可拖 Splitter | ✅ 完成 |
| A6 | 文件粒度清理 + 建议 | `types.ts:130`、`api/clean.ts:63`、`AssetOverview.tsx:298` | recycle_granularity 后端执行；cleanup_suggestions 优先缓存秒回绝不 walk；SpaceTrendCard 迷你曲线 | ✅ 完成 |
| A7 | 历史页 + 撤销 | `history/HistoryPage.tsx`、`UndoPanel.tsx`、`history/model.ts` | undo.jsonl 台账（上限 500）、过滤/搜索/按天分组、隔离可恢复、index 双校验 | ✅ 完成 |
| A8 | 隐私清理页 | `App.tsx:529`、`CleanupPage.tsx:20` tagFilter | 复用清理页壳，只展示 tags=['privacy'] 脚本（windows-privacy + browser-privacy） | ✅ 完成 |
| A9 | AI 清理建议 + 提案确认 | `CleanupProposalDialog.tsx`、`store.ts:319`、`advisor/agent.ts:154` | AI 出清单（what/purpose/impact/risk 3 级）→ 逐项勾选 → 高危打字确认 → 批量 dry-run → 批量真删 | ✅ 完成 |
| A10 | 清理提醒（R3） | `App.tsx:271`、`ReminderSettings.tsx` | 后端算可清字节 emit 事件，前端只 toast 出清单绝不自动清理 | ✅ 完成 |

## B. 前端：外壳 / 设置 / 系统工具 / 优化

| # | 功能点 | 入口 | 说明 | 状态 |
|---|--------|------|------|------|
| B1 | 应用外壳 2.0 | `App.tsx:90-616`、`Sidebar.tsx`、`nav.ts` | 页面分发（7 视图）、侧栏折叠三态（偏好+断点）、分组导航、页头、layout token、token-guard 守卫 | ✅ 完成 |
| B2 | 设置页 | `Settings.tsx:100-740` | 外观（语言/主题 17 套/字号）、AI（BaseURL/Key/Model/供应商）、通用（联网/思考/并行/硬件加速/托盘/保留数）、权限中心、系统工具、更新（双轨）、插件市场、提醒 | ✅ 完成 |
| B3 | 权限中心 | `PermissionCenter.tsx`、`permissions.ts` | L0 恒开 / L1 默认开 / L2 默认关 / L3 永禁；sessionAuth 会话免确认；isPermEnabled | ✅ 完成 |
| B4 | 系统工具 | `SystemTools.tsx`、`StartupPanel.tsx` | 启动项（枚举/禁用 .disabled 备份/删除进回收站）、风扇（曲线建议 + L2 双确认 + 5s 轮询 + 熔断）、驱动（列表 + WU 查询） | ✅ 完成 |
| B5 | 优化页 + 电源计划 | `OptimizerPage.tsx`、`PowerPlanCard.tsx` | StartupPanel + PowerPlanCard 堆叠；power.plan L2 双确认；optimizer.css 单列可滚动骨架（本轮新补） | ✅ 完成 |
| B6 | i18n 国际化 | `i18n/core.ts`、`namespaces/`（18 个） | zh/en 两语言，en 缺→zh 回退，zh 缺→key；useSyncExternalStore 即时切换；coverage 守卫（键集一致/前缀/静态命中/迁移清单） | ✅ 完成 |
| B7 | 托盘常驻 | `App.tsx:252`、`Settings.tsx:306`、后端 `lib.rs:1084/1261` | close_to_tray 配置、关闭按钮隐藏到托盘、tray_sync 按语言重推菜单文案 | ✅ 完成 |
| B8 | 错误边界 + WebView2 心跳 | `ErrorBoundary.tsx`、`App.tsx:177` | 局部边界（overview/cleanup/history/workspace/privacy）+ 根级兜底；5s 心跳 + watchdog >12s reload 自愈 | ⚠️ 优化页未局部包裹（见 G-高1） |

## C. 前端：AI 助手 / 工具墙 / 权限（子项）

| # | 功能点 | 入口 | 说明 | 状态 |
|---|--------|------|------|------|
| C1 | 聊天面板 ChatPanel | `ChatPanel.tsx:71-821` | 多会话、输入、拖放、回收两步确认、CLI/压测/会话三确认卡、自动总览、Studio 直达 | ✅ 完成 |
| C2 | 会话气泡/思考折叠/计时 | `TurnRow/AdviceCard`、`TraceBlock`、`advisor/traceTimer.ts` | memo 只重渲受影响气泡；trace 渲染 + 运行计时徽章 | ✅ 完成 |
| C3 | 会话上下文 | `advisor/chatHistory.ts` | 事件源式、token 预算、老回合压摘要、roughTokens | ✅ 完成 |
| C4 | AI 提供方 | `advisor/provider.ts` | openai/anthropic/gemini/ollama 四协议、detectProvider、120s 超时、ai_proxy 绕 CORS | ✅ 完成 |
| C5 | 多轮 agent 循环 | `advisor/agent.ts` | 工具调用循环、不支持自动降级、性能上下文注入 | ⚠️ system prompt 带 `$` 前缀（见 G-高2） |
| C6 | 工具注册表 | `advisor/tools.ts` | 25 自定义 + 76 mcpTool + 动态 MCP；execTool 分发；待确认中间态 | ✅ 完成 |
| C7 | 三层防线（写确认） | `tools.ts:824` → `agent.rs WRITE_TOOLS` → `mcp.rs resolve_perm` → PathGuard | 前端 confirmPerm → 主进程白名单硬校验 → 后端守卫 | ✅ 完成 |
| C8 | 工具墙 Toolbelt | `Toolbelt.tsx`、`toolwall.tsx`、`catalog.tsx`、`services.tsx` | 12 分类、卡片/详情、CLI 视图、网络/系统信息面板、硬件聚合、搜索/最近/隐藏推广 | ✅ 完成 |
| C9 | GPU 甜甜圈 | `gpu-donut.tsx` | WebGL2 真满载、4-pass、帧差 FPS、90℃ 熔断、Esc 退出、资源释放 | ✅ 完成 |
| C10 | 基准测试面板 | `toolbelt/bench.tsx`、`charts.tsx` | bench_cpu/memory/disk/gpu、stress 打字确认 + 停止、甜甜圈、坏点、sensor_trend 折线、历史 | ✅ 完成 |
| C11 | 插件市场 | `toolbelt/market.tsx` | 内置插件化、URL 安装、社区注册表、Ed25519 校验、台账、更新回滚卸载 | ✅ 完成 |

## D. 后端：scanner / executor / scaffold / toolbelt

| # | 功能点 | 入口 | 说明 | 状态 |
|---|--------|------|------|------|
| D1 | 全盘并行 walk | `scanner/src/lib.rs:246/574` | jwalk + 独立 rayon 池、取消（5000 文件节流）、top-K 截断、ext/mtime 采集、concurrent 回归测试 | ✅ 完成 |
| D2 | MFT 快速扫描 | `scanner/src/mft.rs:183` | 直读 NTFS MFT（需管理员），catch_unwind 兜底 walkdir、空树守卫 | ✅ 完成 |
| D3 | USN 增量扫描 | `scanner/src/usn.rs:571/256/468` | FRN 链表、批量回放、merge_changes/remove_node 增量传播、cursor 有效性 | ⚠️ rename 目录子树问题（见 G-中1） |
| D4 | 进度/统计/取消 | `scanner/lib.rs:191`、`desktop/scan.rs:44/368/427` | ScanProgress/ScanStats、阶段计时、cancel flag、scan:cancelled: 前缀 | ✅ 完成 |
| D5 | 扫描树缓存/建议秒算 | `desktop/scan.rs:368`、`cleanup.rs:451` | 多槽 scan_tree 内存树 → suggestions_from_tree 秒算；cached_only 绝不 walk；tree_subtree 按需 | ✅ 完成 |
| D6 | 目录匹配原语 | `scanner/src/walker.rs:67/93/410` | find_matching_dirs 祖先去重 + 根守卫、tally_dir_scopes 单遍合并、树上匹配截断裁决 | ✅ 完成 |
| D7 | 执行引擎 | `executor/src/lib.rs:110` | Recycle/Quarantine/Delete + dry_run；逐项失败跳过；Delete TOCTOU 复检；undo.jsonl 追加 | ⚠️ dry-run 不返回字节（见 G-中3） |
| D8 | 回收站 + 兜底 | `executor/src/lib.rs:49` | trash::delete → SHFileOperationW 兜底；Win11 误报 workaround | ✅ 完成 |
| D9 | 保护路径红线 | `executor/src/lib.rs:241` | canonicalize 解析 symlink；盘根/系统目录/主目录 fail-closed | ✅ 完成 |
| D10 | 隔离/撤销 | `executor/src/lib.rs:170/387/425` | Quarantine 命名防撞、restore 越界校验、remove_restored 原子替换 | ✅ 完成 |
| D11 | desktop 清理接线 | `desktop/executor.rs:26/40/438/524` | confirm_gate 硬门、scope_glob_roots 推导、目录粒度强制 Recycle、AI 一律回收站 + 令牌注入 | ⚠️ 目录粒度强制 Recycle 无 lint 提示（见 G-低2） |
| D12 | Scaffold 解析/检测 | `scaffold/src/lib.rs:8/130/157/430/496` | TOML 字段全、load_dir 单文件失败不炸、detect_compiled 内存 children | ✅ 完成 |
| D13 | 红线校验 | `scaffold/src/lib.rs:327/261/398` | 锚定 C:/Users/test 环境、样本反查、lint/gen/运行时共用一把尺 | ✅ 完成（边界见 G-中2） |
| D14 | Winapp2 转换 | `scaffold/src/winapp2.rs:98/310` | FileKey/RegKey/ExcludeKey/Detect 解析、只转 FileKey、坏行不炸 | ✅ 完成（ExcludeKey 不参与 glob 已知限制） |
| D15 | 工具墙后端 | `toolbelt/src/lib.rs:185/305/808/923/996` | 嵌入目录解析、Tools 根定位（C~Z 探测）、CREATE_NO_WINDOW、GBK 兜底、os error 740 提权提示 | ✅ 完成 |
| D16 | 插件签名 | `toolbelt/src/signature.rs` | sha256 + Ed25519、字典序拼接、URL 强制签名 | ✅ 完成 |
| D17 | fancmd 桥 | `agent-server/src/tools/selfheal.rs` | 三级探测、CREATE_NO_WINDOW、15s 超时、写操作全要管理员 | ✅ 完成 |

## E. 后端：agent-server MCP 工具（75 个，11 域）

| 域 | 工具（agent-server 内） | 状态 | 质量 |
|----|------------------------|------|------|
| 磁盘/存储 | disk_health, disk_partition_usage, disk_volume_meta, file_type_stats, file_tree, disk_top_directories, disk_find_biggest_files, disk_find_duplicate_files, disk_io_usage, cleanup_suggestions | ✅ | 有 20 万文件/20 层上限；guard_canonical 防 C:/./ 绕过；truncated 语义合并含糊（低） |
| 硬件 | hw_cpu/gpu/memory/motherboard/temperature/superio/sensors/battery/disk_smart, disk_smart_raw_attributes, usb_devices, hw_cpu_features, hw_dram_timings, hw_displays, hw_ipmi, hw_acpi, bench_gpu | ✅ | 降级链完整（HWiNFO→SuperIO→fancmd→ACPI）；fancmd 管道死锁已修；**superio inp/outp expect 是全仓唯一 panic 出口（高1）** |
| 网络 | net_status/connections/speed/share/wifi/adapter_detail | ✅ | MIB2 原生 + PS 兜底；虚拟网卡过滤；net_wifi 不读密码 |
| 系统 | system_info, sys_services, sys_drivers, sys_boot_items, service_control(L2), scheduled_task_manage(L2), env_vars, recycle_bin_stats, app_list, app_licenses, uninstall_app(L2), list_processes, process_info, process_kill(L2), process_start(L2), process_cpu_usage, sys_defender_status, sys_user_accounts | ✅ | 最完善域；kill 三守卫、start 白名单 8.3 短名展开、uninstall 双白名单 |
| 安全审计 | security_event_logs, security_firewall_rules, security_login_events | ✅ | 日志名白名单；防火墙只读元数据；权限降级中文文案 |
| 盘点建议 | steam_games | ✅ | 复用 steam-inspector crate；幽灵安装规则 |
| AI 自修复 | fan_selfheal_diag, fan_selfheal_fix(L2), fan_control(L2) | ✅ | install_driver 绝不覆盖已有 DLL；run_fancmd 同步读管道（中4） |
| 基准压测 | bench_cpu/memory/disk/gpu, stress_test(L2), stress_test_gpu(L1), stress_cancel, mem_test, bsod_analyze | ✅ | stress 卡住已修（后台线程采样）；跨进程停止文件；detach 不 join（低） |
| 报告趋势 | system_report, sensor_trend, sensor_alert | ✅ | system_report 120s 超时已修；sensor_alert 文本抠数字易碎（低） |
| 工具箱 | toolbelt_list, toolbelt_run(L2) | ⚠️ | **toolbelt_run 无条件 require_confirmed 与描述矛盾（中3）** |
| MCP 管理（主进程） | mcp_list_servers/test/add/update/remove/list_tools/call_tool/audit_tail | ✅ | resolve_perm 五级映射、审计日志、HTTP 强制 https、stdio 拒绝穿越 |

## F. 脚手架 / 打包 / 工程门禁

| # | 功能点 | 入口 | 说明 | 状态 |
|---|--------|------|------|------|
| F1 | 清理脚本库 | `scaffolds/*.toml`（21 个） | 全 cache/privacy 分类、risk 分级、2 个 privacy tag；红线零命中 | ✅ 完成 |
| F2 | 打包配置 | `tauri.conf.json`、`build.rs`、`prepare-sidecar.mjs` | externalBin sidecar、NSIS+MSI、createUpdaterArtifacts、MSI locale | ⚠️ locale-zh-CN.wxl Culture=en-us + Codepage 936 组合错（见 G-中5） |
| F3 | CI 流水线 | `ci.yml` / `release.yml` / `codeql.yml` / `pages.yml` | 三平台 lint+test、audit、scaffold-lint、frontend check+build、四矩阵 release | ✅ 完成（--frozen-lockfile=false 弱于标准，低） |
| F4 | 版本三处单源 | `version.ts` / `package.json` / `Cargo.toml` = 0.2.2 | version.test.ts 守卫一致 | ✅ 完成 |
| F5 | 冒烟脚本 | `scripts/mcp_smoke_newline.py`、`make-icon.ps1` | MCP newline-JSON 冒烟 + 图标生成 | ⚠️ mcp_smoke 默认路径写死 J:\DiskPilot（低） |

## G. 问题总账（2026-10-02 复核：全量逐项对代码，标 ✅已修 / 🔴真欠 / ⚪保留）

> 复核方法：每项 grep 到具体代码位确认。已修项保留记录（含提交），真欠项即为当前待办。
> 下次改动前先看这里，避免重复搜代码。

### 高
| # | 问题 | 状态（2026-10-02） |
|---|------|------|
| G-高1 | 优化页未局部包裹 ErrorBoundary，运行时错误会整窗白屏 | ✅ 已修（`App.tsx:564` 已有 `<ErrorBoundary fallbackLabel={t('shell.boundary.optimizerFailed')}>`） |
| G-高2 | agent.ts system prompt 模板串 `$${baseSystem}` 多一个 `$` 字符污染模型 | ✅ 已修（`agent.ts:135/159` 现为 `baseSystem` 普通拼接，无 `$$`） |
| G-高3 | 总览页执行清理不带 wxidFilter，微信多账号一键清理会清所有账号 | 🔴 真欠 → 修复方式：总览无 wxid 选择器，勾选微信项时**引导去清理页**（`AssetOverview.tsx` doClean 拦截 + `hasCheckedWechat` 纯函数，2026-10-02 修） |
| G-高4 | Store 持久化不订阅 chat.turns，对话后直接关窗整段对话丢失 | ✅ 已修（`store.ts` archiveActiveChat + `App.tsx:135` beforeunload 归档） |

### 中
| # | 问题 | 状态（2026-10-02） |
|---|------|------|
| G-中1 | USN 增量扫描目录 rename 子树不搬移，增量与全量不一致 | ✅ 已修（`scanner/src/usn.rs` `try_rename_move` + `rebase_subtree_paths`） |
| G-中2 | 红线反查锚定边界未写明 | ✅ 已修（`scaffold/src/lib.rs:238-252` 注释已写明锚定与环境变量映射） |
| G-中3 | executor dry_run 不返回 per-path 字节 | ✅ 已修（`executor/src/lib.rs:129-137` 补 `path_bytes_before`） |
| G-中4 | selfheal run_fancmd 同步读管道有死锁隐患 | ✅ 已修（`selfheal.rs:94-106` 后台线程先读 stdout，输出膨胀不会与等退出互锁） |
| G-中5 | MSI locale Culture + TauriLanguage 组合错 | ✅ 已修（`locale-zh-CN.wxl` 现为 Culture="zh-cn" + TauriLanguage=2052） |
| G-中6 | toolbelt_run 无条件 require_confirmed 与描述矛盾 | ✅ 已修（`agent-server/src/tools/mod.rs:1242` 按 manifest risk ≥ Medium 才过门） |
| G-中7 | settings 通用 tab 硬编码 "AI" 未走 i18n | ✅ 已修（`Settings.tsx:584` `t('settings.general.ai')`） |
| G-中8 | PermissionCenter comingSoon 死分支 | ✅ 已修（无 comingSoon 命中，权限 desc 已清标记） |
| G-中9 | 三个新文件未登记 MIGRATED_FILES | ✅ 已修（`migrated-files.ts:36/39/45` 已登记 OptimizerPage/PowerPlanCard/StartupPanel） |
| G-中10 | general_config 前端类型缺 tools_root | ✅ 已修（`api/system-ext.ts:468` ToolbeltStatus 含 tools_root；设置页入口未做，待产品决策） |
| G-中11 | mocks.ts 注释「17 份」过期 | ✅ 已修（`mocks.ts:154` 注释已改「21 份」） |
| G-中12 | aiRecyclePaths dry-run 空返回仍真删 | ✅ 已修（`store.ts:391-416` 先 dry-run 探测，空则逐项再试、仍空计 failed 不真删） |
| G-中13 | runRealDelete 用最新口径而非预览快照 | ✅ 已修（`DryRunPreview` 补 `matchesPaths`/`isCondaSnapshot`，真删一律用预览快照路径与 conda 判定，换盘/重扫不再串路） |
| G-中14 | conda dry-run 字节口径与普通脚本不一致 | ✅ 已修（`model.ts` 新增 `condaDryBytes`：只按 dry-run 实际命中的 env 目录对回 size_bytes，不再把勾选但未过期的 env 全算进虚高） |
| G-中15 | TOOL_LABEL 只 58 条，trace 直显英文名 | ✅ 已修（`agent.ts:98` 有 `prettifyToolName` 兜底回退友好名） |
| G-中16 | cliIndexPromise 装插件后旧索引永久生效 | ✅ 已修（`tools.ts:1343` `invalidateToolbeltCache`） |

### 低
| # | 问题 | 状态（2026-10-02） |
|---|------|------|
| G-低1 | useCleanupStore.open() 死代码 | ✅ 已修（无调用方，接口+实现已删） |
| G-低2 | 目录粒度强制 Recycle 但 lint 不告警 | ✅ 已修（`scaffold` 新增 `directory_scope_recycle_violations` 共用规则 + scaffold-lint 规则 4 + 5 单测；34 份真实 TOML 全量零误伤） |
| G-低3 | Treemap 注释「d3 树图」实为矩形树图 | ✅ 已修（`Treemap.tsx` 已是 treemapSquarify） |
| G-低4 | App setRootMs 恒 0 诊断失真 | ✅ 已修（模块级自增计数替代随机，见 `AssetOverview.tsx:206`） |
| G-低5 | FileView MAX=60000 生产同样生效无提示 | ✅ 已修（`FileView.tsx:149` truncated 时显示「文件过多被截断」提示） |
| G-低6 | SpaceTrendCard 渐变 id 随机多行共享 | ✅ 已修（`AssetOverview.tsx:206` 自增计数 id） |
| G-低7 | App 重复注释行 | ✅ 已修 |
| G-低8 | PowerPlanCard toast 直显 GUID | ✅ 已修（`PowerPlanCard.tsx:22` 标准 GUID→可读名映射） |
| G-低9 | 硬件加速/托盘开关后端写失败静默 | ✅ 已修（`Settings.tsx:312` backendWrite + 失败提示） |
| G-低10 | updatePhase 复用字符串存错误被吞 | ✅ 已修（`Settings.tsx:131-132` 拆字段） |
| G-低11 | nav 宽度常量与 tokens 无守卫 | ✅ 已修（`nav.test.ts:105-120` 以 ?raw 读 tokens.css 钉死展开/折叠宽度常量） |
| G-低12 | optimizer.css padding fallback 不一致 | ✅ 已修（`.optimizer` 用 `var(--layout-page-padding-inline)`） |
| G-低13 | stress 记住被丢弃 | ✅ 已修（`ChatPanel.tsx:529` confirmStress 处理 remember） |
| G-低14 | mcp_smoke_newline.py 路径写死 | ✅ 已修（`scripts/mcp_smoke_newline.py:13` `_REPO_ROOT` 相对路径） |
| G-低15 | ci.yml --frozen-lockfile=false | ✅ 已修（`ci.yml:109` `pnpm install --frozen-lockfile`） |
| G-低16 | superio.rs inp/outp expect 建议改 debug_assert | ✅ 已修（`superio.rs:205/219` 已改 `debug_assert!` 兜底） |
| G-低17 | sensor_trend/alert L0 却写自有数据文件 | ⚪ 保留（文档已声明，设计如此） |

## H. 与记忆的差异点（重要）

1. **G 账本已全量复核（2026-10-02 第二次）**：4 高 4 修 0 欠、16 中 16 修 0 欠、低 17 项 14 修 3 保留。上次复核误标的 8 项（G-中4/11/12、G-低5/11/14/15/16）经代码逐行核对确认 P4 已修，本次实修 4 项（G-中13 快照口径、G-中14 conda 字节、G-低1 死代码、G-低2 lint 规则）。真欠项全部清零，仅 G-低17 ⚪ 保留设计。
2. **mocks.ts 与 TOML 对齐**：记忆里「幽灵 scaffold 整肃 17/17 对齐」，现在实际是 21 个顶层 id 全对齐（新增了 browser-privacy/windows-privacy/windows-update 三个），mocks.ts 注释已同步为「21 份」。
3. **agent-server clippy 存量噪音已清零（2026-10-02）**：原 45 条（agent-server）+ 5 条（toolbelt）+ desktop/scanner 若干，共 8 个 crate 的冗余闭包、match→if-let 折叠、`b"...\0"`→`c"..."` FFI、type alias 抽出、doc 列表缩进、`flatten()`→`map_while(Result::ok)` 等全部修复；`cargo clippy --workspace --all-targets` 现为 0 警告（仅剩 binrw v0.11.3 未来兼容提示，第三方依赖）。
4. 记忆「75 工具」与本次核对一致（mod.rs 75 个 async fn）；`mcp_status`/`disk_space_trend` 不在 agent-server，在 Tauri 主进程（mcp.rs / space_history.rs）。

---

*拆分方式：6 个 Explore subagent 并行（前端清理 / 外壳设置 / AI工具墙 / 后端 crates / agent-server / 脚手架工程），关键争议点（$ bug、mocks 对齐、advice 死代码、toolbelt_run 确认门）已二次核实。*
