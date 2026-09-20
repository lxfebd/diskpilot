# DiskPilot · 组件与 API 参考

> 前端 23 个组件 + 后端 Tauri 命令 + AI 工具的全景参考。
> 给前端开发者：改组件前先查这里，理解 Props 契约与数据来源。
> 依赖方向：**组件 → store → api → types**，禁止反向。组件不写业务逻辑，只渲染 + 交互。

## 一、前端组件清单（components/，23 个 .tsx + 1 测试）

### 布局 / 视图层

| 组件 | 职责 | Props / 数据来源 |
|---|---|---|
| `App.tsx`（根） | 三视图（overview/workspace/tools）组装 + AI 侧栏 + 扫描编排 | `view` state；`scan()` 唯一调用 `api.scan` 的地方；三栏可拖 Splitter 自适应 |
| `AssetOverview.tsx` | 「总览」页：磁盘卡墙 + 分类占用 + Top N 大目录 + 建议清理 | Props `{root, drives, scaffolds, scanning, onScanDrive, onScanAll, onRefresh, onGoWorkspace}`；挂载调 `cleanupSuggestions(path, undefined, true)`（cachedOnly） |
| `DriveStrip.tsx` | 顶部盘符条（总览/工作台共用） | Props `{drives, scanning, onScanAll, onScanDrive, onRefresh, selPath?, onSelect?}`；点卡 = 扫描/秒开 |
| `LeftPanel.tsx` | 工作台左栏容器：树 / treemap / 文件 tab | Props `{root, selectedPath, onSelect}` |
| `TreeView.tsx` | 虚拟滚动目录树（@tanstack/react-virtual） | Props `{root, selectedPath, onSelect}`；行含展开箭头/图标/大小/占比条/scaffold 标签 |
| `Treemap.tsx` | d3-hierarchy 矩形树图 | Props `{node, width, height, onSelect, selectedPath}` |
| `FileView.tsx` | 文件列表（搜索/排序/扩展名统计） | Props `{root, selectedPath, onSelect}` |
| `Studio.tsx` | 工作台右栏：scaffold 清理卡片 + 安装 + Steam + 撤销 | 无 props（读 store） |
| `Splitter.tsx` | 拖拽分隔条 | Props `{onDrag, onDoubleClick?}` |
| `ContextMenu.tsx` | 右键菜单 | 导出 `ContextMenuItem / ContextMenuState`；Props `{state, onClose}` |

### 清理 / 撤销

| 组件 | 职责 | 要点 |
|---|---|---|
| `CleanupModal.tsx` | 按 scope 勾选清理的确认弹窗 | Props `{scaffold, matches, onClose, onCleaned}`；dry-run 预览 + 两步确认 |
| `CleanupProposalDialog.tsx` | AI 清理清单确认弹窗 | 无 props（读 store.proposal）；危险项打字「确认」 |
| `UndoPanel.tsx` | 「最近清理」+ 撤销面板 | 无 props；读 undo.jsonl（api.listUndo） |

### AI 侧栏

| 组件 | 职责 | 要点 |
|---|---|---|
| `ChatPanel.tsx` | 全局 AI 侧栏对话（`.ai-rail`） | 无 props；拖拽上传元数据、agentChat 多轮、工具确认卡（`.cli-confirm-remember` 会话免确认）、新对话/切换/清空时 `clearSessionAuths()` |

### 设置 / 权限

| 组件 | 职责 | 要点 |
|---|---|---|
| `Settings.tsx` | 设置弹窗（外观 / AI / 通用 / 权限 / 系统工具 / MCP 六标签） | Props `{onClose, initialTab?}`；`SettingsTab='appearance'\|'ai'\|'general'\|'perms'\|'system'\|'mcp'`；通用页含「版本更新」区块（check_update） |
| `PermissionCenter.tsx` | 四级权限（L0-L3）开关 | 无 props；L0 恒开不可关、L3 灰显不可解锁（`isPermEnabled`） |
| `McpServers.tsx` | 设置 →「MCP」标签：用户 MCP 服务器管理 | 无 props；服务器列表（三态）+ enabled/writable 开关 + 测试（列出工具）+ 删除两步确认 + 添加/编辑表单（stdio 命令/args/env/cwd 或 http url/headers/timeout）；写操作需 `mcp.manage`（L2）权限 |

