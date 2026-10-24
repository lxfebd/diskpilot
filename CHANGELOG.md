DiskPilot 变更日志。格式遵循 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，
版本号遵循 [SemVer](https://semver.org/lang/zh-CN/)。

逐轮开发记录（R1–R408，共 300 条）按日期分卷归档在
[docs/changelog/](docs/changelog/README.md)；本文件只保留当前未发布的
近期轮次（R381–R408）与已发布版本摘要。

## [未发布]

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
