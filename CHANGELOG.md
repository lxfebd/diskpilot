DiskPilot 变更日志。格式遵循 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，
版本号遵循 [SemVer](https://semver.org/lang/zh-CN/)。

逐轮开发记录（R1–R408，共 300 条）按日期分卷归档在
[docs/changelog/](docs/changelog/README.md)；本文件只保留当前未发布的
近期轮次（R381–R408）与已发布版本摘要。

## [未发布]

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