### 工具墙 / 硬件 / Steam

| 组件 | 职责 | 要点 |
|---|---|---|
| `Toolbelt.tsx` | 「工具墙」页：12 分类导航 + 工具卡网格 + 详情 modal + 硬件/网络/系统 tab + 插件市场 | Props `{drives, scanning, onScanDrive, onScanAll, onGoWorkspace, onOpenSettings?}`；双击启动（toolbelt_launch，740→UAC 提权）、详情卡「移除此工具」（toolbelt_recycle） |
| `HwPanels.tsx` | 硬件卡片合集 | 导出 `HwReportCard({info, health, loading})`、`StressConfirmPanel({t, onClose, onConfirm})`、`HwTestMonitor`、`useHwTestMonitor(onStop)` |
| `SteamInspector.tsx` | Steam 游戏清单三栏 | 无 props（读 api.listSteamGames） |
| `SteamInspectorModal.tsx` | 弹窗壳（Esc/背景关闭） | Props `{onClose}` |
| `SteamWorkshopModal.tsx` | Workshop 条目弹窗 | Props `{game, onClose}`；标题 localStorage 缓存 |

### 基础

| 组件 | 职责 | Props |
|---|---|---|
| `ErrorBoundary.tsx` | 类组件错误边界 | Props `{children, fallbackLabel?}`；破坏性操作组件必须被它包裹 |
| `ProgressButton.tsx` | 异步进度填充按钮（两步确认模式） | Props `{className?, disabled?, title?, estimatedCount, granularity?, mode?, onAction, idleContent, runningLabel?}` |
| `Logo.tsx` | SVG 标志（粉色垃圾桶） | Props `{size?}` |

## 二、数据来源规则

- **唯一后端入口**：`api.ts`（37+ invoke 包装，浏览器 mock 模式自动走 `mocks.ts`）。组件**不直接 `invoke`**。
- **全局状态**：`store.ts`（zustand，22 个 action）。组件用 `useStore((s) => s.xxx)` 选择性订阅。
- **主题**：`theme.ts`。组件消费 CSS 变量（语义 token），不读 localStorage。
- **权限**：`permissions.ts`。`isPermEnabled(id)` / `grantSessionAuth` / `hasSessionAuth` / `clearSessionAuths` / `permDef`。
- **类型镜像**：前后端共享 schema 在 `types.ts`（`Node` / `Scaffold` / `Scope` / `CleanupSuggestion` 等）；改 Rust 侧 `Scaffold` 字段必须同步。

## 三、后端 Tauri 命令（api.ts 包装名 → invoke 命令名）

```
扫描:        scan→scan_path_usn(回退scan_path) · cancelScan→cancel_scan · treeSubtree→tree_subtree
             estimateSize→estimate_size · volumeInfo→volume_info · listDrives→list_drives
scaffold:    listScaffolds→list_scaffolds · installScaffold→install_scaffold
             uninstallScaffold→uninstall_scaffold · scaffoldSource→scaffold_source
             scopeSizes→scope_sizes · scopeSizesBatch→scope_sizes_batch
             cleanupSuggestions→cleanup_suggestions(rootPath, scopeDays?, cachedOnly?)
             chatScanContext→chat_scan_context
执行:        executeScope→execute_scope(scaffoldId, scopeId, rootPath, dryRun, opts{olderThanDays?,wxidFilter?,envFilter?,confirmed?})
             executeAiPlan→execute_ai_plan · recyclePaths→recycle_paths(paths, reason, confirmed)
             listUndo→list_undo · undo→undo
AI:          advise→advise · setAdvisor→set_advisor · webSearch→web_search · aiProxy→ai_proxy
硬件:        hwInfo→hw_info · hwDiskHealth→hw_disk_health · hwRunTest→hw_run_test
             hwStopTest→hw_stop_test · hwReport→hw_report · powerPlan→power_plan
             hwHistory→get_hw_history（R7：硬件快照归档列表，只读 L0）
             compareHwSnapshots→compare_hw_snapshots(a, b)（R7：两次归档对比，只读不重采）
             runSystemProbe→run_system_probe · networkProbe→network_probe
             fanCurveAdvice→fan_curve_advice（只读建议）
风扇控制:    fanControl→fan_control(level, curve?, tempBreakerC?, confirmed) · fanStatus→fan_status
             fanCurveApply→fan_curve_apply(restoreDefault, confirmed)（L2，双校验 + 温度熔断回退）
Steam:       listSteamGames→list_steam_games · listSteamWorkshopItems→list_steam_workshop_items
             fetchWorkshopTitles→fetch_workshop_titles · openSteamUrl→open_steam_url
工具墙:      toolbeltStatus→toolbelt_status · toolbeltCatalog→toolbelt_catalog
             toolbeltUsage→toolbelt_usage · toolbeltRun→toolbelt_run(tool, argv, timeoutSecs, sessionGranted)
             toolbeltManifests→toolbelt_manifests · toolbeltLaunch→toolbelt_launch
             toolbeltRecycle→toolbelt_recycle(tool, confirmed) · setToolsRoot→set_tools_root
插件:        pluginInstall→plugin_install · pluginUninstall→plugin_uninstall · pluginMarket→plugin_market
             pluginActivate→plugin_activate · pluginDeactivate→plugin_deactivate · pluginExport→plugin_export
             pluginInstallUrl→plugin_install_url(url)（https + Ed25519 签名强制）
             pluginRegistryConfig→plugin_registry_config · pluginSetRegistryUrl→plugin_set_registry_url(url, confirmed)
             pluginRegistryRefresh→plugin_registry_refresh(confirmed)（远端拉取+信封校验+原子写，失败保留旧索引）
             pluginRegistryList→plugin_registry_list · pluginRegistrySearch→plugin_registry_search（读合并索引信封，含 verified）
             pluginRegistryVerify→plugin_registry_verify(id)（只读 L0：签名/字段完备性）
             pluginRegistryInstall→plugin_registry_install(id, version?, confirmed)（社区注册表 L2 plugin.manage，blob 缓存+台账）
             pluginRegistryUpdate→plugin_registry_update(id, confirmed)（版本比对+旧包入缓存+原子换新）
             pluginRegistryRollback→plugin_registry_rollback(id, confirmed)（台账旧 blob 恢复）
             pluginListInstalled→plugin_list_installed（只读 L0：安装台账）
MCP:         mcpListServers→mcp_list_servers（列表 + probe 缓存 5s TTL + 健康探测，只读 L0）
             mcpTestServer→mcp_test_server(id)（连接 + tools/list 探测，强刷缓存）
             mcpAddServer→mcp_add_server(payload, confirmed)（L2 mcp.manage 双校验，stdio/http 双桥接 + stdio 约束校验）
             mcpUpdateServer→mcp_update_server(id, payload, confirmed)（同上，permission_map 缺省保留原值）
             mcpRemoveServer→mcp_remove_server(id, confirmed)（只删配置）
             mcpListTools→mcp_list_tools（enabled 服务器工具合并清单，含 perm 级别）
             mcpCallTool→mcp_call_tool(server_id, name, arguments, confirmed)（permission_map：L0 放行 / L1 确认 / L2 确认+perm_grants / L3 永禁）
             mcpAuditTail→mcp_audit_tail(limit?)（只读：mcp-audit.jsonl 审计尾部）
配置:        generalConfig→general_config · setGeneral→set_general
空间历史:    getSpaceHistory→get_space_history（R6：{root → SpacePoint[]}，扫描后自动追加）
更新:        checkUpdate→check_update
杂项:        inspect→inspect_path · revealInExplorer→reveal_in_explorer · listCondaEnvs→list_conda_envs
```

## 四、store.ts 状态切片与 action

**状态**（12 项）：`root`（「全部磁盘」虚拟根）/ `scanCache`（LRU 6 盘，key=normKey 大写）/ `scanSeq` / `scaffolds` / `selectedPath` / `reclaimedBytes` / `chat` / `studioRequest` / `toasts` / `proposal` / `chatSessions` / `activeChatId`。

**关键 action**：

| action | 作用 |
|---|---|
| `setRoot` / `selectPath` | 根树 / 选中路径 |
| `cacheDrives` / `takeDrive` | 扫描树缓存写入（LRU 提 key + 逐出）；点盘秒开 |
| `recyclePaths(paths)` | 批量回收：逐个 `api.execute({action:'recycle'})` → 成功后 `pruneMany` 一次性剪枝 |
| `aiRecyclePaths(paths)` | AI 清单执行：整批 dryRun(true) → dryRun(false) → 失败回退逐项；门口 `isPermEnabled('cleanup.execute')` |
| `newChat` / `switchChat` / `deleteChat` | 聊天多会话（localStorage，20 会话 × 200 turns） |
| `setProposal` / `requestStudio` / `consumeStudio` | AI 清单 → 工作台联动 |

**规范**：组件不直接改 store 内部结构；用 action。新增状态先想清「该放 store 还是组件局部」。

## 五、AI 工具（advisor/tools.ts，24 个）

| 工具 | 说明 |
|---|---|
| `web_search` | 网页搜索（需 webEnabled 偏好） |
| `path_size` / `list_dir` | 路径大小 / 列目录 |
| `propose_cleanup_plan` | **生成待确认清理清单**（AI 唯一删除路径，走人工确认） |
| `get_disk_health` / `analyze_disk_health` | 磁盘健康 / 分析 |
| `get_cleanup_suggestions` | **可真算**的清理建议（主动查询可全盘 walk；区别于前端 cachedOnly） |
| `get_system_info` / `run_system_probe` | 系统信息 / 探针 |
| `get_tool_manifest` / `get_cli_tool_usage` | 工具 manifest / CLI 用法 |
| `run_cli_tool` | 执行 CLI 工具（中/高危需确认；`hasSessionAuth('cli.run')` 会话免确认） |
| `get_hardware_info` / `generate_hw_report` / `run_hardware_test` | 硬件信息 / 报告 / 压测（`adjust_power_plan` 另列） |
| `adjust_power_plan` | 电源计划调整（L2） |
| `get_fan_status` / `adjust_fan_curve` | 风扇状态（只读）/ 档位调速（L2 fan.control，`promptSessionConfirm` 逐次确认 + 温度熔断） |
| `list_plugins` / `install_plugin` / `uninstall_plugin` / `market_plugins` / `activate_plugin` / `export_plugin` | 插件市场管理 |
| 用户 MCP 服务器工具 | **动态**：`dynamicMcpTools`（`refreshMcpTools()` 拉 `mcp_list_tools` 重建）；`execTool` 查静态表失败再查它，`buildToolDefs` 合并 `[...toolRegistry, ...dynamicMcpTools]`；writable 服务器工具先 `promptSessionConfirm('mcp.manage')` 确认门；刷新时机 = agentChat 开始 + 设置 MCP 页保存后 |

**agentChat**（agent.ts）：OpenAI/Anthropic/Gemini 三协议多轮，`MAX_ROUNDS=6`；系统提示注入工具清单；**L3 工具 AI 不可调用**（manifest 拦截）。`chatHistory.ts`：会话上下文压缩（超预算摘要）。

## 六、样式接入点

组件样式三来源：`styles/layout.css`（布局/通用）、对应领域 css（overview/chat/steam/settings/toolwall）、组件内联 style 原子类（少）。新组件：先查 `tokens.css` 是否已有语义 token，不要新增硬编码色。

## 七、新增组件检查清单

1. Props 最小化：数据能来自 store 就订阅（`useStore` 选择器），不层层下传。
2. 破坏性操作：`<ErrorBoundary>` 包裹 + 两步确认（ProgressButton 或自绘变红再点）+ 默认 `recycle`。
3. 颜色用语义 token（`--surface`/`--text`/`--accent`/`--risk-*`），不写死 hex。
4. 中文文案（用户要求）。
5. 类型与 `types.ts`/`api.ts` 对齐；`tsc --noEmit` + `vitest` 通过。