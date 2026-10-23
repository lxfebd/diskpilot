# Changelog

DiskPilot 变更日志。格式遵循 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，
版本号遵循 [SemVer](https://semver.org/lang/zh-CN/)。

## [未发布]

### 2026-10-24 报表时间不再写死中文地区（R324）

Steam 盘点页导出报告的那行「生成于 …」用的是 `new Date().toLocaleString('zh-CN')`：
英文界面下也按中文地区口径渲染，而且格式随运行时地区变，落进 markdown 里不好排序。

- `format.ts` 新增 `formatDateTime(d, withSeconds = false)` → 地区中立的 `YYYY-MM-DD HH:MM`，
  与 `formatDateKey` 同口径（无效日期返回空串，不打印 NaN）；Steam 报表改用它；
- `format.test.ts` 新增 4 条单测 + 一条守卫：源码里不许出现 `toLocale*('xx-XX')` 这种
  带字面量地区的调用（`.test.` 文件除外），避免以后又写死；
### 2026-10-24 聊天会话列表能用键盘切换了（R323）

侧栏的会话列表每行是纯 `div` 点击：Tab 跳不过去，回车没反应，键盘用户没法切会话；
行里还嵌着一个删除 `<button>`，直接给容器加 `role="button"` 会变成按钮嵌按钮。

- 行结构拆成「图标+标题的真 `<button class="chat-session-main">` + 同级的删除 `<button>`」，
  行本身退回非交互容器；删除按钮不再需要 `stopPropagation`；
- `chat.css`：把整行的内边距挪到 `.chat-session-main`（点行内任意位置仍然能切会话），
  补 `:focus-visible` 描边，并让 `:focus-within` 也把删除 X 显形（否则 Tab 到一个看不见的按钮）；
### 2026-10-24 扩展名过滤行能用键盘选；盘卡片不再嵌一个假按钮（R322）

两处无障碍欠债：文件视图顶部的「按扩展名过滤」行是纯 div 点击，Tab 跳不过去、回车没反应；
盘卡片里那个悬停才出现的「单独重扫」是嵌在整卡 `<button>` 里的 `role="button" tabIndex={-1}` span——
HTML 里按钮不能嵌按钮，读屏软件还会念出一个永远无法聚焦的按钮。

- `FileView.tsx` 的 `.fv-ext-row` 加 `role="button" tabIndex={0}`、`aria-pressed`（它就是开关语义）
  与 `onKeyDown={activateOnKey(…)}`，与 R314 的树行/文件行同一套工具函数；
- `settings.css` 补 `.fv-ext-row:focus-visible` 描边，键盘用户看得见焦点；
- `DriveStrip.tsx` 的 `.ao-strip-refresh` 去掉 `role`/`tabIndex`，改 `aria-hidden="true"`
  （仍是鼠标悬停快捷方式，键盘用整卡按钮扫描），并在源码里写明原因；
### 2026-10-24 面板崩了能一键把现场交出去（R321）

`ErrorBoundary` 的兜底界面只会显示报错信息和一大段堆栈：用户想反馈，只能手抄或截图；
读屏软件也得不到任何提示——它是把 `children` 换成了纯 div，没有 live region。

- 兜底容器加 `role="alert"`，崩掉的瞬间就播报；
- 新增「复制错误信息」按钮：把「面板名 + 报错消息 + 组件栈 + 堆栈」拼成一段文本，
  走 `copyPathWithToast` 复制并 toast「错误信息已复制」——为此给该函数加了可选的
  成功文案键参数（默认仍是「已复制路径」，原有 3 处调用不变）。

### 2026-10-24 三个弹窗支持 Esc 取消（R320）

R315 给通用 `ConfirmDialog` 与烤机确认加了 Esc，但另有三个自家手写的 `modal-bg` 弹窗漏掉了：
清理提案对话框、插件安装对话框、插件市场的两步确认框。它们的关闭方式只有点遮罩或右上角 X。

- 三处都改为 `useEscapeKey(…)`（enabled 与弹窗可见性绑定，没开就不吃键）：
  清理提案 `CleanupProposalDialog`、安装脚本 `Studio` 的 `InstallScaffoldModal`、
  `toolbelt/market.tsx` 的 `confirmAsk`；
- 设置面板与 Studio 主面板暂不改：它们内部还会渲染子弹窗（市场确认框、卸载确认框等），
  Esc 会同时打到父子两层——要做得先给 hook 加「子层优先」，不在这次范围内。

### 2026-10-24 toast 能被读屏软件念出来了（R319）

toast 是这个应用的主要反馈通道（清理完成、打不开文件夹、提醒、扫描失效……），但那个常驻的
`toast-stack` 容器上没有 `role` / `aria-live`，屏幕阅读器对所有提示一律静默；顺带两处
`aria-label` 还写死了英文。

- `toast-stack` 改为 `role="status" aria-live="polite"`（容器常驻，内容一变就播报）；
- `aria-label="close"` → `aria-label={t('common.close')}`，聊天加载圈 `aria-label="loading"`
  → `role="status"` + `aria-label={t('common.loading')}`；`common` 表补 `common.close` /
  `common.loading` 两键；
- `a11y.test.ts` 加守卫：源码里不许出现字面量 `aria-label="…"`，必须走文案表。

### 2026-10-24 纯图标按钮补上可访问名字（R318）

读屏软件读到的是一串空按钮：全仓有 7 个「只有图标、既没有 aria-label 也没有 title」的按钮，
都是对话框/胶囊上的关闭与移除 X。同族的 `CleanupProposalDialog`、`HwPanels` 早就写了
`title={t('cleanup.close')} aria-label={t('cleanup.close')}`，这几处漏了。

- 补齐 7 处：清理预演对话框、设置面板、插件市场确认框、工具墙导出/回收确认框各一个关闭 X，
  以及聊天待发项胶囊上的文件/图片移除 X（新增 `chat.removePending`）。
- `a11y.test.ts` 加一条守卫：用 `import.meta.glob('?raw')` 扫全部组件源码，
  单图标且没有 aria-label / title 的 button 直接失败——以后新增按钮不会再悄悄少了名字。

### 2026-10-24 打开所在文件夹失败时给一句原因（R317）

上一版的「复制路径」补了回音，同族问题还在：四处「在资源管理器里打开」（扫描树 / 文件视图 / Studio 卡片 /
Steam 详情页的重叠文件行）写的是 `api.revealInExplorer(p).catch(() => {})`，
或者干脆 `void api.revealInExplorer(...)`——失败被吞掉，最后一种连未处理的 Promise 拒绝都算上。
而「路径已经不存在」恰恰是这个按钮最常见的失败，用户点了没反应只会以为软件卡了。

- 新增 `src/reveal.ts` 的 `revealWithToast(path)`：成功不打扰（资源管理器窗口自己会跳出来），
  失败 toast「打不开所在文件夹：{原因}」；返回是否成功。配 2 条单测。
- 五处调用统一走它：`tooltree.tsx`、`FileView.tsx`、`Studio.tsx`、`SteamInspector.tsx`
  （顺带修掉那处未处理拒绝）、`toolbelt/services.tsx`（原本自己 catch 的也并进来，文案口径一致）。
- `common` 表补 `common.revealFailed`；顺手清掉三处因此不再使用的 import。

### 2026-10-24 复制路径有回音了（R316）

三处右键菜单的「复制路径」（扫描树 / 工具树 / Studio 卡片）此前写的是
`navigator.clipboard?.writeText(p).catch(() => {})`：成功没有任何提示；环境里没有 clipboard API
时可选链直接短路，连 catch 都不进——用户以为复制了，其实什么也没发生。Steam 安装路径与跑分报告
早就有 toast 回音，这次把口径补齐并收敛成一处：

- 新增 `src/clipboard.ts` 的 `copyPathWithToast(text)`：复制成功 toast「已复制路径」，
  失败 toast「复制失败：{原因}」（不再静默吞 catch），返回是否成功。配 3 条单测
  （成功 / 剪贴板拒绝 / 环境无 clipboard API）。
- 三处菜单改调该函数；`common` 表补 `common.copiedPath` / `common.copyFailed` 两键。

### 2026-10-24 破坏性确认与压测确认都能 Esc 取消（R315）

`ConfirmDialog`（回收 / 删除 / 卸载的两步确认，全仓 5 处调用）与压测的 `StressConfirmPanel`
此前只能点遮罩或按钮关窗：键盘用户按 Esc 没反应，而同一套 Esc 关窗口径 `useEscapeKey`
早已在抽屉、工具详情、预览对话框里落地（R141 收敛的）。本条补齐这两处：

- `ConfirmDialog` 加 `useEscapeKey(onCancel)`——Esc 走取消（破坏性确认里取消永远是安全方向），
  同时把挂载后的焦点先放到「取消」按钮，避免一路 Tab / 回车误触确认。
- `StressConfirmPanel` 加 `useEscapeKey(onClose)`——Esc 与点遮罩、点右上角 X 同义，压测不会因此启动。
- 弹窗内有输入框也照常（Esc 仍是关窗），不额外拦键。

### 2026-10-24 列表行能用键盘选了（R314）

此前全仓只有侧栏写了 `:focus-visible`，列表行（树视图行、文件列表行、空间大头行、页脚重扫）
都是纯鼠标：Tab 跳过、Enter 没反应，这些地方对键盘用户等于不存在。本条把「按什么键等于点一下」
收敛成一处口径并接线：

- 新增 `src/a11y.ts`：`isActivationKey(key)`（Enter / 空格，对齐原生 button）、
  `activateOnKey(handler)`（非激活键直接放行；激活键先 `preventDefault` 再执行——空格不再滚页面）。
  事件只取最小形状 `KeyLike { key, preventDefault? }`，不依赖 React 类型，可纯函数单测；配 4 条单测。
- 接线 4 处：TreeView 树行 `role="button"` + `tabIndex={0}`，Enter/空格选中、←/→ 收起/展开；
  FileView 文件行、总览「空间大头」行、页脚「重新扫描」同为 `role="button"` + Enter/空格触发。
- 三份样式各补 `:focus-visible` 描边（`2px solid var(--accent-strong)`，与侧栏同口径），键盘焦点看得见。
- 未动 `DriveStrip` 卡片内的刷新热区：它嵌在真 `<button>` 里，键盘用户用外层卡片按钮即可扫描，
  避免可交互元素嵌套。

### 2026-10-24 文案层补一道占位符守卫 + 清掉无人使用的键（R313）

i18n 守卫测试此前只保证 zh / en「键集一致」，不保证同一键两边都用得上参数——
zh 写了 {n}、en 改词时忘了 {n}，英文界面会静默丢掉那个数字（反过来会漏出裸 {n}）。
本条补齐这道闸，并顺手清掉一个无人使用的键：

- `coverage.test.ts` 新增用例「同一键的 zh / en 占位符集合一致」：逐键比对 `{…}` 参数集合，
  对不齐直接红。全表现扫无漂移（1979 键），纯闸门、零行为变化。
- 删除从未被任何组件引用的 `cleanup.title`（zh「清理 · {name}」/ en 'Clean · {name}'，含 {name}
  参数、定义两年无人调），动静态扫描都确认无引用后再删；静态 `t()` 命中守卫会兜底。

### 2026-10-24 空间图说出自己不是全量（R312）

空间图（矩形树图）只布局根的前 300 个子项，后端对过深目录还会折叠（`children_truncated`），
但图上没有任何说明——用户容易把「图里没有」当成「盘里没有」。本条在图上落一行脚注：

- 新增导出纯函数 `treeIsTruncated(node, maxChildren = 300)`：根的子项数超上限、或任意层
  `children_truncated > 0` 即判为截断（显式栈迭代，不递归爆栈）。配 5 条单测。
- 判为截断时在 svg 底部画一行「只画前 300 项、折叠目录未展开 · 完整清单见树视图」，
  字号/颜色与既有尺寸标签同档（10px、rgba(22,12,18,0.75)），`pointerEvents:'none'` 不挡点击。
- i18n：`overview.treemap.folded`（zh/en 各一条）。

### 2026-10-24 Steam 卸载残留清单不再悄悄只画前 12 条（R311）

残留卡头部写的是全量「N 处 · 大小」，表格却硬编码 `residueItems.slice(0, 12)`——
超过 12 处时用户只看见 12 行，剩下的既列不出来也没有任何说明，头部数字和可见行数
对不上还找不到差额。本条把窗口显式化：

- 新增导出常量 `RESIDUE_CAP = 12` 与纯函数 `residueWindow(items, showAll, cap)`：
  折叠时给前 cap 条 + 剩余条数，展开时全给、剩余 0；cap 为负按 0（不越界）。配 5 条单测。
- 列表尾在 `hidden > 0` 时加一行「还有 {n} 处未列出」+「展开全部 / 收起」——头部的
  计数从此和能看到的行对得上，也给了看全整份残留的入口（回收仍逐条两步确认，不批量）。
- 已消费 i18n：`steam.residueMore / residueShowAll / residueCollapse`（zh/en 各三条）。

### 2026-10-24 操作历史台账到拉取上限时明说「只显示最近 N 条」（R310）

历史页按 `LOAD_LIMIT = 500` 拉台账（后端 `list_undo` 只 take(limit)、不返回 total）。
条数够到上限时，页面把 500 当成「共 N 条」、把第 500 条当成最早记录——更早的记录
既看不见也没说没加载，用户会以为台账只有这么多。本条把这件事说清并给出口：

- 新增纯函数 `historyTruncated(entries, limit)`：未加载不喊截断；少于上限 false；
  **恰好等量按可能截断处理**（无 total 可查，保守比说错好）；上限非正视为无效
  不报截断。配 5 条单测。
- 截断时列表上方一行说明「记录较多，只显示最近 {n} 条（更早的未加载）」，并给
  「查看全部」（复用此前定义了没人渲染的 `history.viewAll`）：按 5000 上限重拉一次
  （后端本就整读 jsonl 再 take，多要条目零额外磁盘扫描）；仍够到 5000 就只提示、
  不再给按钮——不造一个点了也没更多东西的按钮。

### 2026-10-24 winapp2 预览空结果不再唱「全部安全」（R309）

粘贴只含注册表项/内容为空的 winapp2.ini 解析后，预览此前把 0 条结果当成
「全部可安全安装」——那是 0 条的数据，不是健康结论；且解析失败时直接丢原始异常
文本。本条把为这两个状态写好的词条真正接上：

- 解析出 0 条 → 显示「未解析出可清理文件规则（可能只含注册表项或内容为空）」
  （`studio.winapp2Empty`，此前定义了没人渲染）；0 条时不再出现「全选/全部安全」。
- 解析抛错 → `studio.winapp2ParseFailed`（解析失败：{msg}），文案与错误
  语言一致（此前直接 `String(e)` 可能是英文异常）。
- 没有可安装条目（含全被红线拒绝）时「安装 N 条」按钮禁用，不让用户点了才吃
  一个「请先勾选」错误提示。

### 2026-10-24 体检条温度格点下去滚到温度采样历史（R308）

温度格此前是不可点的 div：说「最高温 71℃」却不给任何去向——采样点、区间峰值、
「最新采样于 X」全在下方温度采样卡里，用户得自己滚。本条给它与红盘格/扫描时效格
同构的落点：

- 点击 → 平滑滚到本页「温度采样历史」卡（`thermalRef` 挂在卡片包裹层上——
  卡片加载中返回 null 时也能正确滚到位置），只导航不动作、不新造任何采样。
- 温度偏高时 title 保留原有告警措辞并追加「滚到温度采样历史看明细」；
  未接回调时按钮禁用（与启动项格同口径），不造点了没反应的按钮。

### 2026-10-24 扫描时效格点下去告诉你是哪块盘没扫（R307）

扫描时效格（R293）此前只是文本：说「最久 21 天没扫」却不说是哪块盘，用户还得去盘条
里逐块猜。本条给它一个与红盘格（R305）同构的落点：

- never/stale/skew 态点击 → 选中最久没扫的那块盘（`summarizeScanAge` 的 `worstRoot`）
  并平滑滚到盘条，右侧详情卡与救援横幅跟着选中盘走；ok/未读态没有可处理对象，
  维持不可点 div——不造一个点了没反应的按钮。
- 只选中不动作：真要扫它仍点盘卡片（与既有交互一致），不静默起 IO。
- 格子内容抽成一份 JSX 供可点/不可点两分支共用，避免两份措辞漂移；
  悬停标题保留原有三态措辞并补一句点击说明。

### 2026-10-24 温度采样卡补「最新采样于什么时候」（R306）

温度采样卡（R299）此前只给「近 N 点 · 跨度 X 分钟 + 区间峰值」——采样开关关了一周，
曲线停在旧处也看不出来。本条把数据的新鲜度明示出来：

- 卡片补一行「最新采样 X 分钟/小时/天前」：`summarizeThermalHistory` 新增 `latestAt`
  （最新有效采样点的 unix 秒，`unread`/`empty` 为 null），新纯函数 `sampleAge` 把
  「多久以前」换算成展示单位（<60 分钟 → 分钟、<48 小时 → 小时、再往上 → 天；
  时钟回拨不显示负时间；无采样时间返回 null 不编年龄）。
- 绝对日期放悬停 title（`formatDateKey`）：数据是三天前的还是三分钟前的，一眼分清。
- 配 5 条 `sampleAge` 单测（单位进位、48 小时进位、时钟回拨、null/NaN），
  既有汇总断言全部补 `latestAt`。

### 2026-10-24 体检条红盘格点下去真的「查看红盘」（R305）

体检条红盘格的 title 一直写着「查看红盘」，点下去却和下方「扫描全部」做同一件事——
直接开始全盘扫描，用户看不到是哪块盘红了。本条给它一个明确落点：

- 有红盘 → 点击选中最满的那块红盘（右侧详情卡 + 低配救援横幅都跟着它走），并把视图
  滚到盘条；没有红盘 → 退化为重新扫描读取容量，title 同步改为「重新扫描读取容量」，
  不再拿「查看红盘」指着一个扫描动作。
- 落点判定抽成纯函数 `pickWorstRedDrive`（`AssetOverview.tsx`）：按物理卷去重、
  容量没读到的盘不参与排序（不能拿 0 当最满），配 4 条单测。

### 2026-10-24 启动项提醒的动作按钮指向能管它的页面（R304）

R292 的启动项变化 toast 只说「新增/消失了几项」（这是对的：软件自装与恶意自装数据上
无法区分），但动作按钮写的是「去总览」——总览里启动项只是一行不可点的计数，
点进去既看不到明细也没有启停入口。真正能看/管启动项的是「系统优化」页的启动项面板。
本条把入口对齐：

- toast 动作改为「去管启动项」，直达系统优化页——与 R294 散热 toast「去调散热」同族
  （动作按钮必须把用户带到信息所在的地方）。
- 总览体检条的启动项格从不可点的计数文本改为按钮（与硬盘健康格一致），
  悬停提示「去系统优化看启动项明细」，点击直达同一页面。

### 2026-10-24 扫描时效格补上「上次扫于哪天」（R303）

后端 `scan_staleness_cmd` 一直返回 `last_at`（源码注释写明「展示『上次扫于 X』用」），
但总览体检卡的扫描时效格只保留天数——最久一块盘「21 天没扫」，用户看不出到底是
哪天扫的，也没法对照「是不是我上个月刚做过一次体检」。本条把日期接上：

- `ScanAgeRow.staleness` 补 `last_at`（与后端 `ScanStalenessOut` 同形，不再手写窄类型丢字段），
  `summarizeScanAge` 返回 `worstLastAt`，如实带出最该处理那盘的上次扫描时间。
- 体检卡过期态文案改为「最久 21 天没扫（2026-10-03）」，title 同步带上日期；
  `last_at` 为 null（从未扫过/读数异常）时回退原文案，不编日期。
- 补 2 条单测：`worstLastAt` 带出与 null 回退、never/skew 如实带出。

### 2026-10-24 立即检查的结果里也点出授权命中项（R302）

R295 让清理提醒的 toast 点出「其中 N 项已按你的授权规则进清单」，但设置页
自己那个「立即检查」按钮的结果只显示总可清字节与每盘明细——同一份
`ReminderPayload.auto_list_hits`，通知里有、设置页没有，用户点完检查看不到
哪几项是按授权自动进清单的。本条把它补上：

- 设置 → 清理提醒的「立即检查」结果在每盘明细下列出命中项（本机清理项名字 +
  字节）；未知 scaffold（规则指向已卸载项）原样显示 id，不猜近似名字。
- 判定抽成纯函数 `hitRows(hits, scaffolds)`（`ReminderSettings.tsx`），配 4 条单测：
  已知/未知 id、null 与空数组、scaffolds 未加载、保持后端命中顺序。

### 2026-10-24 写好了却没人渲染的说明补上，真死键清掉（R301）

一次 i18n 死键审计（1964 个词条 × 1747 处静态调用）发现两类问题——都不是翻译错，
是**「文案写了但界面没接」**与**「功能迭代后留下的孤儿词条」**：

- **补接线 3 处**（词条有、UI 没渲染，用户看不到原本写好的解释）：
  - 总览「温度采样」卡底部补 `overview.thermal.hint`——采样开关在设置页、不在卡上，
    不写清用户会以为曲线是从这里开出来的。
  - 设置 → 清理提醒的阈值下拉前补 `system.reminder.autoListThreshold`，那个数字
    此前没有任何标签。
  - 插件市场「进阶：社区索引」展开后补 `toolbelt.mkt.advancedSub` 一行说明。
- **删 6 个真死键**（zh/en 各 3）：`system.reminder.startupAdded`、`startupGone`
  （R300 改成整句 `startupDelta` 后遗留）、`toolbelt.mkt.installableCount`
  （被 `communityAllComingSoon` 取代）。
- 审计口径写进注释：`perm.${id}`、`toolbelt.healthRating.${rating}`、
  `DistributionBar` 的 `ns` 参数这类**动态拼键**不能算死键——剩下的 65 个都是它们。

### 2026-10-24 启动项变化可以主动对比了（R300）

R292 的后端早就有 `check_startup_delta_cmd`——注释里明写着「设置页立即检查按钮」，
但按钮至今不存在：前端只有后台每 6 小时一次的 `startup-changed` 事件 toast，
用户想「现在就看看」没有入口。本条补上主动查询：

- 设置 → 清理提醒的「启动项变化监听」区新增**「立即对比」**按钮：读
  `startup-history.jsonl` 的最后两份快照做集合 diff（只读，零枚举、零写入）。
- **两种「没得比」分开说**（`formatStartupDelta` 纯函数）：`Err(no_history)` = 真空
  （只有 0/1 份快照，首次快照已记）→ 显示「还没有两份快照可比」；真实读失败 →
  显示「未读到」，绝不用「没有变化」糊住任一种。
- 结果措辞：一致 →「与上一份快照一致」；有差异 →「自 N 天前的快照：新增 A 项、
  消失 G 项」——只说「新出现/消失」，不判断是谁做的。
- 浏览器演示诚实：mock 返回空 delta（没有真实快照就不编造新增/消失）。
- 5 条单测锁四态判定（noHistory / unread / same / changed 的数字与 gap 带出）。

- **安全边界**：对比只读最后两份快照，不改任何启动项、不写快照；「监听开关」与
  「快照间隔」仍是唯一会触发新快照的入口。

### 2026-10-24 温度采样历史第一次看得见（R299）

R298 把「温度采样开关」装进了设置页，但开了之后采到的点一个都看不见——
`thermal_history.rs` 里只有 `read_history` / `is_sustained_high` 这些库函数，
**没有任何 Tauri 命令**，`thermal-history.jsonl` 只被提醒线程自己读。用户打开了开关，
却无法回答最基本的问题：它到底在采吗？采到的温度长什么样？

- 后端新增 `get_thermal_history` 命令（`thermal_history.rs` + `lib.rs` 注册）：只读
  `thermal-history.jsonl`，零传感器写、零通知、零新采样。三态诚实——**缺文件 = 从未采样**
  （空数组）、**文件在但读不开 = `Err`**（不伪装成「从来没热过」）。
- 总览新增「温度采样」卡：把已采到的点画成迷你曲线（与空间趋势同一画法，120×34）。
- **四态措辞分开说**（`summarizeThermalHistory` 纯函数）：`unread` 读失败 ≠
  `empty` 还没采样（开关没开或刚开没到周期）≠ `single` 只有一点（有当值但画不了线）
  ≠ `ok` 多点。把读失败说成「还没采样」、把单点画成走势，都是编造。
- **峰值与最新点分开显示**：「一直 70℃」和「刚才冲到 92℃、现在 70℃」是两种完全不同的
  机器状态；只给一个数会说谎。峰值达到 80℃ 阈值时按 caution 着色，不染红整张卡。
- 非法温度（`NaN`/`Inf`）不参与任何统计——它不是温度，混进去只会污染峰值与曲线。
- 8 条单测（`ThermalHistory.test.ts`）锁住四态判定、乱序排序、非法点剔除、单点不画线。

- **安全边界**：本卡纯只读展示，不写入历史、不改采样频率、不触发任何温控动作；
  采样开关仍只在「设置 → 提醒 → 温度持续偏高提醒」一处，风扇曲线仍走 `fan_control` 双确认。

### 2026-10-24 热管理与启动项监听终于能在设置里改（R298）

R291（温度持续偏高）和 R292（启动项变化监听）的后端能力早就齐了，但 5 个旋钮
只活在 `reminder.json` 里——用户想开「温度持续偏高提醒」或调「连续采样点数」，
只能手改 JSON 文件，前端设置页一个字都没有。这两条管家通知实际上是「装了但关不掉
也开不了」的状态。本条补上前端接线：

- 设置 → 清理提醒新增「温度持续偏高提醒」：采样开关 + 判定阈值（75/80/85/90/95 ℃）
  + 连续采样点数（1/3/5/10/15，实时显示折算分钟数）。默认 85 ℃ / 5 点 ≈ 连续
  25 分钟无一秒喘息，与后端 `ReminderConfig::default()` 同口径。
- 新增「启动项变化监听」：开关 + 快照间隔（15/30/60/120/240 分钟）。默认 1 小时。
- `setReminderConfig` 透传 5 个参数（后端 `reminder.rs:160-165` 早就在收，只是
  前端从没传过）；`ReminderConfig` / `MockReminderConfig` 补字段。
- 补 `normalizeReminderCfg` 配置归一化：旧版后端返回的配置没有 `thermal_*` 与
  `startup_watch_*` 字段，直接把 `undefined` 绑到 `<input type="checkbox">` 会让 React
  当成非受控组件（切换一次变半受控，控制台告警且行为不可预测）。归一化按后端默认值
  补缺，**但绝不覆盖已有值**——`??` 只吃 `null`/`undefined`，用户的 0 和 `false` 是显式
  选择，不能被吞。配 4 条单测（补默认值 / 保留已有值 / null 返回 null / 0 与 false 不被吞）。
- 新增 16 组中英文案（`system.reminder.thermal*` / `startup*`），parity 测试守对齐。

- **安全边界**：温度采样只读传感器、启动项只枚举对比，两条都是只通知不写入；
  风扇曲线仍只有 `fan_control` 的 L2 双确认一条路，启动项禁用仍走逐项确认。

### 2026-10-24 扫描时效阈值可配：设置与体检不再各说各话（R297）

R296 把「这块盘多久没扫」接进今日体检，但阈值是硬编码的 21 天常量，而
后端 `scan_stale_days` 是可配置项（默认 21，钳 ≤3650）——用户改了配置，
体检格仍按 21 天说话，两处脱节。本条把阈值收敛到一处：

- 设置 → 清理提醒新增「扫描时效提醒」：0/7/14/21/30/60 天可选，0 = 关闭。
  与「空间耗尽预警」同款分段控件，文案说明阈值同时作用于体检格与后台提醒。
- 今日体检的扫描时效格改为读 `get_reminder_config().scan_stale_days`：
  用户在设置里改，体检跟着改；设为 0 时整格不渲染——与后端
  `collect_scan_stale` 对 0 返回空的 off 语义一致，不显示一个永远绿的空壳。
- `ReminderConfig` 补 `scan_stale_days` 字段，`setReminderConfig` 透传
  `scanStaleDays` 给后端命令（后端早已接受该参数，缺的只是前端入口）。
- 补 1 条单测锁定「阈值是配置不是常量」：同一块 10 天没扫的盘，阈值 21 判
  ok、阈值 7 判 stale；阈值放宽到 30 时 14 天回到 ok。
- `.seg-5.auto` 补 flex 换行规则（CSS 此前只覆盖 `.seg-4.auto`；6 个阈值选项
  在 5 列网格里会挤成 1/5 宽，换行语义要跟着 `.auto` 走）。
- 新增 4 组中英文案（`system.reminder.scanStale*`），parity 测试守住对齐。

- **安全边界**：只读展示 + 只改用户自己的提醒配置。不发起扫描、不删任何东西。

### 2026-10-24 扫描时效进今日体检：这块盘多久没扫（R296）

health-butler P0-3 的前端接线。R293 在后端把「盘多久没扫」的三态判断做完
（`scan_stamp.rs`：Never / ClockSkew / Days），但前端零调用——只有后台 6 小时
一次的 `scan-stale` 通知，用户主动想看「哪块盘落灰了」没有入口。本条补齐：

- 今日体检横幅新增「扫描时效」格：`scan_staleness_cmd` 按盘只读一次
  `scan-stamps.jsonl`（真实扫描落点），零磁盘扫描——守住 health-butler P0
  「不为展示新造扫描」的门禁。
- **修掉一个对齐 bug**：查询侧此前直接拿传入路径做精确比较，而写入侧存的是
  归一化盘根（小写 + 正斜杠，`C:\` → `c:/`）——传原样 `C:\` 一条都匹配不上，
  天天扫的盘会被误报成「从未扫过」，与 Never 语义恰好相反。现在查询侧走
  `norm_query_root` 与写入侧同口径（`C:\` / `C:/` / `c:\` / `c:` 命中同一条
  记录），配 2 条单测锁定。
- **三态措辞分开，不把任何一态说成另一态**：`never` = 还没扫过；`stale` =
  超阈值没扫（最久那块盘的天数）；`clock_skew` = 时间戳晚于当前（时钟异常，
  此刻任何天数都不可信，明说「天数不可信」）；读失败单独显示「扫描记录未读到」
  ——不说成「刚扫过」也不说成「从没扫过」。
- 阈值 21 天与后端 `collect_scan_stale` 默认值同口径；纯函数 `summarizeScanAge`
  配 7 条单测（优先级 unread > never > stale > skew > ok，阈值边界 20/21/0）。
- 新增 8 组中英文案（`overview.checkup.scanAge*`）；zh/en 键对齐由既有
  `i18n/coverage.test.ts` parity 测试守住。
- **安全边界**：只读展示。不发起扫描、不改配置、不删任何东西。

### 2026-10-24 按项授权自动进清单：设置页规则管理（R295）

health-butler P2-1 的前端接线。R290 在后端已把「按项授权」做完整（规则定义、
阈值判定、命中项随 `cleanup-reminder` 下发），但设置页一直没有规则管理 UI——
用户只能手改 `auto-list.json`，P2-1 对普通用户不可见。本条补齐：

- 设置 → 清理提醒 新增「按项授权自动进清单」区：总开关 + 规则列表 + 添加
  （选清理项 + 阈值，可「一有就进」）+ 删除。
- 每条规则按本机真实 scaffolds 标注生效状态：规则点名的清理项本机没有时
  明示「闲置（不会用近似项顶上）」，不猜近似 id。
- 规则覆写语义与后端一致：调用方持有的就是完整规则集；添加时按 id 排序，
  同一项重复授权取更小阈值（多规则命中语义后端已保证）。
- 清理提醒通知按 `ReminderPayload.auto_list_hits` 额外点出「其中 N 项已按你的
  授权规则进清单」；旧后端该字段缺失时静默跳过（向前兼容）。
- **安全边界不变，且是本条的全部要点**：授权只影响「自动进清单」这一步，
  **绝不自动删除**——删除路径仍只有总览页确认窗（`execute_ai_plan`
  `user_confirmed`）。阈值本身是提醒的门槛，不是删除的门槛。
- 新增 10 条中英文案（`system.reminder.autoList*`）；zh/en 键对齐由既有
  `i18n/coverage.test.ts` parity 测试守住。
- 规则变换抽成纯函数 `withRule` 并配 7 条单测（`ReminderSettings.test.ts`）：
  同 id 重复授权按**替换**处理——后端 `should_auto_list` 对同 scaffold 取
  最低阈值，若保留两条同 id 规则，用户刚改大的阈值会被旧的小值静默吞掉。

### 2026-10-23 散热曲线提示闭环：持续超温时一键去调（R294）

health-butler P2-2 的最后一环。R291 已能判定「温度持续偏高」并发通知，
但那条通知的动作按钮指向**总览**——而风扇控制在「设置 → 系统工具」里，
用户点了按钮还得自己再找一遍。「提示应用预设风扇曲线」在半路断了。

- 温度通知的动作按钮改为直达风扇控制区（`setSettingsTab('system')` +
  `setShowSettings(true)`），文案改为「去调散热」。
- **安全边界不变，且是本条的全部要点**：只导航，不代写。应用曲线仍然
  走 `fan_control` 现有的 `confirmed` + 权限中心双确认——「托管」的
  含义是「主动提示」，不是「绕过确认替用户写」。红线「写操作每次确认
  不变」在此落成具体行为：本条没有任何写入路径。
- 我曾以为这条需要用户拍板「是否绕过 L2 确认」，重读路线图后确认自己
  读错了：原文是「**提示**应用一条预设风扇曲线（写操作，L2 每次确认
  不变）」——答案早就写在需求里，不需要额外决策。
- 新增 2 条中英文案（`shell.toast.actionFanCurve`）；zh/en 键对齐由既有
  `i18n/coverage.test.ts` 的 parity 测试守住（它遍历所有 namespace 断言
  zh/en 无缺键无孤儿键，因此新键漏翻会直接让套件红）。

### 2026-10-23 扫描时间戳历史：P0-3 三信号齐了（R293）

health-butler P0-3 最后一条信号「C 盘三周没扫」。这条的数据源缺口是
结构性的：**扫描只发生在内存**——`AppState.cleanup_cache` 与扫描树缓存
都是进程内 `Mutex`、不落盘，进程一退全部消失；全仓库此前没有任何
`last_scan_at` 类记录。

**刻意不拿 space_history 冒充**：`space-history.jsonl` 的采样时间是趋势
采样（后台线程每日一次、`record_all_drives`），与「用户扫过没」是两件
事——没扫过的盘照样每天在采。拿它算「多久没扫」会把从没扫过的盘说成
「昨天刚扫」，是编造。故本模块独立记录，只认 `commit_scan` 这一个真实
扫描落点。

- 新增 `scan_stamp.rs`：按 root 追加 `{root, at}` 到 `scan-stamps.jsonl`，
  key 用**归一化盘根**（`C:\` → `c:/`，与 suggestion_history /
  space_history 同口径）——否则同一块盘会因大小写与斜杠裂成多条记录，
  「多久没扫」算出来全错。60s 去重、每 root 留最近 30 条、写失败静默。
- `scan_staleness` 返回三态而非 `Option<u64>`：`Never`（从未扫过）/
  `ClockSkew`（记录时间晚于 now）/ `Days(n)`。**三者混在一个 Option 里
  就分不清「还没扫过」与「读失败」与「时钟异常」**，而这三种的 UI 措辞
  完全不同。读失败则整条 Err（R88：不得伪装成「刚扫过」——那会让该
  提醒时完全不提醒）。
- `ClockSkew` 保守返回「0 天前扫过」而不猜：误报刚扫过最多让提醒晚一轮；
  误报「三年没扫」会让用户以为数据全丢了。且它**不作为预警触发条件**
  ——此刻任何天数都不可信。
- 候选盘只从**已有记录**的 root 里取，不凭空枚举盘符：否则会得到一堆
  从未扫过也从未被记录的空洞盘，把提醒变成噪音。
- 配置 `scan_stale_days`（默认 21，钳 1..=3650；0 = 关闭）。6h 检查一次，
  flag 防抖（有盘被扫过才复位）。
- 前端：`App.tsx` 监听 `scan-stale`，走既有 toast 通道；「还没扫过」与
  「N 天没扫」措辞分开。新增 payload 类型 `ScanStaleWarn` 与 3 条中英文案。
- 新增 8 个 Rust 单测（lib 202 → 210）：真空态≠0 天 / 向下取整边界 /
  「差 1 秒满 3 周」仍是 20 天 / 未来时间戳是 ClockSkew / 取 max 而非最后
  一行 / 缺文件真空态 / 损坏行报错 / 裁剪保留每 root 最新。

**至此 P0-3 三条信号全部闭环**：温度持续偏高（R291）、启动项变化
（R292）、扫描久未（R293）——且都已接通前端通知。三条都严格限定在
「只读 + 只通知」，不含任何写路径。

### 2026-10-23 启动项快照历史：接上前端通知，「悄悄加了两个」可见（R292）

health-butler P0-3 第二条信号。`list_startup_items()` 只给当前一份清单，
没有上一份就无从谈「多了两个」——总览今日体检里的启动项数字只是计数。
本条补上快照 + diff，并把 R291/R292 两个后端事件接到前端通知上
（此前只有后端 emit，没有任何 UI 听，功能对用户不可见）。

**关键设计决定——不用 StartupItem.id 做 diff**：
`startup.rs` 的 id 是 `format!("{loc}-{seq}")`，seq 是**单次枚举的自增
计数器**（HKCU/HKLM 两个键 + 启动文件夹依序累加）。这意味着删掉任意
一项会让其后所有 id 整体错位——拿 id 做集合 diff 会产出大量虚假的
「新增 / 消失」。故本模块用**内容键** `location + 单元分隔符 + name`：
同一位置、同一名字就是同一项，与枚举顺序无关。已配单测精确复现
「删中间一项」这个场景，锁住回归。

**诚实性约束（写进模块注释与文案）**：diff 只能说「自上次快照后新出现」，
**不能说是谁加的、也不能判定有害**——软件自装与恶意自装在注册表数据上
长得一模一样，本模块没有能力区分，也不假装能。通知文案因此只说
「新增 N 项 / 减少 M 项」，不带任何归因。

- 新增 `startup_history.rs`：追加式 jsonl，族规同 thermal_history
  （写失败静默、60s dedup、缺文件 = 真空态、损坏行 → Err）。
  **空集合绝不落快照**：读失败掏出空清单时若落盘，下一次会把现有启动项
  全报成新增——那是把故障放大成谎报。**集合未变也不落**：否则一天能写
  出几百份一模一样的行。
- `diff_snapshots` 把「无历史」做成**可命名的错误** `"no_history"`，而不是
  返回 added/gone 全空——后者会被 UI 说成「无变化」，把「没数据」演成
  「没变化」是谎。
- `gap_days` 用**向下**取整而非 `div_ceil`：跨度 1 秒也该是 0 天（同一天
  内）。这条是被自己的测试抓出来的——初版写 div_ceil，测试断言
  「1 秒后仍是 0 天」失败，确认代码错而非测试错。「上一次快照就在一分钟前」
  被说成「距上次 1 天」是把间隔夸大一倍以上的误导。
- 配置 `startup_watch_enabled` + `startup_watch_minutes`（默认 60，钳
  1..=1440；低于 1 分钟会变成 PowerShell 进程风暴）。
- 前端：`App.tsx` 监听 `thermal-sustained` 与 `startup-changed` 两个事件，
  走既有 toast 通道（对齐 space-full-reminder 的 R103 模式），点击直达总览。
  新增 payload 类型 `ThermalSustained` / `StartupChanged` 与 6 条中英文案。
- 新增 9 个 Rust 单测（lib 193 → 202）：内容键与顺序无关 / **seq 错位不
  产生假新增** / 无历史是可命名错误 / added+gone 同时上报 / 集合相同仍是
  diff / gap_days 三种边界 / 缺文件真空态 / 损坏行报错 / 监听默认关。

### 2026-10-23 温度采样历史：让「持续偏高」第一次可判断（R291）

health-butler P0/P2 共用的缺失底座。此前 `sample_thermal_c()` 只给**当前**
一帧温度——瞬时尖峰（编译/解压让 CPU 冲高十秒）与真的散热不济在单帧里长得
一模一样。无论总览的 isTempHigh、P0-3 想要的管家通知、P2-2 想要的风扇托管，
都卡在同一处：**没有「持续」可言**。瞬时即判定会把正常机器唱成过热机，那比
不报更坏。

- 新增 `thermal_history.rs`：追加式 jsonl（5 分钟一点，上限 3000 条约 3 周半），
  与 space_history / suggestion_history 同族规——写失败静默、60s dedup、缺文件 =
  真空态空 vec、文件在但读失败 → Err（R88：读失败不得伪装成「从未过热」）。
- 核心判定 `is_sustained_high(pts, threshold_c, min_samples)`：窗口内**全部**
  采样 ≥ 阈值才算持续，且只看最近 min_samples 条。
  两条语义是这条的全部价值：
  ① **不用平均值**——平均 84℃ 可能来自「一半 95℃ 一半 73℃」（真散热不济），
     也可能来自「一直稳稳 84℃」（机器本就这样）。二者处置完全不同，平均值会把
     它们混为一谈。
  ② **不用单点**——只有一个点时「全部 ≥ 阈值」等价于瞬时判定，正是要消灭的东西。
     样本不足 → 判否并如实返回实际条数，调用方据此说「还在观察」。
- 接线 `reminder` 后台线程：`thermal_sample_enabled` 独立开关（采集零成本，
  顺手读一次传感器，不碰磁盘），每 5 分钟一点；命中 `thermal-sustained` 事件，
  **本地 flag 防抖**（报过一次就等温度回落才复位，否则每分钟一条 toast 等于骚扰）。
。配置项 `thermal_sustained_c`（默认 85℃，钳 50..=110）与
  `thermal_sustained_samples`（默认 5，钳 1..=60）。
- **仍然只读**：命中只发通知。应用风扇曲线依旧走 `fan_control` 的
  「confirmed + 权限中心」双确认，本条不绕过、也不自动写入任何调速通道。
- 新增 10 个 Rust 单测（lib 183 → 193）：样本不足不判定 / 全超温才判定 /
  一秒回落即破持续 / 只看最近窗口 / 峰值查询 / 缺文件真空态 / 损坏行报错 /
  追加与裁剪保序 + 采集默认关 + 用户选 1 点如实保留。

### 2026-10-23 按 scaffold 授权自动进清单：免去「想起来要看」（R290）

health-butler P2-1。用户可给某个 scaffold 配一条阈值规则（如「微信缓存
≥5GB 时提醒我」），命中即在既有清理提醒的节奏里主动告知。**授权范围严格限
于「自动进清单/主动告知」，不触碰执行**——真执行仍只有用户逐项确认后的
`execute_ai_plan(user_confirmed)` 一条路，铁律不变。

- 新增 `auto_list.rs`：规则模型（scaffold_id + min_bytes）与三个纯函数
  （`should_auto_list` / `rule_status` / `tripped_rules`），全部可单测；
  配置持久化到 `app_data_dir()/auto-list.json`，读→合并→写，缺文件=空规则集
  （真空态诚实），读失败静默 default（增强功能不阻塞扫描/提醒主流程）。
- 三条判定约定值得单独说：
  ① **总开关关 = 一律不自动**（规则再匹配也不生效），与 `reminder.enabled` 同语义；
  ② **同一 scaffold 多条规则取最低阈值**——用户既已授权，取更低的阈值才不会让
     「配了 5GB 却因另一条 1GB 规则没生效」这种意外发生；
  ③ **规则点名的 scaffold 本机不存在 → 报 `unknown_scaffold` 并闲置**，绝不悄悄
     用一个近似 id 顶上（拼错要被看见，不是被猜中）。
- 接线在 `reminder::compute_reminder` 的本轮循环内随手累计 scaffold 小计，
  **零额外磁盘扫描**——P0 门禁「不为展示新造磁盘扫描」是硬约束，特意没有另起
  一遍 `cleanup_suggestions`。命中项随 `ReminderPayload.auto_list_hits` 下发
  （`#[serde(default)]`，旧前端不读此字段，向前兼容）。
- 三个 Tauri 命令：`get_auto_list_config` / `set_auto_list_config` /
  `get_auto_list_config_with_status`（后者把每条规则连同其生效状态一起返回，
  UI 才能明示「这条规则指向的 scaffold 本机没有」）。
- 新增 8 个 Rust 单测：开关语义 / 无规则不触发 / 阈值含边界 / 阈值为 0 /
  多规则取最低阈值 / 未知 scaffold 不猜 / `tripped_rules` 过滤与定序 / 关开关
  时 `tripped_rules` 返回空。

### 2026-10-23 AI 清理预演工具：自己核实清单，不把幻觉路径交给用户（R289）

- 此前 AI 出清理清单只能"凭猜"：`propose_cleanup_plan` 纯出清单、从不校验
  路径是否真实存在，模型编造或已不存在的路径会原样进确认窗，用户点完才发现
  一条都没命中。R288 落地后总览有了跨次对比，但"这次清单准不准"仍无人核实。
- R289 新增 `execute_cleanup`：**预演**（走 `execute_ai_plan` 的 `dryRun`
  分支）——逐条实测路径存在性与可释放字节，AI 在给最终建议前就知道"这条真的
  命中了吗、能清多少"，返回文本如实汇报命中项与未命中项（含原因）。
- 安全边界写得比功能更硬：放开的是**只读核实**，不是执行权——
  ① 真执行仍只有 `executeAiPlan(userConfirmed)` 一条路，`user_confirmed`
  是唯一放行条件，缺它一律拒绝；
  ② 预演**免 L2 权限门**（dry-run 是同级的只读实测，若要求开 L2，AI 连
  "核实路径"都做不到，反而更依赖猜测——比放行只读更大的风险）；
  ③ 存在性校验方向反转：真执行遇不存在路径**整体拒绝**，预演反而**逐条
  报 0 字节**——正是要靠它发现"这条不存在"，让 AI 自己把幻觉项划掉；
  ④ 都走 `execute()` 的 `protected_path()` 黑名单，预演与真执行同口径；
  ⑤ dry-run 分支只测字节、不回收、**不写 undo 台账**（否则"最近清理"会塞满
  无法撤销的幽灵条目）。
- 确认窗链路与 `propose_cleanup_plan` 完全同源：只挂 `pendingProposal`，
  由 `agent.ts` 的 `takeProposal()` 统一交 UI 确认窗，不直接弹多个窗；
  **只有真实命中的项才进确认窗**——预演不到等于不存在，不该把幻觉路径端给
  用户点；一条都没命中时**不挂**清单（空确认窗会让用户对"清什么"一无所知）。
- 描述里明确禁止越权："你不能真正执行任何删除/回收——真正回收只发生在
  用户逐项核对并确认之后。"
- 新增 6 个前端测试（tools.test 79→85）：注册与 `tauriOnly` 归属 / 命中项
  经 pendingProposal 全量交付确认窗 / 未命中项剔除且如实列出 / 全未命中不
  产生清单 / 空 items 明确报错 / 预演不写 undo 台账（dry-run 纯只读）。
  （**前端 751 总 / 43 files**；Rust 175 lib + tsc/eslint/vitest 关。
  Rust lib 由 R288 的 174 增至 175：本条同时修了 R288 一个会静默失效
  的 bug，见下节。）

### 2026-10-22 建议清理跨次对比落地：总览答「比上次多出 X GB」（R288）

- health-butler P0 承诺的「建议清理卡补跨次对比：比上次多出 X GB —— 用
  cleanup_suggestions 快照 diff」此前只有文档、没有任何实现（grep 0 命中）。
  各 scaffold 可清量每次都要重算（全盘 walk 或内存树统计），跨次对比不可能
  靠"再算一次上次"得到——上次的树早就变了，所以必须在建议算出来那一刻把
  汇总数字记下来。
- 新增 `suggestion_history.rs`（与 space_history / hw_history 同一族规）：
  追加式 jsonl 单行快照，只存 root 级总量 + 每 scaffold 小计，**不落路径
  明细**（明细随文件系统实时变化，存下来立刻过期，拿旧明细当事实是编造；
  且总量与 scaffold 小计已足够回答"多出多少 / 涨在哪个应用"）。
- 族规沿用：写失败静默（对比是增强功能，不阻塞扫描主流程）；缺文件 = 真空态
  → 空；文件存在但读失败 → Err（R88 诚实性：读失败不得伪装成"没有历史"）；
  同 root 60s 去重；每 root 保留最近 30 条。
- `diff_points` 纯函数的诚实性约定：没有上次 → `total_delta = null`（**不
  当 0 说成"持平"**）；只在两个快照都出现过的 scaffold 才给环比项，单边出现的
  分别归 new/gone（不把"新增"误述成"涨了 N 字节"）；同刻或时间倒序 → 不出
  差值（间隔 0 天的环比无意义）。
- 接线：`commit_scan` 在把建议 move 进 cleanup_cache 之前提取汇总并记录
  （扫描完成即有一次真实快照，不为展示新造任何磁盘 IO）。
- **本机验证边界（写下来而非隐去）**：`cargo test --workspace` 在本机有
  `-p diskpilot-desktop --bin diskpilot` 一个 target 无法运行——Windows 拒绝
  启动该 exe（`os error 740`，需要提权），直接以 PowerShell 启动同一 exe 同样
  报 740。该 target 在 `src/main.rs` 中 `#[test]` 计数为 0，故无测试损失；
  但**该执行路径在本机从未被真正跑过**，不能声称"全绿"。其余 274（非 desktop）
  + 175（desktop lib）实测通过，Rust 合计 0 failed。
- 前端：总览「建议清理项目」卡标题下加环比行（↑多出/↓更少/持平 + 距上次
  N 天 + 主要变动 scaffold + 新增/减少类数），全部走语义 token（深浅主题
  自动适配）；读失败显示"未读到"而非凭空消失；只有本次快照时显示"首次
  统计，下次扫描后可对比变化"。
- **Bug 修复（与 R289 同一提交）**：`suggestion_diff` 查询侧未做 key 归一化
  ——写入侧 `norm_path_key` 产出小写 + 正斜杠（`C:\` → `c:/`），前端
  `suggestionDiff(root)` 送的却是原始盘符形状 `C:\`，`map.get(&root_path)`
  因此**永远查不到刚记下的快照**：真机上环比必然为空，且失败形态是
  `Ok(None)`（"没有历史"）而非报错——即 R88 明令禁止的"读失败/查不到伪装成
  没有历史"。修复为查询时同样走 `norm_path_key`，配回归测试锁住口径。
  （此 bug 使 R288 的环比功能在真机不可用；修后才能声称该功能可用。）
- 预览模式 mock 诚实：没有跨会话历史文件，不凭空造"上次"假环比——首次询问
  = 无比值，本会话内再次询问才与上一份会话快照比（能演示真实交互）。
- 新增 9 个 Rust 测试 + 5 个前端测试（diffTrend 四态 / shouldShowDiff 门）。
  （**745 总 / 43 files**；Rust 174 lib + cargo clippy/tsc/eslint 关）

### 2026-10-17 插件只读工具预览放行：已装插件/市场盘点（R287）

- 插件类工具里 list_plugins / market_plugins 是只读枚举（pluginListInstalled
  / pluginMarket 都有 mock 演示数据源，与插件页/插件市场页同源），却仍
  tauriOnly。预览下 AI 问「装了哪些插件/市场有什么」无工具可用；且
  list_plugins 原 execute 走 toolbeltCatalog（演示目录无 plugin 字段），
  预览会永远返回「没有插件」——改用 pluginListInstalled 拿到真实演示数据。
- R287 修复：放行这两个只读枚举（预览返回演示插件清单，标注"与插件管理
  页同源"）；activate_plugin / uninstall_plugin / export_plugin / install_plugin
  （补写清单/写文件/下载安装）保持 tauriOnly。
- 新增 8 测试（tools.test 71→79）：2 放行不带 tauriOnly + 4 写保持 +
  2 执行层（已安装插件/内置市场）。
  （**740 总 / 43 files**；tsc/eslint/build 关）。

### 2026-10-17 硬件报告预览权限门顺序修正：不误导开预览用不上的开关（R286）

- R285 放行 generate_hw_report 时，带 savePath 的分支先查 L1 权限门再查
  预览——预览下带 savePath 且权限未开，会返回「去设置→AI 权限中心开启
  导出报告到文件」。可预览下开权限也没有后端可写文件，把用户引去开一个
  预览用不上的开关是误导。
- R286 修复：预览分支提前——非 Tauri 下带 savePath 直接返回「浏览器预览
  模式不支持写文件：导出报告文件需桌面应用」；权限门只在桌面端生效。
- 新增 1 测试（tools.test 70→71）：预览下带 savePath 拒绝且不提权限中心/
  未授权。
  （**732 总 / 43 files**；tsc/eslint/build 关）。

### 2026-10-17 硬件报告生成预览放行：只读正文演示（R285）

- R277 放行 get_hardware_info / analyze_disk_health 时整体藏了 generate_hw_report。
  但它无 savePath 时是纯只读正文生成（markdown 报告文本），预览下「给我
  一份硬件报告」AI 无工具可用；只有带 savePath 才写文件。
- R285 修复：放行 generate_hw_report——非 Tauri 分支配平 savePath：无
  savePath 基于 hwInfo mock 渲染演示报告正文（标注预览模式，只读无副作用）；
  带 savePath 明确拒绝「浏览器预览模式不支持写文件」。桌面端仍走
  api.hwReport 完整报告（含保存）。
- 新增 3 测试（tools.test 67→70）：不带 tauriOnly + 执行层（演示正文 / 带
  savePath 拒绝）。
  （**731 总 / 43 files**；tsc/eslint/build 关）。

### 2026-10-17 清理清单生成闭环测试：确认窗数据链路验证（R284）

- R283 放行 propose_cleanup_plan 只测了 execute 返回文案，没测关键链路：
  AI 出的清单要经 pendingProposal → takeProposal() 交给 UI 确认窗。若这
  个链路断，清单只存在于工具返回文本里，UI 确认窗拿到的是空——预览与
  桌面端同链路，不能只靠返回文案。
- R284 修复：新增闭环测试——execute 生成清单 → takeProposal 取走 → 断言
  title/items 完整 → 再取为空（无残留）。
- 新增 1 测试（tools.test 67→68）。
  （**729 总 / 43 files**；tsc/eslint/build 关）。

### 2026-10-17 清理清单生成预览放行：AI 演示完整清理流程（R283）

- R274-R282 放行了大批只读工具，但 propose_cleanup_plan（生成【待用户
  确认】清理清单，是「AI 帮清理」的核心流程工具）仍 tauriOnly。它是纯出
  清单：pendingProposal + UI 确认窗，本身零文件操作；确认后执行走清理页
  mock 台账（R273-R281 已诚实化）——预览下 AI 完全可以演示「出清单→
  用户逐项确认→执行回收」完整闭环，被 tauriOnly 藏起来反而让预览 AI
  无法建议清理。
- R283 修复：放行 propose_cleanup_plan（预览确认窗同桌面端，执行落 mock
  台账）；previewSystemNote 声明补一句「生成的清理清单路径来自演示数据源，
  可演示流程，不能假装清的是本机真实文件」。
- 新增 2 测试（tools.test 65→67）：不带 tauriOnly + 执行层跑通（返回确认
  文案）。
  （**728 总 / 43 files**；tsc/eslint/build 关）。

### 2026-10-17 预览模式 AI 数据诚实声明：演示数据不再说成本机真实（R282）

- 缺口：R274-R281 批量放行只读工具（磁盘/硬件/系统状态/风扇/Steam/清理
  建议），预览模式 AI 拿到的是 mock 演示数据。但 agentChat 的系统提示词
  只强调「工具返回真实数据，不要编造」——AI 不知道当前在浏览器预览模式，
  会把演示磁盘/硬件数值直接当成本机真实数据输出，误导用户。
- R282 修复：新增 previewSystemNote()（非 Tauri 注入「浏览器预览模式声明」：
  这些工具返回的是演示数据必须如实说是演示、不得说成本机真实、真实数据
  需桌面端）拼进 agentChat system 尾部；i18n chat.hero.browserMode 文案同步
  更新（原「AI 会走真实接口、硬件类工具需桌面端」已过时——硬件只读已
  预览放行，改为「模拟数据 AI 工具会标注」）。
- 新增 2 测试（agent.test 6→8）：非 Tauri 返回声明、Tauri 环境返回空。
  （**726 总 / 43 files**；tsc/eslint/build 关）。

### 2026-10-17 AI 工具中文标签全覆盖：预览放行工具不直显裸名（R281）

- 缺口：R275 放行的 get_fan_status 不在 agent.ts 的 TOOL_LABEL 表里，
  AI 操作历史/trace 显示的是 prettify 兜底英文（"Fan Status"），与其余
  工具的中文标签风格不一致；且没有任何测试保证「每个注册工具都有中文
  标签」——以后再加工具漏标也不会被发现。
- R281 修复：补 get_fan_status 的标签「风扇控制状态读取」；导出 toolLabel
  并新增测试块：①所有 toolRegistry 工具标签非空且不含下划线（不直显
  snake_case）②get_fan_status 有专门中文标签 ③未见过的工具优雅降级。
- 新增 3 测试（agent.test 3→6）。
  （**724 总 / 43 files**；tsc/eslint/build 关）。

### 2026-10-17 只读工具预览支路执行层测试：跑真数据验证放行非空转（R280）

- R274-R279 放行了 12 个只读工具，但测试只断言「注册表不带 tauriOnly」——
  注册存在 ≠ 预览支路真的产出数据。若某工具预览分支依赖缺失的
  navigator/window（node 环境）会崩，或 mock 数据源没给够字段会空转，
  注册表断言发现不了。
- R280 修复：新增执行层测试块——node 环境 isTauri=false 走浏览器预览
  支路，真实跑 14 组 execute 断言产出文本内容（path_size 实测曝光、
  get_disk_health 演示数据标注、get_fan_status 只读口径、get_cli_tool_usage
  /get_tool_manifest 预览标注与未收录提示等）；顺带给 run_system_probe
  的预览支路补 navigator 守卫（node 环境无名 crash）。
- 新增 15 测试（tools.test 50→65）。
  （**721 总 / 43 files**；tsc/eslint/build 关）。

### 2026-10-17 工具箱信息工具预览放行：能力说明书（R279）

- R274-R278 放行了大批只读工具，但 get_tool_manifest / get_cli_tool_usage
  （AI 执行 run_cli_tool 前必查参数的工具）仍 tauriOnly——预览模式
  toolbeltManifests/toolbeltUsage 返回空/拒绝，AI 想问「这个工具怎么用」
  无任何数据。而 TOOLBELT_CATALOG mock（与工具墙同源，18 个工具的描述/
  分类/风险/标签）一直存在却没用上。
- R279 修复：两个工具加非 Tauri 预览支路——按名在 TOOLBELT_CATALOG 里查，
  命中返回描述级信息并明示「预览仅演示目录信息，无完整参数表」；未收录
  如实说明。桌面端仍走完整 Manifest/使用文档。run_cli_tool（真正执行）保持
  tauriOnly。
- 新增 3 测试（tools.test 47→50）：两个信息工具不带 tauriOnly + run_cli_tool
  仍带。
  （**706 总 / 43 files**；tsc/eslint/build 关）。

### 2026-10-17 系统状态工具预览放行：配置/运行状态（R278）

- R274-R277 放行了测量/汇总/硬件类只读工具，但 get_system_info（execute
  本就内置非 Tauri 回退支路：Windows 版本 UA 解析 + CPU/内存浏览器近似值）
  与 run_system_probe（「看看现在卡不卡」时 AI 必调）仍 tauriOnly 被藏。
  预览模式 AI 答「电脑什么状态」时只能用 get_hardware_info 的静态配置，
  没有实时状态口径。
- R278 修复：放行 get_system_info（现有回退支路即预览数据源）；run_system_probe
  补同样预览支路（browser 近似值 + listDrives 演示清单，明示「浏览器拿不到
  实时占用」）并放行。list_top_processes 无进程快照数据源保持隐藏（不假装
  有真实进程列表）。
- 新增 2 测试（tools.test 47→49）：get_system_info / run_system_probe 不带
  tauriOnly。
  （**705 总 / 43 files**；tsc/eslint/build 关）。

### 2026-10-17 硬件只读工具预览放行：配置/硬盘健康（R277）

- R274-R276 放行了测量/汇总/磁盘容量类只读工具，但 get_hardware_info /
  analyze_disk_health（用户问「这电脑什么配置」「硬盘还好吗」时 AI 必调
  的两个只读工具）仍 tauriOnly——底层 hwInfo / hwDiskHealth 有 mock 演示
  数据源（与「系统信息」页同源），预览模式 AI 答配置问题时只能凭记忆。
- R277 修复：放行这两个硬件只读工具（预览用 mock 同源数据，只读无副作用）；
  run_hardware_test（写系统压测）与 generate_hw_report（savePath 写文件）
  保持 tauriOnly + 权限门——预览不假装能写。
- 新增 4 测试（tools.test 44→48）：get_hardware_info / analyze_disk_health
  不带 tauriOnly；run_hardware_test / generate_hw_report 仍带 tauriOnly。
  （**704 总 / 43 files**；tsc/eslint/build 关）。

### 2026-10-17 磁盘健康工具预览放行：AI 可读演示磁盘清单（R276）

- R274/R275 放行了测量与汇总只读工具，但 get_disk_health（问「哪块盘满
  了/该清哪里」时 AI 必调的工具）仍 tauriOnly——现有实现走
  api.callTool('disk_health')（预览 reject），预览模式 AI 答磁盘问题时
  只能凭记忆猜，没有任何实时磁盘数据可用。
- R276 修复：非 Tauri 分支用 listDrives mock（与总览磁盘卡同源，used 随
  会话释放台账下降）拼只读磁盘清单文本，并明示「浏览器预览模式演示
  数据」；桌面端仍走 agent-server MCP disk_health 统一单源（真实数据）。
- 测试调整：get_disk_health 从 STILL_TAURI_ONLY 移入 R276 describe（+1
  断言：不带 tauriOnly + execute 函数）。
  （**700 总 / 43 files**；tsc/eslint/build 关）。

### 2026-10-17 只读汇总工具预览放行：清理建议/Steam 盘点/风扇状态（R275）

- R274 只放行了 path_size / inspect_dir 两个测量工具，但其余只读汇总工具
  （get_cleanup_suggestions / get_steam_library_summary / get_fan_status）仍
  标 tauriOnly 被整体隐藏——底层（cleanupSuggestions / STEAM_INVENTORY /
  fanStatus）都有 mock 演示数据源（与总览建议、Steam 盘点页、系统工具页
  同源）。预览模式 AI 问「这个目录能清多少」「Steam 占多大」「风扇什么
  状态」时无工具可用，只能凭记忆答。
- R275 修复：放行这三个只读汇总工具（预览模式用 mock 同源数据，只读
  无副作用）；写操作类（run_cli_tool / adjust_fan_curve / 安装/切换电源
  等）与底层无 mock 的（run_system_probe / list_top_processes /
  get_disk_health）保持 tauriOnly——预览不假装可写/有真实系统数据。
- 新增 10 测试（tools.test 34→44）：三个放行工具不带 tauriOnly；七个
  保持工具仍带 tauriOnly（写操作或底层无 mock）。
  （**700 总 / 43 files**——测试总数里程碑 700；tsc/eslint/build 关）。

### 2026-10-17 只读测量工具预览模式放行：AI 可核实路径大小/内容（R274）

- 缺口：advisor 的 path_size / inspect_dir（只读测量）标了 tauriOnly，在
  浏览器预览模式被整体隐藏——但底层 api.estimateSize / api.inspect 都有
  mock 演示数据支持。预览模式 AI 聊天里问「这个路径多大/里面有什么」
  时 AI 没有任何工具可核实，只能凭记忆猜，与「预览模式 AI 建议可跑通」
  的表达目标脱节。
- R274 修复：去掉这两个只读工具的 tauriOnly，预览模式放行。诚实性：
  它们是无副作用的只读测量（estimateSize mock 对树外返回 0、inspect mock
  只返回样例子项），不是假装写成功；写操作类工具仍保持 tauriOnly 拒绝。
- 新增 2 测试（tools.test 32→34）：path_size / inspect_dir 注册表不带
  tauriOnly、按 path 参数执行。
  （**690 总 / 43 files**；tsc/eslint/build 关）。

### 2026-10-17 清理页真删诚实化：root 下无命中目录不再假装删除成功（R273）

- 缺口：mocks.executeScope 真删时若 rootPath 下没有该 scope 的命中目录
  （清过的 scope 再执行 / root 不含该目录），mockRealScopePaths 返回空，
  但 mockScopeEntries 会退回**合成路径**（`root\scope\mock_*`）——不存在的
  路径被记入台账、计入释放字节、历史页显示「已回收虚拟路径」，与真实
  后端 execute_scope 无匹配文件时返回空（nothingMatched）语义脱节。
- R273 修复：真删且 realPaths 为空 → 直接返回空数组（调用方走
  nothingMatched，不假装「删了虚拟路径」）；dry-run 预览不受影响（仍返回
  样例清单，先看预览后执行才诚实）。重写 L677 冒烟测试改用真实命中
  rootPath（C:\Users\90740 + crash-dumps-local），并新增 R273 测试。
- 新增 1 测试（mocks.test 82→83）：真删无命中目录返回空数组（台账/
  磁盘卡不受影响）+ dry-run 预览仍有清单。
  （**688 总 / 43 files**；tsc/eslint/build 关）。

### 2026-10-17 右键回收诚实化：树外路径不假装删除成功（R272）

- 缺口：mocks.recyclePaths（R270 后）对任意传入路径都返回回收成功条目
  ——树外/已不存在的路径（聊天建议卡、AI 面板可能传入）也被记入台账、
  计成功，与 executeAiPlan（R267 树内校验、未知路径诚实跳过）口径不一致：
  「删了不存在的路径还说自己删了」。
- R272 修复：recyclePaths 只对树内节点 / 已知文件表命中的路径产生条目，
  树外路径自然过滤（调用方按返回条目数计成功、bytes_freed 计释放，未知
  路径如实计失败、不写台账、不动磁盘卡）。混合请求（真实 + 幽灵）只回收
  真实的。
- 新增 1 测试（mocks.test 81→82）：幽灵路径不产生条目（台账/磁盘卡不受
  影响）；真实 + 幽灵混合只回收真实的。
  （**687 总 / 43 files**；tsc/eslint/build 关）。

### 2026-10-17 AI 提案执行记账补全：磁盘卡/空间趋势联动下降（R271）

- 缺口：executeAiPlan mock（R267）真删只写台账 + 剪 mock 级回收台账，
  不记 sessionReclaimedBytes / reclaimedBySource——executeScope（R231）
  与 recyclePaths（R270）都有记账，AI 提案确认执行后磁盘卡 used 不下
  降，「清完了空间没回来」的观感脱节。
- R271 修复：executeAiPlan 真删时按实测 bytes_freed 累加会话台账 +
  per-source 入账（与另两条执行路径记账完全同构），磁盘卡 used 与
  空间趋势「今天」随 AI 提案确认执行下降；dry-run 不下账。
- 新增 1 测试（mocks.test 80→81）：AI 提案真删 CrashDumps → 磁盘卡 used
  下降该目录实测大小；再 dry-run 同路径不下账（used 不变）。
  （**686 总 / 43 files**；tsc/eslint/build 关）。

### 2026-10-17 右键回收记账补全：磁盘卡/空间趋势联动下降（R270）

- 缺口：recyclePaths mock（树右键「移入回收站」）只写台账 + 剪 mock 级
  回收台账（R268），不记 sessionReclaimedBytes / reclaimedBySource——
  executeScope（R231-R233）真删后磁盘卡 used 与空间趋势「今天」下降，
  右键回收后卡不降，「清完了空间没回来」的观感脱节。
- R270 修复：recyclePaths 真删时按树内 / 已知文件表实测 bytes_freed 累加
  会话台账 + per-source 入账（与 executeScope 同源），磁盘卡 used 与
  空间趋势随右键回收下降；还原（该 source 隔离条目）撤回对应入账回涨。
- 新增 1 测试（mocks.test 79→80）：右键回收 CrashDumps → 磁盘卡 used
  下降该目录实测大小 → 注入隔离条目 undo 还原 → used 回涨。
  （**685 总 / 43 files**；tsc/eslint/build 关）。

### 2026-10-17 确认窗逐项实测补树内真实大小：显示与执行记账同源（R269）

- 缺口：estimateSize mock 只查 MOCK_KNOWN_FILES 已知文件表，树内真实
  目录路径（如 AI 建议的 C:\Users\90740\AppData\Local\CrashDumps）返回
  0——确认窗显示「约 0 B」，但 executeAiPlan 真删时用树内大小非 0，
  「显示 0、执行释放 GB」脱节。
- R269 修复：estimateSize 未命中表时查树内真实节点（目录/file 叶子）大小
  ——与 executeAiPlan 记账（live 树）同源；已回收路径返回 0（刚清过再
  实测就没有了，与后端一致）；树外未知路径仍返回 0（诚实「路径不存在」）。
- 新增 2 测试（mocks.test 77→79）：①树内真实目录返回节点大小（显示 =
  执行记账）；②已回收路径再实测返回 0。
  （**684 总 / 43 files**；tsc/eslint/build 关）。

### 2026-10-17 右键回收与建议同源收敛：mock 级台账补齐（R268）

- 缺口：mocks.recyclePaths（工作台树右键「移入回收站」）只写 UNDO_LEDGER
  台账、不记 mock 级回收台账——前端 applyRecycledPrune 剪的是内存树，
  演示建议数据源（cleanupSuggestions/scopeSizesBatch）没剪，右键清完的
  目录总览「可清理」还建议（与 executeScope R225 / executeAiPlan R267
  不一致，后两者都记）。
- R268 修复：mocks.recyclePaths 成功后也 _mockRecyclePath 每条 source——
  三条执行路径（清理页 / AI 提案 / 右键回收）统一收敛，总览建议不再
  复活已右键回收的目录。
- 新增 2 测试（mocks.test 75→77）：①右键回收目录后 crash-dumps-local
  建议字节下降（CrashDumps 被剪，剩 WER/Minidump 兜底）；②右键回收写
  台账且 AI 提案 dry-run 不再返回该路径（双路径共享同一台账）。
  （**682 总 / 43 files**；tsc/eslint/build 关）。

### 2026-10-17 AI 提案执行补 mock：浏览器演示「AI 提案清理」真能清（R267）

- 缺口：预览模式 executeAiPlan 恒返回空数组——总览「重复文件查找 → AI
  提案确认窗」勾选后执行必全败（dry-run 恒空 → 逐项也空 → failed 全计），
  树不剪、台账不记、toast 全败；与 executeScope/recyclePaths 两条执行
  mock「真清了」不一致。
- R267 修复：mocks.executeAiPlan——dry-run 只返回可回收条目（树内真实存
  在 + MOCK_KNOWN_FILES 已知文件，剪过/已回收的排除）；真删写 UNDO_LEDGER
  台账 + mock 级回收台账（后续 scan/cleanupSuggestions/scopeSizesBatch 与
  前端剪树同源收敛），bytes_freed 用真实大小；未确认拒绝执行；危险路径
  （盘根/Windows/Program Files）与后端同口径不放行。api/clean.ts 接上。
- 新增 4 测试（mocks.test 71→75）：dry-run 不写台账；真删写台账 + 二次
  执行不再重复入账；未确认拒绝；未知/危险路径不产生条目。
  （**680 总 / 43 files**；tsc/eslint/build 关）。
- 浏览器实测：重复文件查找提案勾 5 项执行 →「已按你的确认回收 5 项
  （回收站可找回） · 约 1.19 GB」。

### 2026-10-17 总览「立即清理」记账改用实测字节：与清理页同口径（R266）

- 缺口：总览 doClean 记账用预览预估 it.bytes，而清理页 runRealDelete 用
  后端实测 bytes_freed——同一 scope 两条路径「已释放 X」显示不一致
  （days 过滤后实际删 vs 全量预估）。
- R266 修复：①记账改 entries.reduce(bytes_freed) 实测累加（L724 同口径）；
  ②bytes=0 时 toast 用 common.recycle.doneNoBytes 兜底，不再显示「释放约
  0 B」误导（与清理页/AI 提案同口径）。
- 浏览器实测：总览一键清理 17 项 →「已清理 17 项 · 释放约 58.1 GB（已进
  回收站）」。
- 无新增测试（组件闭包口径修复；tsc/eslint/build 把关）（**676 总 /
  43 files**）。

### 2026-10-17 AI 写工具权限未开时给「未授权」指引：不再说成“用户取消”（R265）

- 缺口：mcpTool 通用确认门把「权限未开」和「用户点取消」都回成「需要用户
  确认后执行，已取消」——用户去点确认也无济于事（权限没开，确认卡根本
  不弹）。fan_control / power_plan / cli.run 等专用路径早已区分。
- R265 修复：mcpTool 门先查 isPermEnabled——未开时回「调用 X 未授权：请
  前往「设置 → AI 权限中心」开启「控制系统状态」后重试」，不再误导。
  已开时才弹确认卡，取消才回「已取消」。
- 新增 3 测试（tools.test 29→32）：①process_kill 权限未开回「未授权」+
  权限中心指引；②file_recycle 未开也走指引（不是“用户取消”）；③权限已开
  时走确认门，取消回「已取消」且不含「未授权」。stub window.localStorage
  供 isPermEnabled 读，用 resolveSessionConfirm 模拟确认卡。
  （**676 总 / 43 files**）。

### 2026-10-17 电源计划切换加执行时权限门（R264）

- 缺口：PowerPlanCard.apply 直接 api.powerPlan(scheme, true) 写电源计划——
  执行函数无 power.plan 权限门（只有 UI disabled + 无权限不加载）。
- R264 修复：apply 开头加 isPermEnabled('power.plan')（复用
  optimizer.power.permOff 文案）——关闭时 setErr 拒绝。
- 无新增测试（4 行组件闭包门，同 R261-R263 模式；tsc/eslint/build 把关）
  （**673 总 / 43 files**）。

### 2026-10-17 风扇控制/恢复默认/驱动检查加执行时权限门（R263）

- 缺口：SystemTools.applyFanControl（写风扇档位）/ restoreDefaultFan（恢复
  默认曲线）/ checkWuUpdates（联网查驱动更新）执行函数无门——只有 UI 层
  fanCtrlPerm/driverPerm 条件渲染。fan.control/fan.adjust/driver.check 均 L2
  高危，UI 之外不拦。
- R263 修复：三函数开头加对应 isPermEnabled 门（复用 system.fan.permOff /
  system.driver.permOff 文案）——关闭时 setErr 拒绝。
- 无新增测试（4 行组件闭包门，同 R261/R262 模式；tsc/eslint/build 把关）
  （**673 总 / 43 files**）。

### 2026-10-17 MCP 服务器新增/JSON 导入加执行时权限门（R262）

- 缺口：McpServers 的 update/remove 已有 manageOk 门，但 save（新增 addServer）
  和 importJson（批量 JSON 导入）执行函数无门——mcp.manage 是 L2 高危
  （新增服务器 = 授权外部连接），UI disabled 之外不拦。
- R262 修复：save / importJson 开头加 isPermEnabled('mcp.manage')（复用
  mcp.needPermTitle）——关闭时 setErr 拒绝写入。
- 无新增测试（4 行组件闭包门，同 R261 模式；tsc/eslint/build 把关）
  （**673 总 / 43 files**）。

### 2026-10-17 启动项启停/删除加执行时权限门：不只靠 UI 禁用（R261）

- 缺口：StartupPanel.toggleStartup / removeStartup 只靠 UI 禁用（!startupPerm
  时按钮 disabled）——执行函数本身无门。startup.manage 是 L2 高危（改注册表 /
  删启动项），权限在列表加载后被关（或绕过 UI 调用）时仍能执行。
- R261 修复：两个执行函数开头加 isPermEnabled('startup.manage')（复用
  system.startup.permOff 文案）——执行时拦截，与 Studio.install /
  api.system-ext（plugin.manage）同风格。
- 盘点确认各 L2 写路径均带门：startup.manage（本修复）/ plugin.manage（api 层）
  / scaffold.manage（Studio 执行时）/ mcp.manage（McpServers）/ fan.adjust、
  fan.control（SystemTools + advisor）/ driver.check / power.plan（PowerPlanCard
  + advisor）/ cli.run（advisor）。
- 无新增测试（门为 4 行组件闭包逻辑，与 Studio.install 已覆盖模式同款；
  tsc/eslint/build 把关）（**673 总 / 43 files**）。

### 2026-10-17 总览「立即清理」加权限门：cleanup.execute 关闭时拒绝（R260）

- 缺口：AssetOverview.doClean（总览一键清理）直接 api.executeScope 真删——
  绕过清理页 R257 已加的 cleanup.execute 权限门。权限中心关掉后总览仍能静默
  真删。
- R260 修复：doClean 开头加 isPermEnabled('cleanup.execute')（同 R257 文案
  errors.permExecuteMissing）——关闭时 toast 拒绝，不做任何执行。
- 排查确认全部真删入口现已统一带门：总览 doClean / 清理页 runRealDelete /
  AI 提案（cleanup.execute）；右键与 AI 面板回收（file.recycle）。
- 无新增测试（门逻辑同 R257 已覆盖；组件接线由 tsc/eslint/build 把关）
  （**673 总 / 43 files**）。
- 浏览器实测：cleanup.execute 关闭 → 总览点「立即清理」→ toast「清理执行
  未授权：请前往「设置 → AI 权限中心」开启「执行清理计划」后再试」。

### 2026-10-16 AI 面板回收走 store 权限门：file.recycle 关闭时拒绝（R259）

- 缺口：ChatPanel.confirmRecycle 直接调 api.recyclePaths——绕过 store.
  recyclePaths 的 file.recycle 权限门（权限中心关掉后 AI 面板「确认回收」仍
  能真删），且删完不剪树（节点残留显示已删文件）、不 bumpScanSeq（台账读者
  收不到信号）。
- R259 修复：confirmRecycle 改走 store.recyclePaths（同一权限门 + 剪树 +
  bump + 实测记账 + toast），聊天气泡保留回收结果文案；done=0（权限被拒/
  全失败）时聊天里回 permExecuteMissing 原文，不假装成功。
- 排查确认其余真删入口均已带门：TreeView/FileView/SteamInspector/App 都走
  store.recyclePaths，清理页/AI 提案走 cleanup.execute 门（R257）。
- 无新增测试（store 门 R256/R258 已覆盖；组件接线由 tsc/eslint/build 把关）
  （**673 总 / 43 files**）。

### 2026-10-16 右键回收加权限门：file.recycle 关闭时拒绝（R258）

- 缺口：recyclePaths（工作台右键「移入回收站」）直接 recycle_paths，没有
  file.recycle 权限门（L2 高危默认关）——权限中心关掉后仍能真删，与 R257
  清理页同类的漏网入口。
- R258 修复：recyclePaths 开头加 isPermEnabled('file.recycle')，关闭时
  toast 指引权限中心 + 返回全失败，不剪树不记账。至此三个执行入口全有门：
  清理页 runRealDelete（cleanup.execute）、AI 提案 aiRecyclePaths
  （cleanup.execute）、右键 recyclePaths（file.recycle）。
- 新增 1 测试（file.recycle 关闭 → 拒绝，api 不被调、树不剪、scanSeq 不变；
  store.test 预置双权限开启，既有剪枝测试回归全绿）（**673 总 / 43 files**）。
- 浏览器确认 file.recycle 默认即关（权限表 L2 语义正确）。

### 2026-10-16 清理页真删加权限门：cleanup.execute 关闭时拒绝执行（R257）

- 缺口：aiRecyclePaths 有 cleanup.execute 权限门（L2 高危默认关），清理页
  runRealDelete 却直接 executeScope——权限中心关掉「执行清理计划」后清理页
  仍能真删，权限表形同虚设。
- R257 修复：runRealDelete 开头加 isPermEnabled('cleanup.execute') 权限门
  （与 aiRecyclePaths 同文案 errors.permExecuteMissing）——关闭时拒绝执行、
  清空预览与预备态，绝不静默执行。排查还确认 recyclePaths（右键回收）走
  file.recycle 门（L2 默认关）一致语义。
- 新增 2 测试（权限关闭→拒绝且不调 executeScope；权限开启→正常执行回归；
  测试 stub window.localStorage + 预置权限）
  （**672 总 / 43 files**）。

### 2026-10-16 剪枝统一 bump scanSeq：右键回收 / AI 提案也刷新脚本卡徽标（R256）

- 缺口：applyRecycledPrune（剪树）只在 pruneRecycled（清理页）路径后 bump
  scanSeq；recyclePaths（右键回收）和 aiRecyclePaths（AI 提案执行）剪树后
  不 bump——台账已追加记录，但依赖 scanSeq 的读者（清理页脚本卡「上次清理」
  徽标 / 总览 loadClean）收不到信号，卡在旧状态。
- R256 修复：bumpScanSeq 移进 applyRecycledPrune 内部——三条清理写路径
  统一递增 scanSeq，信号全面覆盖。
- 新增 3 测试（recyclePaths / aiRecyclePaths / pruneRecycled 剪枝后 scanSeq
  递增；aiRecyclePaths 测试预置 cleanup.execute 权限 + stub window.localStorage）
  （**670 总 / 43 files**）。

### 2026-10-16 清理页命中状态 chips 计数跟随筛选：不再与列表脱节（R255）

- 缺口：chips（全部/已命中/未检测到）计数基于全量 cards——搜索「微信」后
  列表只剩 1 张卡，chips 仍显示「已命中 20」，计数与列表脱节（误导还有更多
  没显示）。
- R255 修复：chipCounts 纯函数按当前筛选集（filtered）计数——搜索/状态筛选
  后 chips 数字即时跟随；chips 加数字徽标（.cp-status-chip-n 弱化样式）。
- 新增 4 测试（全命中 / 混有未命中 / 全未命中 / 空列表）
  （**667 总 / 43 files**）。
- 浏览器实测：搜「微信」→ chips 21/20/1 → 1/1/0，列表 1 卡；清空恢复。

### 2026-10-16 总览建议清理勾选跨重拉保留：取消的项不再被全勾回来（R254）

- 缺口：总览每次 loadClean 都 setChecked(全 true)——用户手动取消某建议项
  （如微信）后，清理/切盘/重扫一次就全勾回来，可能误清用户主动排除的项。
- R254 修复：mergeCheckedState 合并勾选——重拉后新 key 默认勾选、已有 key
  保留原选择、消失的 key 自然失效（与清理页 cancelPreview 保留勾选的语义
  对齐）。
- 新增 4 测试（空勾选全勾 / 保留手动取消 / 消失 key 移除 / 纯函数不改原对象）
  （**663 总 / 43 files**）。
- 浏览器实测：17 行全勾 → 取消 1 → 16 勾；重扫后仍 16 勾（之前恢复 17 全选）。

### 2026-10-16 历史页搜索对脏数据防御：null source 不再整页崩（R253）

- 缺口：filterUndoEntries 对 source/destination/reason 直接 .toLowerCase()——
  后端若返回 null / 缺字段（JSON 边界脏数据），会抛 TypeError 让整个历史页崩。
- R253 修复：搜索字段走 String() 兜底——非字符串按空串参与匹配，关键词
  搜索只是过滤，不该因脏数据炸页面。
- 新增 1 测试（null source / 缺 destination / 缺 reason 不抛 TypeError，
  关键词照常命中干净项）（**659 总 / 43 files**）。

### 2026-10-16 清理页 preflight 横幅可手动重跑：退出进程后再点「重新检测」（R252）

- 缺口：preflight（「以下进程还在运行，建议先退出再清理」横幅）只在进脚本
  时跑一次——用户在任务管理器退出占用进程后，横幅一直残留到换脚本。
- R252 修复：横幅加「重新检测」按钮（recheckPreflight）——重测中先收起旧
  横幅避免闪「旧的还在跑」，回填最新结果；占用进程已退出横幅即时消失。
- 新增 3 测试（收起旧横幅回填新结果 / 进程仍在保留横幅 / 无会话不动作）
  （**658 总 / 43 files**）。
- 预览模式 mock 的 preflight 恒返回空进程，横幅不可见；行为由单测守护。

### 2026-10-16 保留天数输入边界 sanitize：超长/负数不再污染持久化（R251）

- 缺口：清理页 scope 行「保留天数」输入无上限——输 999999 天会被
  persistScopeDays 永久入库（之后每次会话默认值被带偏），且按「保留近
  999999 天」算可清范围几乎为空（误导「没得清」）。
- R251 修复：sanitizeRetentionDays（model.ts 纯函数）——负值/非数 → 0，
  超过 3650 天（10 年）→ 3650。setDays 统一走 sanitize。
- 新增 5 测试（正常/负值/超限截断/非有限数/小数取整）
  （**655 总 / 43 files**）。
- 浏览器实测：输 999999 → 3650；输 -5 → 0；恢复 30 正常。

### 2026-10-16 清理页搜索/筛选后自动改选匹配脚本：左右不再错位（R250）

- 缺口：清理页搜索或命中状态筛选后，左侧脚本列表过滤了，但右侧主体仍
  显示筛选前的选中脚本——「左边只有微信、右边还在清安全软件套装」的
  左右不一致。
- R250 修复：选中脚本不在 filtered 集时自动改选 filtered 首个（与
  tagFilter 进页语义一致）；filtered 空（搜索无结果/筛选空）保留原选中
  不打断，清空搜索即恢复。
- 浏览器实测：搜「微信」→ 左侧 21 卡缩成 1 卡 + 右侧自动改选 WeChat；
  清空恢复 21 卡。

### 2026-10-16 总览预览缓存随建议项剪枝：清过的项不再残留陈旧 dry-run（R249）

- 缺口：总览建议项展开预览（items + R244 上次清理徽标）缓存在 previews
  里，清理后建议项消失/重拉，但预览缓存不剪——还原恢复重现时展示的
  是上一轮的陈旧 dry-run（旧条目/旧徽标，误导）。
- R249 修复：loadClean 重拉后按存活 key 剪掉 previews/previewOpen 里消失
  项的缓存（pruneStalePreviews 纯函数，无变化返回原对象免重渲染）。
- 新增 4 测试（剪掉消失项 / 全存活引用不变 / 空缓存 / 消失项被剪）
  （**650 总 / 43 files**）。
- 浏览器实测：清理流程正常（两步执行、按钮态恢复、建议项随预剪）。

### 2026-10-16 空间大头 Top 10 行点击定位到该目录（R248）

- 缺口：总览底部「空间大头 Top 10」每行点击只跳工作台（onGoWorkspace()）
  ——落在盘根，用户还得自己找那个大头目录在哪，定位价值为零。
- R248 修复：Top 10 行带目录 path（topDirs 加 path 字段）；App 侧
  onGoWorkspace 区分盘根/子目录——子目录时 `driveRootOf(p)` 还原盘根扫描，
  扫完 `selectPath(p)` 定位（FileView 按 selectedPath 逐段下钻，无需节点
  引用）。新增纯函数 driveRootOf（format.ts，含 UNC 处理）+ 4 测试
  （**646 总 / 43 files**）。
- 浏览器实测：点 Top10「Users」行 → 工作台树 Users 节点高亮选中
  （67.0 GB / 402.3k），一点即达。

### 2026-10-16 筛选空态归因修复：空搜索词不再报「没有匹配「」的脚本」（R247）

- 缺口：脚本列表的搜索与命中状态筛选叠加后，筛选（非搜索）导致空列表时
  仍显示「没有匹配「{q}」的脚本」——q 为空时文案荒谬（「没有匹配「」」）。
- R247 修复：q 非空 → 搜索空文案；q 空 → 新增「当前筛选下没有脚本」。
  两种归因不混淆。
- 浏览器实测：筛「已命中」+ 空词 →「当前筛选下没有脚本」；搜「zzzzzz」
  →「没有匹配「zzzzzz」的脚本」；清空恢复 21 卡。

### 2026-10-16 清理后脚本卡「上次清理」徽标即时刷新（R246）

- 缺口：R241 脚本卡徽标只在挂载/allCards 变化时拉台账——用户清完一个
  脚本，回清理页列表徽标不更新（要刷新页面才见「今天」）。
- R246 修复：lastCleanTs effect 依赖加 store.scanSeq——清理执行成功后
  store.pruneRecycled → bumpScanSeq（R222 链路），台账重拉，徽标即时
  变「今天」。
- 浏览器实测：清 Firefox 缓存后脚本卡即时显示「Firefox 717 MB 上次清理
  今天」（1.20GB → 717MB），无需刷新。

### 2026-10-16 「一键智能清理」进清理页：点「清理」不再跳「浏览」（R245）

- 缺口：总览快速操作「一键智能清理」（ai）此前只跳工作台——用户点
  「清理」却进「浏览」，动作语义与落点脱节（junk 已正确进清理页）。
- R245 修复：ai 与 junk 一样走 onOpenCleanup——清理页有脚本列表 + 全选 +
  保留期 + 两步预览，是「一键清」的正确落点。
- 浏览器实测：总览点「一键智能清理」→ 清理页（active=清理，21 脚本）。

### 2026-10-16 总览预览弹窗「上次清理」徽标：刚清过的目录一眼可见（R244）

- 缺口：总览建议项展开预览（ScopePreview）只显示文件清单/合计，没有
  「这个目录上次清过多久」——清理页 scope 行有 lastCleanByPath，总览
  预览没有，两处台账视角不一致。
- R244 新增：建议项加 path 字段（cleanupSuggestions 的 hitPath 命中目录），
  togglePreview 时用 latestCleanTs(台账, path) 聚合「上次清理时刻」，预览
  头部显示「上次清理 今天/N 天前」（formatLastActive 同源文案）。台账
  读失败或无记录不显示（诚实，读失败 ≠ 真没清过）。
- 新增纯函数 latestCleanTs（model.ts）+ 3 测试（目录下最新时刻/目录本身
  被清/无匹配 null）（**642 总 / 43 files**）。
- 浏览器实测：未清过的建议项预览无徽标（诚实）；清过的建议项从列表
  消失（R225 修剪语义）——总览建议项在预览模式清完即消失，徽标在真实
  后端「部分清理后目录仍在建议项里」时显示。

### 2026-10-16 清理预览条目数与释放字节同源：进度不再与合计脱节（R243）

- 缺口：executeScope 的 dry-run 分片量 n 用哈希 mockScopeBytes 兜底——
  12GB 目录也可能只分 4 条进度，预览弹窗「36/64 条」与「共 414 MB」
  脱节（进度条总数与实际释放量不匹配）。
- R243 修复：n 与「真正会释放的字节」同源（days 缩放后的真实字节，
  每 ~24MB 一条）——进度条总数、释放合计、空间回落三者同源。
- 新测试：dry-run 条目数与释放总量成比例（每条约 24MB，≤64 条上限）
  （**639 总 / 43 files**）。
- 浏览器实测：CrashDumps 1.35GB → 预览「60 个文件 · 共 414 MB」，
  60 条 × 24MB ≈ 414MB，与释放字节一致。

### 2026-10-16 还原隔离文件后磁盘卡/趋势联动刷新（R242）

- 缺口：UndoPanel / 历史页还原隔离条目后只重拉台账——磁盘卡/趋势依赖
  reclaimedBytes 驱动（App 订阅），还原后空间回涨（真实后端）或台账变化
  时磁盘卡不刷新，UI 与后端状态脱节。
- R242 修复：两处还原成功路径调 store.bumpScanSeq()——scanSeq 驱动
  listDrives 重拉（App L270-273 订阅）；真实后端隔离还原后空间回涨由
  reclaimedBytes 订阅覆盖，种子未入账条目（R233 语义）空间不变但磁盘卡
  也反映最新值。
- 浏览器实测：历史页还原「旧报告.docx」（隔离种子条目）→ 无崩溃、条目
  移除、总览正常渲染（可清理 40.2 GB 不回涨，符合 R233 种子不入账设计）。

### 2026-10-16 清理页脚本卡「上次清理」徽标：刚清过的脚本一眼可见（R241）

- 缺口：脚本卡只显示名称/风险/体量，用户进清理页不知道哪些脚本刚清过
  （scope 行的 lastCleanByPath 只在选中脚本详情里，脚本列表没有全局视角）。
- R241 新增：清理页拉操作台账（listUndo），把每条 source 按命中目录前缀
  归属到脚本卡（entriesScaffold），聚合每卡最新清理时刻，卡片下方显示
  「上次清理 今天 / N 天前」（formatLastActive 同源文案）。台账空/读失败
  无徽标（诚实，不假装清过）。
- 新测试（CleanupPage.test.tsx）：目录本身/目录下文件/子目录命中该卡、
  同名前缀不误配、未命中 null、大小写归一（**638 总 / 43 files**）。
- 浏览器实测：清「浏览器缓存」后该卡变「307 MB 上次清理 今天」，其余
  未清脚本无徽标。

### 2026-10-16 总览可清理卡「全选 / 清空」一键切换（R240）

- 缺口：总览可清理卡底部只有「立即清理」——默认全勾（R126），用户取消
  部分项后想重新全选或全部清空得逐项点勾选框；批量勾选是清理场景的
  高频操作。
- R240 新增：底部操作栏「全选 / 清空」toggle 按钮——当前全选时显示
  「清空」（一键取消全部），有未选项时显示「全选」（一键补齐）；点击
  同时解除 arming（避免误触发两步清理）。微信项选中仍走底部微信引导。
- i18n 双语键 overview.clean.selectAll / clearAll（CN+EN 对齐）。
- 浏览器实测：全选 → 清空（按钮变「全选」）→ 全选（按钮变回「清空」），
  勾选状态正确切换。

### 2026-10-16 空间耗尽预警与设置阈值联动：不再硬编码 30 天（R239）

- 缺口：checkSpaceForecast 的 warn 硬编码 `days < 30`——用户把预警阈值
  设成 3/7/14/30 天时，mock 判定与设置页脱节（API 契约写明「days ≤ 预警
  阈值，未设阈值恒 false」，mock 却不读配置）。
- R239 修复：warn 读 reminderCfg.full_warn_days（0=关恒不预警）；设了阈值
  才按 `days ≤ threshold` 判。与真实后端 space-full 提醒逐盘判语义一致。
- 新测试：0=关恒不 warn；阈值 200 天（>92 初始外推）触发预警；阈值 10 天
  不预警（**632 总 / 42 files**）。
- 浏览器实测：设置预警阈值 30 天 → C 盘约 92 天满 → 无 ⚠ 预警（诚实：
  92 > 30 不触发，配置联动生效）。

### 2026-10-16 Conda 环境区与回收台账同源：删掉的环境不再列出（R238）

- 缺口：listCondaEnvs 静态返回 base + tf2 + web——清理页 envs-stale 真删
  掉 tf2 后，删除完成时刷新环境区仍列出已删环境（真实桌面端 conda env
  删了枚举就没了）。
- R238 修复：listCondaEnvs 过滤回收台账——已回收路径（含其目录下文件）
  的环境不再返回；base 永不被清、始终保留。
- 新测试：_mockRecyclePath 后 tf2 不再列出、base + web 保留（**631 总 /
  42 files**）。
- 浏览器实测：删 tf2 前环境区含 envs 8.90GB（tf2 6.80GB）；删后 envs 子树
  消失、环境区只剩 pkgs，tf2 不再列出。

### 2026-10-16 重复文件查找尊重选中盘 + 回收台账：重扫不列已清文件（R237）

- 缺口：scanDuplicateFiles 忽略 rootPath——扫 D 盘仍返回 C 盘重复组；
  静态组也不感知回收台账，清理页清掉目录后重扫仍列已删文件（真实后端
  重复查找读当前磁盘，已回收路径不会再出现）。
- R237 修复：
  - 只扫选中盘：rootPath 前缀外的文件不返回（D 盘扫描 = 空，不编造）；
  - 与回收台账同源：已回收路径（含其目录下的文件）从重复组剪掉，剪完
    不足 2 份的组整组丢弃（只剩 1 份就不是重复）；keep 落在已回收路径
    时自动回落到剩余文件。
- 新测试：D 盘扫描空、回收候选文件后该组消失且他组不受影响（**630 总 /
  42 files**）。
- 浏览器实测：总览重复文件查找 C 盘弹出 5 项约 1.19GB 可回收（installer
  ×2 + meeting ×2 + report ×1），确认窗正常。

### 2026-10-16 总览可清理合计收敛：限定盘内命中目录，不再虚高（R236）

- 缺口（R235 暴露）：总览「可清理」卡片合计虚高到 133GB（占磁盘 71%）——
  每脚本挑前 2 个 cache/media scope × 全全局字节叠加（浏览器 38GB + Conda
  36GB + WeChat 22GB …），周报/提醒共享这个失真合计。
- R236 修复：cleanupSuggestions 在盘根口径（总览选中盘，如 C:\）下给
  scopeRealBytes 传 rootPath 过滤——建议字节限定在该盘命中目录内，多目录
  脚本各目录都算但都在盘内；盘外 scope 返回 0 跳过。深路径调用（清理页
  等）保持旧口径不受影响。
- 新测试：C 盘根口径合计 < 100GB 且每项 > 0、crash-dumps 建议合计 ≤
  scopeSizesBatch 盘根合计（不翻倍）（**628 总 / 42 files**）。
- 浏览器实测：总览可清理从 133GB（虚高）收敛到 **40.2GB**（C 盘命中目录
  真实合计）。

### 2026-10-16 周报/提醒可清字节与建议同源：清理后周报环比如实下降（R235）

- 缺口：周报与清理提醒的可清字节固定 2.91GB——清理后周报「全盘可清理
  2.91GB」不变，环比 delta 失真（固定样例 vs 计算值）。
- R235 修复：
  - generateWeeklyReport / runReminderCheck 的可清字节 = cleanupSuggestions
    建议合计（rootPath='C:\' 与总览选中盘一致）——清理后周报/提醒随之下降；
  - getWeeklyReports 首次的上周样例也用建议基线——清理前 delta≈0，清理后
    本周下降、delta = 本次清理量（不再「比上周多 130 GB」的失真环比）。
- 测试更新：周报/提醒可清字节 === 建议合计（**627 总 / 42 files**）。
- 浏览器实测：基线「全盘可清理约 144 GB」→ 清 CrashDumps 后生成周报
  环比「可清理量比上周少 138 MB」，清理效果如实入账。

### 2026-10-16 空间外推联动磁盘：清理释放后「还能用 X 天」自动增大（R234）

- 缺口（R231/R232 遗留）：checkSpaceForecast 恒定「112 天 / ok」——清理
  后磁盘降了，外推天数却不动，清理提醒「还能用 112 天」是一成不变的死
  数字。
- R234 修复：外推天数与磁盘当前值同源——用 listDrives 同一口径（used
  随会话释放台账下压），按趋势卡同源日增（0.42GB/天）外推满盘天数。
  清理后磁盘降 → 剩余空间增大 → 天数增大；≤30 天时 warn 告警。
- 新测试：真删后满盘天数增大（释放 0.276GB / 日增 0.42GB ≈ +0.66 天）
  （**626 总 / 42 files**）。
- 浏览器实测：清理 CrashDumps 前「约 92 天后满」→ 清理后「约 94 天后
  满」，外推随释放联动。

### 2026-10-16 还原空间回涨：只撤回该 source 实际入账过的释放（R233）

- 缺口（R231/R232 遗留）：隔离条目 undo 后磁盘卡/空间趋势不回涨——演示
  里「还原了但空间没回来」。但直接减条目 bytes_freed 会误伤：种子隔离
  条目（旧报告.docx 3.2GB）从没入账过会话台账，clamp 到 0 会把别的清理
  释放凭空回涨（磁盘卡跳满）。
- R233 修复：
  - executeScope 真删时按 source 记 per-source 入账（reclaimedBySource）；
  - mock undo() 还原隔离条目时只撤回该 source 实际入账过的部分——真实
    链路还原后 used 回涨；种子条目（未入账）还原不回涨，保持诚实。
- 新测试：还原后磁盘回涨 ≈入账量、未入账的种子条目还原不改变磁盘
  （**625 总 / 42 files**）。
- 浏览器实测：恢复「旧报告.docx」→ 提示已恢复，磁盘保持 148 GB 不变
  （种子条目未入账，不回涨、不误伤）。

### 2026-10-16 释放入账按实际删除量：预览/释放/磁盘三处口径一致（R232）

- 缺口（R231 遗留）：executeScope 不传 olderThanDays 时释放字节按全量
  （0.9GB）入账，而清理页「待清」显示 days 缩放后 276MB——「预览 276MB、
  磁盘却降 0.9GB」的口径错位。
- R232 修复：
  - executeScope 的 olderThanDays 未显式传时取 scope.prompt.default——
    执行释放字节与清理页「待清」（scopeSizesBatch 同源缩放）一致；
  - sessionReclaimedBytes 按执行条目 bytes_freed 和累加（= 实际删的量），
    总览 toast「已释放 X」、空间趋势今天回落、磁盘卡下降三处同源。
- 测试更新：空间趋势与 listDrives 断言改为「回落 ≈ 276MB（0.9GB×0.3），
  远小于整目录全量」（**623 总 / 42 files**）。
- 浏览器实测：预览「12 个文件 · 共 276 MB」→ 清理后磁盘卡 -276MB
  （可用 39.0→39.5 GB），与预览释放量一致。

### 2026-10-16 清理释放入账磁盘：磁盘卡/空间趋势随清理同源下降（R231）

- 缺口：空间历史曲线是固定脚本（14 天爬升 + 一次 2GB 回落），listDrives
  只在挂载时拉一次——演示里「刚清完 276MB，磁盘卡与趋势纹丝不动」，
  清理释放量没有入账到磁盘展示。
- R231 修复：
  - mock 模块持会话级释放台账 `sessionReclaimedBytes`：executeScope 真删
    累加（dry-run 不动），`getSpaceHistory` 今天末点与 `listDrives` used
    同源下压——曲线今天回落、磁盘卡下降，清完再看两处一致；
  - `App.tsx` listDrives 跟随 `scanSeq + reclaimedBytes` 重拉：清理后自动
    刷新磁盘条（缓存命中不 bump scanSeq，故用 reclaimedBytes 驱动）。
- 新测试：真删后空间趋势今天末点回落 ≈0.9GB、listDrives used 下降
  （**623 总 / 42 files**）。
- 浏览器实测：清理 CrashDumps 前「可用 39.0 GB · 已用 148 GB」→ 清理后
  自动变「可用 40.8 GB · 已用 146 GB」，无需手动重扫。

### 2026-10-16 还原回补扫描树：还原与清理互逆（R230）

- 缺口（R225 遗留）：mock 级回收台账 `recycledPaths` 只进不出——真删
  剪掉目录后，即使隔离条目还原成功，扫描树里该目录也回不来（演示上
  「还原了但总览还缺这个目录」）。
- R230 修复：mock `undo()` 还原 quarantine 条目成功时，把 source 从
  mock 级回收台账移除——被还原的目录回到 scan / cleanupSuggestions /
  scopeSizesBatch 的树里；新增测试 hook `_mockPushUndoEntry` 注入隔离
  条目覆盖该语义（预览模式 scaffolds 全是 recycle mode，无法自然生成
  隔离条目，测试注入以验证「还原 = 目录回补」）。
- 新测试：还原后 crash-dumps-local 建议字节增长（CrashDumps 0.9GB 回补）
  （**621 总 / 42 files**）。
- 浏览器实测：历史页隔离条目点「恢复」→ 提示「已恢复：旧报告.docx」
  → 台账条目消失，还原链路完整。

### 2026-10-16 执行只清所选 scope 目录：不误伤同脚本其它目录（R229）

- 缺口（R228 遗留）：executeScope 真删剪枝用 mockRealScopePaths 收集
  **整个 scaffold** 的命中目录（CrashDumps/WER/Minidump 全算），清
  「应用崩溃转储」会把 WER / Minidump 一起剪掉——演示里执行一个
  scope 目录，另外两个目录的字节也被清空。
- R229 修复：mockRealScopePaths 按 scopeId 过滤，只收集该 scope glob
  目录名匹配的节点（brace/通配段跳过）——清 WER 不再误伤
  CrashDumps/Minidump；R225 相关测试改用真实 scopeId
  （crash-dumps-local）并新增「清一个目录其余目录仍可清」断言。
- 浏览器实测：清「应用崩溃转储」后该行显示「空」，WER（320MB）与
  Minidump（140MB）两行字节不受影响（**620 总 / 42 files**）。

### 2026-10-16 多目录脚本每行真实字节：不翻倍不截断（R228）

- 缺口（R227 遗留）：crash-dumps 的三个 scope 行此前共享同一总量
  （都显示 922 MB），且多命中目录经 aggregateScopeSizes 累加会翻倍
  （浏览器实测三行都是 1.80 GB）；WER 目录被外层 windows-privacy
  的 Windows 节点截断，detect 目标命中不了。
- R228 修复：
  - MOCK_TREE 补 WER（320MB）与 Minidump（140MB）真实节点；
  - `scopeRealBytes` 按 rootPath 过滤、按 glob 目录名绑定 scope 行——
    每个 scope 只算自己目录的字节（CrashDumps/WER/Minidump 各一行）；
    brace/通配段不参与目录名匹配（WER 的 `{ReportQueue,ReportArchive}`）；
  - `collectScaffoldCards` 命中后仍下钻（嵌套脚本目录不再被截断），
    每目录只算给自己的脚本；
  - `scopeSizesBatch` 的 sizes 移入 rootPaths.map 内，每个命中目录
    各算各的，聚合不再翻倍。
- 新测试：多 root_path 各算各的聚合不翻倍、C:\Windows 下只算 Minidump、
  crash-dumps 命中 WER/Minidump 不截断（**620 总 / 42 files**）。
- 浏览器实测：脚本行 1.35 GB = CrashDumps 0.9 + WER 320MB + Minidump
  140MB 精确同源；三行各显示自己目录真实字节与天数缩放待清量。

### 2026-10-16 清理字节口径同源：全部来自扫描树真实命中（R227）

- 缺口：清理页 scope 行 / 总览建议 / 执行释放的字节此前走哈希公式
  （`mockScopeBytes`），与工作台树里目录的真实 size 各说各话——
  「工作台 0.9 GB / 清理页 75 MB / 总览 180 MB」数字对不上。
- R227 收敛：`scopeSizesBatch` / `cleanupSuggestions` / `executeScope`
  的字节优先取 **MOCK_TREE 修剪后命中节点的真实 size**（哈希只作
  未命中兜底）；`total_bytes` 显示目录全量、`bytes` 显示按保留天数
  过滤后的可清量（R212 天数缩放保留，语义与真实后端 mtime 过滤一致）。
- 新测试：scope 行字节 ≤ 树里真实命中总量（且不缩到 0），total ≥ bytes
  （**617 总 / 42 files**）。
- 浏览器实测：崩溃转储脚本行 922 MB（= 树真实 0.9 GB），三个 scope 行
  「共 922 MB · 待清 276 MB」——全量/可清/总览建议同源一致。

### 2026-10-16 剪枝落盘：刷新后不再复活已删目录（R226）

- 缺口：R222/R224 的 `applyRecycledPrune` 只改内存 scanCache，剪过的树
  **不同步 IndexedDB**——刷新后 `takeDrive` 命中持久化旧树，已回收目录
  「复活」。真实后端无此问题（每次扫描走磁盘），mock 演示必须一致。
- 现在 `applyRecycledPrune` 在缓存树有变化时同步 `persistScanCacheIdb`，
  剪完立即落盘，刷新后不再出现已删目录。
- 新增单测：`recyclePaths` 剪枝后，fake IDB 里持久化树已不含被剪路径
  （**616 总 / 42 files**）。
- 浏览器实测：清崩溃转储后直读 IDB `scanTrees`，持久化树已不含
  CrashDumps（C:\Users 等其余目录保留）。

### 2026-10-16 预览模式清理闭环：真实路径回收 + 剪枝同源收敛（R225）

- 缺口：mock `executeScope` 真删返回的 source 是合成路径
  （`{root}\{scope}\mock_..._chunk_N`），与扫描树不同源——R222/R224 的
  `pruneRecycled` 按 source 剪 root/scanCache 是空操作，演示里「清完
  再看」各处体量不变。
- R225 三处收敛：
  - mock 真删 source 改为 **MOCK_TREE 真实命中节点路径**（与扫描同源）；
  - mock 级「已回收路径」台账 + 修剪函数：`scan` / `cleanupSuggestions`
    从剪过再克隆的树返回——总览「可清理」建议项、工作台体量在真删后
    与 store 前端剪枝同源收敛，不再复活已删目录；
  - dry-run 保持合成清单（预览展示不受影响）。
- 新测试：真删 source 对应树里真实 crash-dumps 节点；回收后
  cleanupSuggestions 不再建议已清目录（**615 总 / 42 files**）。
- 浏览器实测：清崩溃转储后回总览，「应用崩溃转储」建议项与分类条
  全部消失（此前常驻显示 75.1 MB）。

### 2026-10-16 总览页「立即清理」后跨页同步剪树（R224）

- R222 只补了清理页 runRealDelete——总览页 doClean「立即清理」执行成功后
  同样只加 reclaimed、不剪 root/scanCache，工作台/分类占用仍旧值。
- 现在 doClean 在批循环里收集成功条目的 source 路径，执行后调
  `useStore.getState().pruneRecycled(paths)`（与 R222 同一批剪枝语义）。
- 无新测试（复用 R222 的 pruneRecycled 测试；现有 **613 全绿**）。

### 2026-10-16 清理完成提示附「查看历史」入口（R223）

- 清理页执行成功后，成功提示区附「查看历史」按钮——清理完想还原误删
  时不再需要自己去左侧导航找历史页，一步直达（历史页每条记录可恢复）。
- CleanupPage 新增可选 `onOpenHistory` prop 贯通到 CleanupBody（App.tsx
  传入 `openHistory`，普通清理页与隐私清理页共用）；新增 i18n
  `cleanup.gotoHistory`（CN/EN）。
- 浏览器实测：勾选崩溃转储 3 个 scope → 预备 → 预览 → 双重确认 →
  提示「已清理 12 个文件 · 约 186 MB（预估）· 进了系统回收站 查看历史」，
  点「查看历史」落到操作历史页（15 条记录）。
- 无新测试（纯组件接线；现有 613 全绿）。**613 总 / 42 files。**

### 2026-10-16 清理页执行后跨页同步剪树（R222）

- 缺口：清理页 runRealDelete 成功后只重测当前脚本 scope 大小并记
  「已回收」，**没有把成功回收的路径从 store.root / scanCache 剪掉**——
  回总览/工作台时，分类占用、建议清理体量仍是旧值，看起来像「没清掉」。
- 现在 runRealDelete 在批循环里收集成功条目的 source 路径，收尾调用
  store 新增公开 action `pruneRecycled(paths)`（与 recyclePaths /
  aiRecyclePaths 内部同一批 applyRecycledPrune 语义：root + 受影响缓存树
  一次剪枝，并 bumpScanSeq 触发下游刷新）。全败/全取消不剪（没真删）。
- 新增单测：runRealDelete 成功后 root / scanCache 不再含已删路径、
  已回收按实测入账（**613 总 / 42 files**）。
- 浏览器实测：清理页预览弹窗「确认删除」→「再点确认」两步执行链路
  可用（既有交互，未改动）；剪枝确定性由单测锁定。

### 2026-10-16 清理脚本列表命中状态过滤（R221）

- R220 搜索框之上叠加「命中状态」过滤 chips（全部 / 已命中 /
  未检测到），与搜索词叠加生效——演示时直接抽「已命中」看 20 个
  真实可清项，「未检测到」只留安全软件套装（刻意不命中，防 AV 冲突
  的诚实设计）。
- 新增 i18n `cleanup.scriptsStatus*`（4 keys，CN/EN）。
- 浏览器实测：全部 21 → 「已命中」20（带体量降序）→ 「未检测到」1。
- 无新测试（纯组件接线；现有 612 全绿）。**612 总 / 42 files。**

### 2026-10-16 清理脚本列表搜索（R220）

- 清理页 21 个脚本此前只能滚动翻找；新增本地搜索框（输入即过滤，
  无需后端）。空查询显示全部；无匹配显示「没有匹配」空态。
- 中文别名表（cleanup/searchAliases.ts）：脚本名多为英文，中文用户
  搜「微信」「直播」「会议」也能命中——微信→WeChat、直播→OBS、
  会议→Teams + Zoom。别名表是搜索索引数据（非 UI 文案），仿 mocks.ts
  先例不进 MIGRATED_FILES（i18n coverage 守卫不拦数据文件）。
- 新增 i18n `cleanup.scriptsSearch` / `cleanup.scriptsSearchEmpty`（CN/EN）。
- 浏览器实测：搜「微信」→ WeChat 1 项；「直播」→ OBS；「会议」→
  Teams + Zoom 两项。
- 无新测试（纯组件接线；现有 612 全绿）。**612 总 / 42 files。**

### 2026-10-16 总览分类占用行可直达清理页预选（R219）

- 总览「文件分类占用」行此前点击跳工作台定位（R92）——有「可回收 X」
  副文字的行（回收指引最强）却去不了清理页，用户还得再找入口。
- 现在有可回收潜力的分类行点击 → 清理页预选该脚本（与 R217 建议项
  「去清理」同一语义）；无可回收项的分类行保持跳工作台定位不变。
- 新增 i18n `overview.cat.gotoCleanup`（CN/EN，行 title 提示）。
- 浏览器实测：点「浏览器缓存 Chrome / Edge（可回收 498 MB）」→
  清理页预选「浏览器缓存 Chrome / Edge 19.2 GB」，4 个 scope 行
  渲染可勾选。
- 无新测试（纯交互接线；现有 612 全绿）。**612 总 / 42 files。**

### 2026-10-16 建议项「去清理」按钮样式补齐（R218）

- R217 补了建议项「去清理」按钮但漏了 CSS——按钮以未样式化原生
  形式渲染（视觉缺陷）。补 `.ao-clean-goto` 样式（ghost 级轻按钮：
  22px 高 / 8px 内边距 / 1px 边框圆角 / hover 高亮），与同排的
  「预览展开」peek 按钮视觉一致。
- 浏览器实测 computed style：22px / 0 8px / 1px solid #d4d4d4 /
  6px radius / flex，与 peek 对照一致。
- 无新测试（纯样式补漏；现有 612 全绿）。
  **612 总 / 42 files。**

### 2026-10-16 总览建议项可直达清理页并预选脚本（R217）

- 可用性缺口修复：此前总览「建议清理项目」只有顶部「打开清理页」
  （无参，进页自动挑第一个命中），想针对某个具体建议项去清理页
  处理得自己在脚本列表里翻——逐条建议项新增「去清理」按钮：
  带该条的 scaffoldId 调 `openCleanup(id)` 直落清理页并预选该脚本
  （App.tsx `openCleanup` 本就支持可选 scaffoldId，AssetsOverview
  的 `onOpenCleanup` 签名补上可选参数贯通）。
- 新增 i18n `overview.clean.goto` / `overview.clean.gotoTip`（CN/EN）。
- 浏览器实测：总览「OBS 日志文件（OBS Studio 缓存 · 缓存 310 MB）」
  点「去清理」→ 落在清理页且预选「OBS Studio 缓存 1.10 GB」，
  详情区渲染 OBS 2 个 scope 行可勾选。
- 无新测试（纯组件接线 + 签名贯通；现有 612 全绿）。
  **612 总 / 42 files。**

### 2026-10-16 补 executeScope 执行路径天数联动测试（R216）

- R212 的天数缩放只覆盖了 `scopeSizesBatch`（尺寸探测路径），
  `executeScope`（dry-run/真删执行路径）的 `olderThanDays` 联动改了
  但没测——补测试覆盖执行路径：天数越小 dry-run 清单条目数与总字节
  越大（与尺寸探测同一语义，两条路径口径一致）。
- 新增 1 测试（mocks.test.ts）。**612 总（+1）/ 42 files。**
- 顺带验证：总览建议清理（6.40 GB）与工作台清理卡片已同步包含
  R210–R215 全部新增命中（OBS / Teams / Unreal / Discord / Zoom），
  两条独立链路（cleanupSuggestions + collectScaffoldCards）口径一致。

### 2026-10-16 清理页补「Zoom」命中演示，21 脚本全命中（R215）

- 清理页 21 脚本命中升至 21/21（21 个检测项只剩「安全软件套装」因杀软
  互斥风险按设计不演示——这是诚实行为，不是遗漏）。
- MOCK_TREE 补 `AppData\Roaming\zoom.us\{data\{WebRTC,Telemetry},logs}`
  （zoom，1.7 GB，音视频会议缓存/遥测/日志）挂 scaffold_id 命中；
  Roaming 体量 22.6→24.3 GB。
- 新增 1 测试（mocks.test.ts）：zoom 命中 zoom.us 且 >1GB。
  **611 总（+1）/ 42 files。**
- 浏览器实测：列表「Zoom 1.70 GB」，详情 6 个 scope 行（音视频缓存 /
  遥测 / 日志 / 安装包归档 / Chromium 缓存 / 应用日志）可勾选可预览。
- 里程碑：从 R205 的 8/21 命中到 R215 的 21/21 全命中，清理页每
  个脚本都有真实可点的演示数据。

### 2026-10-16 清理页补「Discord」命中演示（R214）

- 清理页 21 脚本命中升至 20/21（剩余：安全软件套装 / Zoom）。
- MOCK_TREE 补 `AppData\Roaming\discord\{Cache,GPUCache,blob_storage,
  Crashpad}`（discord，1.4 GB，Electron 缓存族）挂 scaffold_id 命中；
  Roaming 体量 21.2→22.6 GB。
- 新增 1 测试（mocks.test.ts）：discord 命中 discord 目录且 >1GB。
  **610 总（+1）/ 42 files。**
- 浏览器实测：列表「Discord 1.40 GB」，详情 5 个 scope 行（Chromium
  缓存 178 MB / 代码缓存 / GPU 缓存 / Blob 存储 / 崩溃报告）可勾选
  可预览。

### 2026-10-16 清理页补「Microsoft Teams」双变体命中演示（R213）

- 清理页 21 脚本命中升至 19/21（剩余：安全软件套装 / Discord / Zoom）。
- MOCK_TREE 补两个 Teams 命中目录，完整覆盖 R210 的 detectVariants
  修复（同脚本双变体同屏渲染）：
  - 1.x 经典版：`AppData\Roaming\Microsoft\Teams\{Cache,GPUCache,
    media-stack}`（0.9 GB，独立节点不抢 Microsoft 的 windows-privacy）；
  - 2.x 新版：`AppData\Local\Packages\MSTeams_8wekyb3d8bbwe\
    {LocalCache,TempState}`（3.2 GB，UWP 容器）。
- Roaming 体量 20.3→21.2 GB；Local 体量 51→54.2 GB。
- 新增 1 测试（mocks.test.ts）：teams 双变体命中（1.x 经典 + 2.x 新版
  各一个命中目录，totalSize > 1GB）。**609 总（+1）/ 42 files。**
- 浏览器实测：列表「Microsoft Teams 4.10 GB」，详情同屏渲染 2 组
  scope——1.x 组（Chromium 缓存 510 MB / 运行日志 405 MB…）+ 2.x 组
  （新版 LocalCache 357 MB…），共 2 section 11 勾选项；对照组
  WeChat 4.x/3.x 大版本共存不受影响。

### 2026-10-16 保留天数选择与可清字节联动（R212）

- 修复真实缺陷：预览模式里 scope 行的「保留最近 N 天」天数选择器改值
  后「待清」字节纹丝不动——`scopeSizesBatch` mock 忽略 `scopeDays`
  （真实后端按 mtime 过滤），演示上天数选择形同虚设。
- `mockScopeBytes(scopeId, days)` 增加天数缩放：只有带 `prompt.days` 的
  scope 受影响（无保留期的 scope 不受 scopeDays 影响，不误伤），天数
  越大可清越少，封底 30%（0 天=全清，≥30 天=30%）。
- `scopeSizesBatch` 接入 `scopeDays`（api 层已传，mock 此前丢弃）；
  `executeScope` 的 `olderThanDays` 同步联动（dry-run 清单与真删台账
  字节口径一致）；`cleanupSuggestions` 用默认天数，总览建议大小与
  清理页 scope 行同口径。
- 新增 2 测试（mocks.test.ts）：scopeDays 使可清字节随天数缩小（1 天
  清得多、默认≈30 天、单调递减且为正）；无 prompt.days 的 scope 不
  受 scopeDays 影响。**608 总（+2）/ 42 files。**
- 浏览器实测：QQ「接收的文件」改 1 天 → 303 MB（几乎全清），改
  60 天 → 92.9 MB（封底 30%），联动真实可见。

### 2026-10-16 清理页补「OBS Studio 缓存」命中演示（R211）

- 清理页 21 脚本仍有 4 个未命中（安全软件/ Discord / Teams / Zoom），
  OBS 是直播/录屏用户高频软件，%APPDATA%\obs-studio 真实常驻——补命中。
- MOCK_TREE 补 `AppData\Roaming\obs-studio\{crash-dumps,logs}`（obs-cache，
  1.1 GB，含 .log/.txt 崩溃日志）挂 scaffold_id 命中；Roaming 体量
  19.2→20.3 GB。
- 新增 1 测试（mocks.test.ts）：obs-cache 命中 obs-studio 且 >1GB。
  **606 总（+1）/ 42 files。**
- 浏览器实测：清理页 21 脚本 17 个命中，列表新增「OBS Studio 缓存
  1.10 GB」；详情 2 个 scope 行（OBS 崩溃转储 217 MB · 1,621 文件 /
  OBS 运行日志）可勾选可预览。

### 2026-10-16 修复变体脚本 scope 行不渲染 + 补 Unreal DDC 命中（R210）

- 修复真实缺陷（与 R206 隐私分组同根因的另一变体）：`detectVariants`
  只识别 WeChat 的 3.x/4.x 变体，而 engine-caches（unity/unreal/godot）
  与 teams（1.x/2.x）的全部 scope 都带 `variant` 标记——变体不识别则
  `visibleScopes` 过滤后为空，**勾选行一条不渲染**（列表有命中、有覆盖
  统计、点开却无可勾项）。
- `detectVariants` 补齐 5 个变体识别：UnrealEngine→unreal、
  /Unity|UnityHub→unity、/Godot→godot、MSTeams 包→2.x、
  Microsoft/Teams→1.x（MSTeams 优先于 1.x 判定）。
- MOCK_TREE 补 `AppData\Local\UnrealEngine\Common\DerivedDataCache`
  （engine-caches，5.1 GB，含 .udd 文件）挂 scaffold_id 命中。
- 新增 5 测试：engine-caches 命中 UnrealEngine（+1，mocks.test.ts）；
  detectVariants 识别 unreal/unity/godot/2.x/1.x（+4，model.test.ts）。
  **605 总（+5）/ 42 files。**
- 浏览器实测：清理页列表「游戏引擎缓存 5.1 GB」详情渲染 Unreal DDC
  2 个 scope 行（DDC 289 MB · 4,014 文件 / DLC Partial 186 MB）——
  修复前详情区只有覆盖统计、无任何勾选项。
- 顺带验证：历史页隔离条目「恢复」链路正常（已恢复：…旧报告.docx，
  条目从台账移除；`listUndo` 与 `undo` 同为 timestamp 降序，index 对齐
  无错位风险）；总览页「重复文件查找」快速操作弹 CleanupProposalDialog。

### 2026-10-16 清理页补「腾讯 QQ（经典版）」命中演示（R209）

- 清理页 21 脚本仍有 7 个未命中，腾讯 QQ 是中国 Windows 最常用 IM，
  %APPDATA%\Tencent\QQ 真实常驻——补命中让 QQ 接收文件/图片/视频清理可演示。
- MOCK_TREE 补 `AppData\Roaming\Tencent\QQ\<QQ号>\{FileRecv,ImageRecv}`
  （qq-pc，3.6 GB，含 zip/pptx/png 接收文件）——挂 scaffold_id 直接命中；
  Roaming 下 Tencent 从 leaf 扩成带 children 节点（体量 2.3→3.6 GB）。
- 新增 1 测试（mocks.test.ts）：qq-pc 命中 Tencent\QQ 且体量 >1GB。
  599 总（+1）/ 42 files。
- 浏览器实测：清理页 21 脚本 15 个命中，列表新增「腾讯 QQ（经典版）
  3.60 GB」；详情显示 5 个 scope 行（接收的文件 310 MB / 视频 278 MB /
  图片 273 MB 等）分 media/cache 两组，可勾选可预览。
- 顺带验证：总览页「重复文件查找」快速操作正常（弹 CleanupProposalDialog，
  5 项谨慎级建议、默认未勾选、逐项核对路径与说明后才可勾选）——之前误判
  为不跳转，实为按钮选择器问题，非缺陷。

### 2026-10-16 清理页补「Windows 更新缓存」命中演示（R208）

- 清理页 21 脚本仍有 8 个未命中（安全软件/QQ/游戏引擎/Discord/Teams/OBS/
  Zoom/Windows 更新）。Windows 更新缓存是真实 Windows 必有目录（
  SoftwareDistribution\Download 存放已下载未清理的更新包），演示价值高。
- MOCK_TREE 补 `C:\Windows\SoftwareDistribution\Download`（windows-update，
  2.4 GB，含 kb*.cab / *.msu 更新包文件）——挂 scaffold_id 直接命中。
- 新增 1 测试（mocks.test.ts）：windows-update 命中 SoftwareDistribution
  且体量 >1GB（校验命中来自真实更新包结构而非空目录）。598 总（+1）/
  42 files。
- 浏览器实测：清理页 21 脚本 14 个命中，列表新增「Windows 更新缓存
  2.40 GB」；详情显示 2 个 scope（安装包缓存 218 MB · 635 文件，因
  recommended_selected 默认预选 ✓ + 传递优化 171 MB · 2,602 文件）。
- 顺带验证：总览页「建议清理项目 → 打开清理页」跨页跳转正常（自动选中
  浏览器缓存）；工具墙 18 个硬件工具卡完整渲染。

### 2026-10-16 清理页补齐 3 个高频脚本命中演示（临时文件 / 崩溃转储 / Firefox 缓存）（R207）

- 扫描 MOCK_TREE 对 21 个清理脚本的命中：11 个未命中（安全软件/崩溃转储/
  QQ/系统临时/游戏引擎/Discord/Firefox 缓存/Teams/OBS/Windows 更新/Zoom）。
  挑真实 Windows 必有、演示价值最高的 3 个补齐命中节点。
- MOCK_TREE 补：`AppData\Local\Temp`（system-temp，2.8 GB）、`AppData\
  Local\CrashDumps`（crash-dumps，0.9 GB，含 .dmp 文件）；Firefox 树重构
  ——Profiles 保持归 browser-privacy，同级的 cache2 / shader-cache 归
  firefox-cache（visit 命中 scaffold_id 即不下钻，两脚本互不抢占）。
- 新增 3 测试（mocks.test.ts）：system-temp 命中 \Temp、crash-dumps 命中
  CrashDumps、firefox-cache 命中 cache2/shader-cache 且不吞 Profiles
  （browser-privacy 仍持 Profiles）。597 总（+3）/ 42 files。
- 浏览器实测（删 IndexedDB 旧树缓存重扫）：清理页 21 脚本中 13 个命中——
  列表新增 系统临时文件 2.80 GB / Firefox 浏览器缓存 1.20 GB / 崩溃转储
  922 MB；Firefox 详情命中 cache2+shader-cache 两目录，3 个 scope 行
  （网页缓存 641 MB / 着色器缓存 598 MB / 启动缓存 338 MB）可勾选可预览。

### 2026-10-16 清理页补「使用痕迹」分组，隐私 scope 可勾选可执行（R206）

- R205 让隐私脚本命中扫描树后暴露下一个缺陷：CleanupBody 按 category 只
  渲染 media / cache / backup 三组，而 privacy 脚本的全部 scope
  （recent-items / thumbcache / inet-cache / chromium-history /
  firefox-formhistory）category 全是 'privacy'——落不进任何组，勾选行
  永不渲染。隐私清理页因此只有覆盖统计，没有可勾项，预览/执行无从谈起。
- 修复：CleanupBody 补第四组 `<ScopeGroup label={t('cleanup.groupPrivacy')}
  group={privacyScopes} />`（分组同样走 sortScopesByBytes 降序）；
  i18n 补 `cleanup.groupPrivacy`：使用痕迹 / Usage traces（CN/EN 并行，
  共 1 个新 key）。
- 新增 1 测试（mocks.test.ts）：所有 tags 含 'privacy' 的脚本其全部 scope
  的 category 必须是 'privacy'——把「隐私 scope 分组渲染」固化成数据不变量，
  防止后续脚本改 category 时静默失去勾选行。594 总（+1）/ 42 files。
- 浏览器实测全链路：隐私清理页 → Windows 活动与使用痕迹 →「使用痕迹」组
  显示 3 行（Recent 511 MB · 174 文件 / thumbcache 384 MB · 4,550 文件 /
  INetCache 637 MB · 456 文件）→ 全选（两段式防误触）→ 预览模态
  68 个文件 · 1.50 GB · 进系统回收站可还原 · 「确认删除」可用——
  隐私清理演示链路首次端到端打通。

### 2026-10-16 预览模式隐私清理命中演示数据（R205）

- 隐私清理页（tagFilter="privacy" 的 CleanupPage）两脚本恒「未检测到」：
  MOCK_TREE 里 `User Data` 已被 browser-cache 占用（DFS 命中即不下钻），
  windows-privacy（Recent / thumbcache / INetCache）与 browser-privacy
  （Firefox 配置）在树里无命中节点，fallbackByNameContains 又限深 4 层
  够不到深目录 → 浏览器演示隐私清理永远是空态。
- MOCK_TREE 补真实磁盘结构的隐私痕迹节点（scaffold_id 命中，visit 全树
  不限深）：Local\Microsoft\Windows\Explorer（thumbcache_*.db /
  iconcache_*.db）+ Local\Microsoft\Windows\INetCache + Roaming\
  Microsoft\Windows\Recent（.lnk）+ Local\Mozilla\Firefox\Profiles\
  *.default-release（formhistory.sqlite / places.sqlite）。同时把 Local\
  Microsoft（原 leaf）扩成带 children 的节点、Roaming 补 Microsoft 子树。
- 新增 3 测试（mocks.test.ts）：browser-privacy 命中 Firefox 配置、
  windows-privacy 命中 ≥2 目录且总大小 >0、browser-cache 命中不受新增
  节点影响（仍 ≥2 目录 / >10GB）。MOCK_TREE 改 export。
- 593 总（+3）/ 42 files。
- 浏览器实测（删 IndexedDB 旧树缓存后重扫）：隐私清理页显示 Windows 活动
  与使用痕迹 2.60 GB（命中 Local\Microsoft\Windows 1.90GB + Roaming\
  Microsoft 717MB，覆盖统计 清理脚本覆盖 1.50GB / 红线保护 1.10GB）+
  浏览器使用痕迹 922 MB——不再「未检测到」，清理/预览按钮可用。

### 2026-10-16 Steam 打开动作 URL 映射提取为纯函数并补测试（R204）

- R202 的 `openSteamUrl` 非 Tauri 分支把 URL 计算内联在方法里（依赖
  window.open 副作用），无法直接测试；且与 Tauri 分支共享方法体，行为
  验证只能靠浏览器。
- 提取 `export function steamUrlFor(action, appid)` 纯函数（system-ext.ts
  顶部）：workshop_page → steamcommunity 公开 https 版、uninstall /
  rungameid / validate → steam:// 协议、nav → store 商店页。
  `openSteamUrl` 非 Tauri 分支改为 `window.open(steamUrlFor(...))`。
- 新增 `src/api/steamUrl.test.ts` 4 个测试：workshop https 版、steam://
  三动作、nav 商店页、数字边界（appid 0 / 大数）——590 总（+4）/ 42 files。
- 浏览器实测重构无回归：Steam 盘点 → 创意工坊 → 「在 Steam 中打开」→
  window.open 仍捕获 `.../sharedfiles/filedetails/?id=3010099`。
- 另验证本轮（无代码改动）：设置→通用 tab 完整（AI 联网/思考过程/并行
  扫盘/硬件加速/托盘开关）、AI 权限中心 18 开关 localStorage 持久化
  （点「调整电源计划」→ diskpilot.perms.v1 写入 + UI 即时 on）、清理页
  conda 链路无回归（扫描后 13.1 GB 命中 + 环境列表 base/tf2/web + 包缓存
  232 MB）。

### 2026-10-16 预览模式 UndoPanel 回收站提示补 catch（R203）

- R200 把 `api.openRecycleBin` 非 Tauri 改为 reject 后，HistoryPage 已补
  `.catch`，但 Studio 侧栏的 UndoPanel（同样渲染「打开系统回收站」按钮）
  仍是 `void api.openRecycleBin()` 无 catch——浏览器点击会未处理拒绝。
- UndoPanel 改 `.catch` + 页面级 setErr（复用 `cleanup.openRecycleBinFailed`
  i18n），点击显示与历史页同款诚实错误提示。
- 顺带全库扫描 `void api.xxx()` 无 catch 调用：webviewHeartbeat /
  cancelScan / revealInExplorer / scaffoldSource / stress_cancel 全部确认
  安全（resolve 或带 catch），无其他未处理拒绝缺口。
- 浏览器实测：Studio 最近清理面板 recycle 条目 → 点「打开系统回收站」→
  `.undo-msg.error` 显示「打不开系统回收站：Error: open_recycle_bin 仅在
  Tauri 环境可用…」。
- 无新测试（catch 行为，与 R200 先例一致）；586 总 / 41 files。

### 2026-10-16 预览模式 Steam 打开动作真实打开网页（R202）

- `api.openSteamUrl` 非 Tauri 分支原先 `Promise.resolve()` 静默——Steam
  盘点详情「在 Steam 中打开」（workshop_page）点击无任何反馈，UninstallVia
  Steam 的「正在启动 Steam 卸载」toast 也变成假成功。
- 非 Tauri 改为 `window.open` 真实 URL：workshop_page →
  `steamcommunity.com/sharedfiles/filedetails/?id=`（公开 https 版，能真
  开），uninstall/rungameid/validate → steam:// 协议（浏览器弹系统协议
  处理器），nav → store.steampowered.com/app/。不静默、不假拒绝——
  纯前端能做到的打开动作在浏览器里真实生效。
- 浏览器实测：Steam 盘点 → Counter-Strike 2 → 创意工坊（3 项 · 543 MB）
  → 点「在 Steam 中打开」→ window.open 捕获
  `https://steamcommunity.com/sharedfiles/filedetails/?id=3010099`。
- 无新测试（纯前端打开动作，与 R197-R201 先例一致）；586 总 / 41 files。

### 2026-10-16 预览模式重复文件确认窗显示真实大小（R201）

- 总览页「重复文件查找」→ CleanupProposalDialog 逐项实测
  `api.estimateSize` 非 Tauri 恒返回 0 → 每项被标「路径不存在 · 约 0 B」，
  勾选统计也是 0 B，与「约 1.19 GB 可回收」标题自相矛盾。
- `mocks.estimateSize`：新增 MOCK_KNOWN_FILES 表（scanDuplicateFiles 的
  8 个重复文件路径 → 组大小 486/121/8 MB），命中返回真实大小、未命中
  返回 0（与后端「路径不存在/空」同语义——确认窗的 missing 提示在预览
  模式依然诚实，只是 mock 数据本身能命中）。
- `api/scan.ts` estimateSize 非 Tauri 分支接 mock。
- `MB` 常量上移到文件顶部（原在 L1319 清理演示区，MOCK_KNOWN_FILES 顶层
  引用触发 TDZ）。
- 浏览器实测：重复文件确认窗 installer_x64 (1).exe 显示 486 MB，
  「路径不存在」误标消失，标题「约 1.19 GB 可回收」一致。
- mocks.test.ts +2（586 总 / 41 files）：候选路径命中返回组大小 / 未知名
  路径返回 0。

### 2026-10-16 预览模式「打开系统回收站」改为诚实拒绝（R200）

- 历史页 recycle 类条目渲染「打开系统回收站」按钮（restoreKind
  'osRecycleBin'，mock UNDO_LEDGER 有 action='recycle' 条目），但
  `api.openRecycleBin` 非 Tauri 分支 `Promise.resolve()`——点击静默无操作。
- 改为 `Promise.reject(new Error(t('errors.tauriOnly', { cmd: 'open_recycle_bin' })))`；
  HistoryPage L224 由 `void api.openRecycleBin()` 改为 `.catch` + 页面级
  `setErr`（新增 i18n `cleanup.openRecycleBinFailed`，CN/EN 平行，含
  「浏览器预览模式无桌面回收站」说明）。
- 浏览器实测：历史页点「打开系统回收站」→ 错误横幅
  「打不开系统回收站：Error: open_recycle_bin 仅在 Tauri 环境可用（浏览器
  预览模式无桌面回收站，此操作仅桌面应用可用）」。
- 无新测试（reject 行为，与 R195/R196/R199 先例一致）；584 总 / 41 files。

### 2026-10-16 预览模式脚本安装改为诚实拒绝（R199）

- `api.installScaffold` 非 Tauri 分支原先 `Promise.resolve('')` ——Studio
  「+ 安装脚本」导入 toml 后 toast「安装成功 id: 」却什么都没写（假成功，
  空 id）。
- 改为 `Promise.reject(new Error(t('errors.tauriOnly', { cmd: 'install_scaffold' })))`
  ——浏览器演示导入解析/勾选流程，确认安装时给出诚实错误
  「install_scaffold 仅在 Tauri 环境可用」。Studio.tsx 两个调用点
  （toml tab L291 / winapp2 tab L403）均有 try/catch，无未处理拒绝。
- `uninstallScaffold` 保持现状：UninstallButton 仅 `source==='user'` 显示，
  而 `scaffoldSource` 非 Tauri 固定返回 'embedded' → 浏览器不可达，无假成功面。
- 浏览器实测：Studio 安装模态填合法 toml → 安装 → 错误区显示
  「install_scaffold 仅在 Tauri 环境可用」。
- 无新测试（reject 行为，与 R195/R196 先例一致）；584 总 / 41 files。

### 2026-10-16 预览模式补全 Conda 环境区（R198）

- 清理页「Conda / Anaconda」脚本在浏览器显示「未检测到」且永远无环境区：
  `api.listCondaEnvs` 非 Tauri 返回 `[]`（环境区空），且 MOCK_TREE 没有
  anaconda3 目录（conda 脚本的 must_have_child:['pkgs'] 永不命中）。
- `mocks.listCondaEnvs`：base + 2 个用户环境（tf2 stale 90d 默认勾选 /
  web），base 灰显不可勾、大小与最后活跃时间齐全。
- MOCK_TREE `C:\Users\90740\` 下加 anaconda3（pkgs 4.2GB + envs/tf2 6.8GB +
  envs/web 2.1GB = 13.1GB，scaffold_id: 'conda'）——conda 脚本命中后
  isConda session 触发 listCondaEnvs mock，环境区可演示。
- `api/clean.ts` listCondaEnvs 非 Tauri 分支接 mock。
- 浏览器实测（清 scanCache IndexedDB 强制重扫）：清理页 Conda 13.1 GB
  detected，环境区「base · 不可清 4.20 GB · tf2 6.80 GB · 90 天没动过 ·
  web 2.10 GB」+ 包缓存 scope 待清 232 MB 全渲染。
- mocks.test.ts +1（584 总 / 41 files）：base 不可勾 + ≥2 用户环境 + stale 默认勾选。

### 2026-10-16 预览模式启动项启停/删除真实生效（R197）

- `api.setStartupItem` / `removeStartupItem` 非 Tauri 分支原先 `Promise.resolve()`
  / `Promise.resolve('')` ——演示里点「禁用/恢复」toast 显示成功但列表纹丝不动
  （假成功，最差的演示口径）。
- `mocks.setStartupItem` / `removeStartupItem`：作用于模块级内存态
  `startupItems`（同 R192 reminderCfg 模式）——启停翻转 enabled、删除移出
  列表且返回备份路径（注册表项 .reg 备份 / 文件夹项回收站空串）；
  `listStartupItems` 改读内存态副本，会话内切换/删除真实持久（刷新页面重置）。
- 真实桌面端行为不变（写注册表/启动文件夹 + .disabled 备份 + 回收站），
  浏览器只在内存态演示，不假装写了系统。
- 浏览器实测：系统优化页「开机自启 5 项 → 6 项」（恢复 OBS Studio），
  OBS 行 sys-row-off → sys-row，两步确认「确认恢复？」→ 恢复全链路可用
  （需 startup.manage 权限）。
- mocks.test.ts +1（583 总 / 41 files）：启停持久 + 删除返回备份路径/移出列表。

### 2026-10-16 预览模式补全电源计划（R196）

- 系统优化页「电源计划」卡在浏览器预览模式直接报错——`api.powerPlan`
  非 Tauri 恒 tauriOnly reject，枚举电源计划（只读）断链。
- `mocks.powerPlan`：3 个标准计划（均衡/高性能/节能 GUID）+ 当前活动
  = 均衡；电源计划卡能渲染「使用中」标记 + 切换按钮。
- `api/system-ext.ts` powerPlan 非 Tauri 分支：无参（读）→ mock；
  带 setScheme（写：切换系统电源计划，L2 + 两步确认）→ 保持 tauriOnly
  拒绝——演示不假装「已切换系统电源计划」。
- 浏览器实测：系统优化页「平衡 · 使用中 · 高性能/节能 · 切换」，
  ⚡ 活动标记齐全（需 power.plan 权限）。
- mocks.test.ts +1（582 总 / 41 files）：3 标准计划 + active∈schemes。

### 2026-10-16 预览模式补全社区脚本仓库（R195）

- 工作台右侧「清理模板库 → 社区脚本仓库」在浏览器预览模式展开后
  显示「暂无社区脚本」——`api.scaffoldRegistryList` 非 Tauri 返回
  {items: [], warning: null}，社区仓库区演示断链。
- `mocks.scaffoldRegistryList`：3 条演示社区脚本（Edge 缓存清理 /
  临时文件深度清理 / 系统日志归档清理），带版本/作者/风险标签/
  Ed25519 签名徽标元数据。「一键安装」写操作保持 tauriOnly。
- `api/clean.ts` scaffoldRegistryList 非 Tauri 分支接 mock。
- 浏览器实测：展开社区仓库 → 三条目齐全，✓ Ed25519 已签名徽标 +
  风险标签 + 安装按钮。
- mocks.test.ts +1（581 总 / 41 files）：≥3 条带签名与下载量条目。
- 验证注记：Studio 右栏需收起 AI 侧栏（1280px 窗口三栏预算不足时
  rightVisible=false）才能看到——这是窗口响应式设计，非 bug。

### 2026-10-16 预览模式补全 MCP 审计日志（R194）

- 设置页「MCP → 审计日志」在浏览器预览模式显示「暂无审计记录」——
  `api.mcpAuditTail` 非 Tauri 返回 []，审计面板演示断链。
- `mocks.mcpAuditTail`：4 条与 R188 mcpListServers 同源的演示审计——
  调用 list_dir（L0 成功）、调用 write_file（L2 已拒绝 + 权限不足原因）、
  编辑服务器、添加服务器；granted/ok/perm/args 键名齐全，审计徽标
  （已拒绝红标 / L0 绿标 / 成功勾）都能渲染。
- `api/mcp.ts` mcpAuditTail 非 Tauri 分支接 mock。
- 浏览器实测：审计面板显示「调用 list_dir · L0 · 参数 path」、
  「调用 write_file · L2 · 已拒绝 · 权限不足」、编辑/添加服务器记录。
- mocks.test.ts +1（580 总 / 41 files）：含成功 + 拒绝 + 增删审计。

### 2026-10-16 预览模式补全插件市场（R193）

- 设置页「插件市场」在浏览器预览模式两个空态：内置列表
  （`pluginMarket` → []）与已安装台账（`pluginListInstalled` →
  {installed: [], total_events: 0}）都是空，市场页演示断链。
- `mocks.pluginMarket`：3 个内置 CLI 工具（智能清理/启动项管理/驱动
  检测），2 个已插件化（与 R184/R185 的启动项/驱动 mock 呼应）。
- `mocks.pluginListInstalled`：2 条台账（智能清理 v2.3.1 注册表源 +
  2 个历史版本；启动项管理 v1.2.0 远程 URL + 1 个历史版本）——
  「更新」「回滚」「卸载」三按钮都能出现，R162 台账排序（新在前）
  可看。
- `api/system-ext.ts` 两接口非 Tauri 分支接 mock。社区索引
  （pluginRegistryList/Search）保持 EMPTY_REGISTRY——它依赖真实
  URL 配置，写操作全 tauriOnly，mock 列表会假装有远端内容（语义
  不符）；「未配置索引 URL」提示是诚实状态。
- 浏览器实测：市场 tab「已插件化 2 / 3」+ 三条目；
  社区 tab 进阶区「已安装 2」台账：启动项管理 v1.2.0（历史 1 个版本）
  / 智能清理 v2.3.1（历史 2 个版本），更新/回滚/卸载按钮齐全。
- mocks.test.ts +2（579 总 / 41 files）：市场列表形状 +
  台账含 ≥2 历史版本条目（回滚/更新按钮可演示）。
- 修 tsc 报错：MockInstalledPlugin.history 用具体结构类型
  MockLedgerEntry[]（与 api LedgerEntry 同形，避免 Record 不兼容）。

### 2026-10-16 预览模式补全清理提醒（R192）

- 设置页「清理提醒」在浏览器预览模式两个断点：
  - 开提醒开关点了没反应——`getReminderConfig` / `setReminderConfig`
    非 Tauri 分支都返回硬编码默认值（enabled: false），保存的 patch
    被丢弃，开关永远弹回关闭。
  - 「现在检查一次」永远 toast「没有可提醒的项目」——`runReminderCheck`
    非 Tauri 返回 null（浏览器没真实空间历史）。
- `mocks.getReminderConfig` / `mocks.setReminderConfig`：模块级可变状态
  （模拟 app_data_dir/reminder.json），开关/间隔/阈值/周报开关在会话内
  可真实切换（不再弹回关闭）。
- `mocks.runReminderCheck`：C 盘 2.91 GB 提醒负载（与清理建议 mock
  同源），只读提醒——绝不执行清理。
- `api/clean.ts` 三接口非 Tauri 分支接 mock。
- 浏览器实测：开关 false→true 保持开启；「现在检查一次」→
  「上次检查：可释放 2.91 GB · C:\ · 2.91 GB（已发通知）」。
- mocks.test.ts +1（577 总 / 41 files）：开关会话内切换 + 检查负载。

### 2026-10-16 预览模式补全硬件信息（R191）

- 承接 R190：总览「今日体检」硬盘健康格在浏览器预览模式永远「未读到」——
  `api.hwInfo` / `api.hwDiskHealth` 非 Tauri 分支 tauriOnly 拒绝，而体检卡
  其余四格全是模拟数据，五格只缺硬盘健康显得突兀。
- `mocks.hwInfo`：一台与驱动 mock 同源的演示机器（ThinkPad X1 Carbon +
  i7-1365U 12 线程 + 16GB Samsung 内存 + Iris Xe 显卡 + Samsung 980 PRO +
  Win11 专业版 + 温度 32.5°C），字段齐全（cpu/cpu_usage/gpu/gpu_live/
  memory/system/bios/board/os/disk/thermal）。
- `mocks.hwDiskHealth`：C 盘 Samsung SSD 980 PRO 1TB，health=Healthy、
  drive_letters=C、reliability=true、cdi_report Good 100% temp 42°C，
  note 注明「演示数据：浏览器预览模式未连接真实 SMART 传感器」
  （诚实标注）。受控压测（hw_run_test）保持 tauriOnly 拒绝——
  真实负载测试不演示。
- `api/system-ext.ts` hwInfo / hwDiskHealth 非 Tauri 分支接 mock。
  工具墙整体保持 isTauri gate（浏览器显示「硬件工具箱仅在桌面应用中
  可用」）——桌面端工具执行能力不演示，仅硬件信息这类只读快照可看。
- 浏览器实测：体检卡五格齐全「无红盘 · 可清理 2.91 GB · 最高温 52℃ ·
  启动项 8 个 · 已禁用 3 · 1 块硬盘健康」（原来「硬盘健康未读到」）。
- mocks.test.ts +2（575 总 / 41 files）：hwInfo 静态字段齐全 +
  hwDiskHealth C 盘 Healthy。

### 2026-10-16 预览模式补全风扇曲线建议（R190）

- 总览「今日体检」温度格在浏览器预览模式永远「未读到温度」——
  `api.fanCurveAdvice` 非 Tauri 分支 tauriOnly 拒绝（真传感器命令），
  而体检卡的盘容量/清理建议全是模拟数据，唯独温度格空着显得突兀。
- `mocks.fanCurveAdvice`：均衡档建议 + 最高温 52°C + CPU 负载 34% +
  6 锚点建议曲线（与 R187 周报 mock 同源 52°C）+ CPU 风扇传感器；
  note 注明「演示数据：浏览器预览模式未连接真实温度传感器」
  （诚实标注，不冒充真读数）。
- `api/system-ext.ts` fanCurveAdvice 非 Tauri 分支接 mock。
  风扇控制（fan_curve_apply 写 EC）保持 tauriOnly 拒绝——真实硬件写
  不演示假成功；建议区仍需「风扇曲线建议」权限（L2 session），
  与 R184/R185 的「演示数据 + 权限闸保留」模式一致。
- 浏览器实测：体检条「最高温 52℃」（原来「未读到温度」）；
  系统工具页开权限后显示「均衡 · 最高温度 52℃ · CPU 负载 34%」+
  「档位与上次一致（均衡）· 最高温持平」（R173 对比）+
  建议曲线锚点 + 风扇传感器 + 演示数据标注。
- mocks.test.ts +1（573 总 / 41 files）：52°C 同源 + 曲线锚点 ≥4。

### 2026-10-16 预览模式补全重复文件查找（R189）

- 总览「快速操作 → 重复文件查找」在浏览器预览模式点了没反应——
  `api.scanDuplicateFiles` 非 Tauri 分支返回 `[]`，AI 清理提案窗
  （CleanupProposalDialog）演示链路在「重复文件」这一环断掉。
- `mocks.scanDuplicateFiles`：3 组确定性重复（installer 486MB ×3 份、
  meeting 视频 121MB ×3 份、report docx 8MB ×2 份），每组 keep 建议保留
  一份（path 最短）、其余进 recycle_candidates；running 为空
  （不做「跳过原因」演示分支）。只读 mock——清理执行仍走确认窗铁律
  （executeAiPlan user_confirmed），本 mock 不碰文件。
- `api/clean.ts` scanDuplicateFiles 非 Tauri 分支接 mock。
- 浏览器实测：点「重复文件查找」→ 清理提案窗弹出，installer_x64 /
  meeting-2026-09 重复组带路径可见，可回收合计 >0。
- mocks.test.ts +1（572 总 / 41 files）：≥2 组、每组 keep∈files 且
  recycle_candidates 不含 keep。

### 2026-10-16 预览模式补全 MCP 服务器列表（R188）

- 设置页「MCP」tab 在浏览器预览模式一直是空表——`api.mcpListServers`
  非 Tauri 分支返回 `[]`，MCP 是桌面端特色能力，列表/权限摘要/工具数
  在演示里看不到。
- `mocks.mcpListServers`：3 个样例服务器——filesystem（stdio · 启用 ·
  可写 · 3 个 L0 / 4 个 L1 · 7 个工具）、time（http · 启用 · 4 个 L0 ·
  4 个工具）、github（http · 停用 · 未接入，不编造工具数）；
  enabled 服务器有 tool_count，停用的为 null（诚实）。
- `api/mcp.ts` mcpListServers 非 Tauri 分支接 mock（补 mocks import）。
  增删改/调用工具保持 tauriOnly 拒绝（真后端副作用不演示假成功），
  写操作仍需 mcp.manage 权限。
- 浏览器实测：「MCP 服务器（3）」列表 + 每行启用状态/传输类型/权限摘要/
  工具数/测试编辑删除按钮 + 停用行「未接入」。
- mocks.test.ts +2（571 总 / 41 files）：stdio/http 混合覆盖、
  停用服务器不编造工具数。

### 2026-10-16 预览模式补全周巡检简报（R187）

- 历史页「周巡检简报」卡在浏览器预览模式只有空壳——`api.getWeeklyReports`
  返回 `[]`（卡片空态），`generateWeeklyReport` 返回 emptyWeeklyReport
  （生成一份全空周报），R95 环比对比、R154 间隔天数、盘健康摘要全无。
- `mocks.generateWeeklyReport` / `getWeeklyReports`：C 盘样例周报，可清字节
  = 2.91GB（与 R182 cleanupSuggestions 演示口径同源）、最高温 52°C、
  硬件快照 2 份、盘健康 1 块 good、周内硬件变化（CPU 温降 3°C / 通电
  +168h）。未生成过时先出一份 7 天前的样例——环比一开始就有数据可看
  （生成前就有「与上周对比」入口，不用先点一次生成）。
  生成写入演示归档，`getWeeklyReports` 立即读到，最新在前。
- `api/clean.ts` 两个接口非 Tauri 分支接 mock；删除死代码
  `emptyWeeklyReport`（原兜底已被 mock 取代）。
- 浏览器实测：「全盘可清理约 2.91 GB · 最高温 52°C · 7 天前 · 硬件快照 2 份」；
  点「生成本周简报」→「刚刚 · 7 天后到期」+「与上周对比 · 间隔 7 天 ·
  可清理量与上周持平 · 最高温与上周持平」+「去清理页看明细 C:\ 2.91 GB」+
  「硬盘健康 1 块盘全部健康」+ 硬件变化明细。
- mocks.test.ts +2（569 总 / 41 files）：未生成时样例一份、生成后归档+1
  且最新在前。

### 2026-10-16 预览模式补全空间外推检查（R186）

- 设置页「清理提醒 → 检查趋势」在浏览器预览模式一直是空结果——
  `api.checkSpaceForecast` 非 Tauri 分支返回 `[]`，R100 的「预计满盘天数」
  体检口径在演示里拿不到数据。
- `mocks.checkSpaceForecast`：返回 C 盘 `{ days: 112, reason: 'ok',
  warn: false }`——与 R183 趋势 mock 的「约 112 天满」同源（同一份趋势
  口径外推），体检/清理提醒的立即检查不再空手而归。
- `api/clean.ts` checkSpaceForecast 非 Tauri 分支接 mock。
- 浏览器实测：清理提醒「检查趋势」显示「C:\ · 约 112 天后满」，
  与趋势卡外推一致。提醒未开启时手动检查会被忽略（设计行为，数据已就绪）。
- mocks.test.ts +1（567 总 / 41 files）：C 盘正天数、reason=ok、不告警。

### 2026-10-16 预览模式补全系统工具驱动检查（R185）

- 承接 R184：「系统工具 → 驱动检查」的已装驱动列表在浏览器预览模式是
  空态（`api.listInstalledDrivers` 非 Tauri 分支返回 `[]`），开权限后
  也只能看到空表。
- `mocks.listInstalledDrivers`：9 条样例驱动，覆盖常见类别
  （display ×1：Intel Iris Xe Graphics；sound ×1：Realtek Audio；
  network ×2：Intel Wi-Fi 6E AX211 / Realtek GbE；storage ×1：Samsung
  980 PRO；chipset ×1；input ×3），带厂商/版本/日期字段；每次调用返回
  副本（防污染）。只读 mock——`check_driver_updates` 联网查询保持
  tauriOnly 拒绝（授权联网命令不演示假查询）。
- `api/system-ext.ts` listInstalledDrivers 非 Tauri 分支接 mock。
- 权限闸保持原设计：驱动检查需开启「驱动更新检查」（L2 session）。
- 浏览器实测（权限开启后）：「已装驱动 9 个」+ 类别筛选计数
  （input 3 · network 2 · chipset 1 · display 1 · sound 1 · storage 1），
  每条驱动带厂商·版本·日期·类别。
- mocks.test.ts +2（566 总 / 41 files）：类别覆盖 + 独立副本。

### 2026-10-15 预览模式补全系统工具启动项（R184）

- 设置页「系统工具 → 启动项」在浏览器预览模式一直是「空列表」——
  `api.listStartupItems` 非 Tauri 分支返回 `[]`；即使不去动权限闸，
  mock 层先补上，让演示路径从「打开开关 → 空表」变成「打开开关 → 有数据」。
- `mocks.listStartupItems`：8 条样例启动项，覆盖四个来源分组
  （注册表 HKCU ×4：腾讯QQ/微信/OBS Studio/腾讯会议；用户启动文件夹 ×2：
  aria2/uTools；注册表 HKLM ×1：NVIDIA；机器启动文件夹 ×1：HP 打印助手），
  5 启用 3 停用；每次调用返回副本（防止调用方改动污染模块级数据）。
- `api/system-ext.ts` listStartupItems 非 Tauri 分支接 mock。
- 权限闸保持原设计不动：启动项仍需在 AI 权限中心开启
  「管理启动项」（L2 session），与真实桌面端体验一致。
- 浏览器实测（权限开启后）：「开机自启 5 项 · 已停用 3 项」，
  来源分组渲染（注册表 HKCU / 用户启动文件夹 / 注册表 HKLM），
  搜索「微信」命中过滤，启停/删除两步确认入口齐备。
- mocks.test.ts +2（564 总 / 41 files）：四来源分组覆盖、
  调用返回独立副本不污染。

### 2026-10-15 预览模式补全总览趋势卡（R183）

- 承接 R182：磁盘详情和清理建议能看了，但「磁盘空间趋势」卡还是
  「扫描后自动记录空间变化，这里会显示多日曲线」——浏览器预览模式下
  `api.getSpaceHistory` 返回空对象 → series 为空 → 曲线/增量/耗尽预测
  全无。趋势是总览最后一个空卡。
- `mocks.getSpaceHistory`：返回与 listDrives 同一块 C 盘的近 14 天每日
  采样（已用空间缓慢爬升约 0.4GB/天，第 4 天含一次演示清理回落 -2.0GB，
  趋势能看到「清完下降」的增量）；采样 >5 点 → `spaceTrendSamples` 判定
  rich（趋势可信），增量、周环比、耗尽预测（forecastFullDays）全部展示。
  最后一点用当前真实值（与 listDrives 的 148.1GB 一致，不矛盾）。
- 浏览器实测：趋势行「C: 盘 79% · 148 GB · +4.50 GB（近 13 天）· 14 点 ·
  约 112 天满」，SVG 迷你面积曲线 + 渐变填充，7天/30天/全部窗口切换正常，
  点行跳工作台定位（R91）。
- mocks.test.ts +1（562 总 / 41 files）：C 盘 ≥5 采样点、时间升序、容量自洽。

### 2026-10-15 预览模式补全总览页演示链路（R182）

- 承接 R181：清理页能演示了，但总览页还是「未检测到磁盘」——浏览器预览模式
  下 `api.listDrives` 返回空数组 → `selDrive` 恒为 null → 磁盘详情卡、清理建议
  卡、空间趋势、救援横幅、底部统计全部空态，`cleanupSuggestions` 也返回空列表，
  R102 逐项预览和一键清理同样点不出来。
- `mocks.listDrives`：返回与 MOCK_TREE 同源的一块 C 盘（187.1GB 总量 ·
  148.1GB 已用 · 39.0GB 可用），总览整页从「未检测到磁盘」恢复成可演示。
- `mocks.cleanupSuggestions`：按 rootPath 前缀在 MOCK_TREE 收集命中的
  scaffold 目录（与扫描命中同源），每脚本挑最多 2 个 cache/media scope
  生成建议项；字节沿用 R181 的 `mockScopeBytes`，总览显示的大小 =
  执行时真正清的大小（同一口径）。未命中目录返回空列表（不编造）。
- 浏览器实测：磁盘详情（C 盘 79%）、清理建议「预计可释放 2.91 GB」13 项、
  逐项预览 12 条样例明细、微信项勾选引导去清理页（G-高3 不受影响）、
  「打开清理页」跳转正常、底部「本机已用 148 GB · 已扫 1/1 个盘」。
- mocks.test.ts +2（561 总 / 41 files）：listDrives 单 C 盘且容量自洽、
  cleanupSuggestions 命中/scope 存在性/未命中空表。

### 2026-10-15 预览模式补全清理演示链路（R181）

- 浏览器预览模式（非 Tauri）此前 `scopeSizesBatch` 返回空表、`executeScope`
  返回 `[]`——清理页所有 scope 显示「空」且 checkbox 禁用，「勾选 → 预览 →
  真删 → 进度条/取消 → 历史台账」整条链路在浏览器里**无法演示**（作者在
  dev server 里调 UI 看不到 R178–R180 的效果，只能靠单测）。
- `mocks.scopeSizesBatch` 改为按 scaffold 的 scopes 生成**确定性尺寸表**
  （scope_id 哈希，50MB–2.5GB 量级、含 file_count），同 scope 两次查询结果
  一致、刷新不跳变；未知脚本仍返回空表（不编造）。
- 新增 `mocks.executeScope`：dry-run 返回样例清单（路径拼在命中目录下，
  条目数 = 命中体量换算，上限 64 条），**只出清单不写台账**；真删才追加
  回收条目到台账（与 `listUndo` 共用），同步把 `api.executeScope` 非 Tauri
  分支从 `Promise.resolve([])` 接到 mock——历史页可见真删结果。
- 已在浏览器实测：勾选 4 个 scope → 预览窗显示「90 个文件 · 共 2.00 GB ·
  进系统回收站」→ 确认后弹窗内实时进度「正在清理 · 4/8 项 · 已释放 1.22 GB」
  （R180）+「取消」按钮 → 取消瞬间出「已取消 · 先清掉 54 个文件 · 约 1.22 GB」。
- mocks.test.ts +6（557 总 / 41 files）：尺寸表与 scopes 一一对应、确定性、
  未知脚本空表、dry-run 不写台账、真删追加台账、条目上限。

### 2026-10-15 进度移入预览弹窗，取消按钮真正可点（R180）

- 承接 R178/R179 发现的问题：真删过程中预览弹窗（zIndex 60 全屏遮罩）一直
  盖在清理页上方，CleanupBody 里的实时进度条和「取消」按钮用户**根本够不到**
  ——实测只有 `runRealDelete` 会置 running，且它运行时 `preview` 始终非空，
  body 进度块从未有机会露出来。用户当时只能看到 ProgressButton 的**模拟进度**
  （按估算时间渐近爬到 92%，不是真实进度）且**没有取消入口**。
- 现在把真实进度 + 取消按钮**移进 DryRunPreviewDialog 弹窗内部**：
  弹窗摘要下方新增实时进度条（`cleanup.progressInfo`：已清 done/total 项 ·
  已释放字节；取消已登记时切 `cleanup.cancelling` 变淡）；弹窗底部主操作位在
  真删进行中从「确认删除」换成「取消」按钮（`cleanup.cancellingBtn` 禁用态）。
- 移除 CleanupBody 中已死的进度块（running 期间永远被弹窗遮住），避免双份
  进度的维护漂移。取消语义不变：已交批次不撤回（回收站可逆）、后续批次不再提交。

### 2026-10-15 取消登记即时可见（R179）

- 承接 R178：点「取消」后到批次边界生效前，按钮仍亮、进度照跑，用户不确定
  取消到底有没有被接住。现在 `cancelRequested` 一旦置位，进度区立刻进入
  「取消中」态：文案换成 `cleanup.cancelling`（已交批次继续完成 · 已释放 X）、
  按钮禁用并显示「取消中…」、进度条变淡、标题转灰。
- 纯前端呈现层改动，取消语义不变（已交批次不撤回、后续批次不再提交）。
  i18n `cleanup.cancelling / cancellingBtn`（zh/en 成对）。
- useCleanupStore.test.ts +1（551 总 / 40 files）：断言 `cancelCleanup` 只置位
  `cancelRequested`、不清进度 / 不改 running——UI 的「取消中」态正依赖这个契约。

### 2026-10-15 清理执行可取消 + 实时进度条（R178）

- 真删此前是一次性 `Promise.all` 全并发提交：点了「确认执行」后只能干等，
  大目录清理期间既看不到进展也无法中止。现在 `runRealDelete` 改为**分批执行
  （CONCURRENCY=4）**，每批结束在**批次边界**检查 `cancelRequested`——已交后端
  的批次不撤回（清进回收站可逆），取消只「不再提交后续批次」。
- 执行中界面新增进度区：`已清 done/total 项 · 已释放 {size}` + 进度条 + 「取消」
  按钮（CleanupBody）。进度 `done/total/freedBytes` 随批实时写回 session；字节用
  后端实测 `bytes_freed` 累计，非预览预估。
- 收尾文案按四种口径如实报：全成 `cleanup.done` / 中途取消已清一部分
  `cleanup.cancelled` / 取消但还没清掉任何文件 `cleanup.cancelledNone` / 全败
  `cleanup.allFailed`；部分失败仍追加 `cleanup.partialFail`。i18n
  `cleanup.progressInfo / cancelled / cancelledNone`（zh/en 成对）。
- 后端 `execute_scope` 无取消令牌，故取消是纯前端可中断性——回收站删除可还原，
  中途停下安全。新增 useCleanupStore.test.ts +4（550 总 / 40 files）。纯前端改动。

### 2026-10-15 清理分类组按可清大小降序排序（R177）

- 清理页三个分类组（媒体 / 缓存 / 备份，conda 下为依赖包）原本只有「媒体」组
  按可清大小降序，缓存 / 备份组按目录原序平铺——同是分类组却两种顺序口径，
  用户按体量挑大项时认知跳脱。现在四类组统一交给 `ScopeGroup` 按「可清大小」
  降序渲染（与媒体组对齐）。
- 新纯函数 `sortScopesByBytes(scopes, sizeOf)`（cleanup/model.ts）：稳定降序、
  不改入参、sizeOf 取不到按 0 排尾。消除了 `CleanupBody` 顶部对媒体组的内联
  `.sort(...)`，避免两处排序口径漂移。纯前端改动，无后端依赖。
- cleanup/model.test.ts +4（546 总 / 39 files）。

### 2026-10-15 工具墙主网格可排序（R176）

- 工具箱几十上百个工具，主网格此前只能按目录原序排列（不好找）。
  现在网格标题行新增「排序」控件：**默认 / 名称 / 风险 / 分类**。
  - 名称：按中文名 localeCompare 排序。
  - 风险：高危→中危→低危（同风险按名称），安全敏感用户一眼看到高风险工具。
  - 分类：按所属分类聚合（同分类按名称），配合左侧分类导航更直观。
  - 默认保持目录原序（老用户无感）。
- 排序在搜索 / 推广过滤 / 风险筛选**之后**执行，只影响呈现顺序、不影响集合。
- i18n `toolbelt.wall.sortLabel / sortDefault / sortName / sortRisk /
  sortCategory`（zh/en 成对）。纯前端改动，无后端依赖。

### 2026-10-15 清理行动条显示「占可清总量」比例（R175）

- 底部行动条的「已选 N 项 · 共 X GB」没说这次选择覆盖了全盘可清量的几成
  ——用户不知道自己的选择是「清一半」还是「几乎全清」。现在选中有字节时
  补「（占可清总量 N%）」，比例一眼可见。
- 新纯函数 `computeAllCleanableBytes(s)`（useCleanupStore.ts，所有可见
  scope 可清字节之和，未测到按 0）；复用 R174 的 `cleanRatio` 算占比；
  i18n `cleanup.summaryShare`（zh/en 成对）。总可清量为 0 时不显示比例
  （避免除零）。

### 2026-10-15 清理项显示可清理占比条（R174）

- 清理页每个 scope 行的「总大小 · 可清理 · 保留」三个数字要读一遍才知道
  「这项能清掉几成」。现加一条 46px 细占比条（可清部分填强调色，悬停显示
  「约可清掉这项的 N%」），比例一眼可见。
- 新纯函数 `cleanRatio(bytes, totalBytes)`（cleanup/model.ts）——total 非正
  或非有限 → null（没分母就不给比例）；结果钳到 [0,1]（可清 > 总 的脏数据
  不画出超长条）；占比为 0 时不出条。i18n `cleanup.ratioTip`（zh/en 成对）；
  CSS `.cleanup-ratio` / `.cleanup-ratio-fill`。cleanup/model.test.ts +3
  （542 总 / 39 files）。

### 2026-10-15 风扇建议显示与上次的对比（R173）

- 风扇曲线建议此前每次只显示当前档位/温度/锚点，看不出「这次和上次比变了
  没」——档位抬升说明负载或温度在涨，回落说明变凉快了。现每次读取后记录
  快照（localStorage），并显示「档位较上次：静音 → 均衡 · 最高温 +8℃」或
  「档位与上次一致（均衡）· 最高温持平」。
- 新文件 `src/fanHistory.ts`：`parseFanSnapshot` / `formatFanSnapshot`
  （坏 JSON / 缺 level / 非法温度 → null，不编造）+ `fanAdviceDelta`
  （任一方温度读不到 → 温差 null）；SystemTools 的 loadFan 成功后存快照、
  渲染前算 delta。i18n 5 个新键（zh/en 成对）；CSS `.sys-fan-delta`。
  fanHistory.test.ts（新）+9（539 总 / 39 files）。

### 2026-10-15 工具墙按风险等级筛选（R172）

- exe 工具卡片上有 low/medium/high 风险徽标，但没法用它们筛选——安全敏感
  的用户想「只看低风险工具」只能逐张看徽标。现网格标题旁加「全部风险 /
  低 / 中 / 高」chips（与搜索、隐藏推广可叠加）；缺 risk 字段按 low 处理
  （与卡片徽标兜底同口径）。
- 新纯函数 `filterToolsByRisk(list, filter)` + `RiskFilter`（catalog.tsx
  导出）；i18n `toolbelt.wall.riskAll`（zh/en 成对，等级标签复用 mkt.risk*）；
  CSS 复用 `.sys-filter-chips`。catalog.test.ts +3（530 总 / 38 files）。

### 2026-10-15 AI 分区按写入权限筛选（R171）

- 「AI 可操作」分区此前只能按搜索词过滤，卡片上的只读/写徽标无法用来筛
  选——想「先用只读的试试」得逐张看徽标。现加「全部 / 只读 / 写操作」chips，
  与搜索词可叠加（先权限后搜索）。
- 新纯函数 `filterAgentTools(list, filter)` + `AiPermFilter`
  （catalog.tsx 导出）；i18n `toolbelt.wall.aiPermAll/aiPermRead/aiPermWrite`
  （zh/en 成对）；CSS 复用 `.sys-filter-chips`。catalog.test.ts +4
  （527 总 / 38 files）。

### 2026-10-15 工具墙搜索覆盖厂商/版本/架构（R170）

- 工具墙搜索此前只匹配名称 / 描述 / 标签——按厂商（Intel、NVIDIA）、版本号
  或架构找工具都搜不到，而卡片上明明写着这些信息。现抽纯函数
  `matchesToolQuery(tool, q)` 统一口径，覆盖名称 / 描述 / 厂商 / 版本 / 架构 /
  分类 / 目录名 / 标签（null 安全，缺字段不炸）。
- catalog.tsx 导出该纯函数；toolwall.tsx 的 activeTools 过滤改用它；
  新增 catalog.test.ts +4（523 总 / 38 files）。

### 2026-10-15 工具详情显示可选架构版本（R169）

- 工具详情卡此前只显示当前架构（`arch`），而 `arch_variants`（同一工具带的
  全部可执行文件变体）**前端从未使用**——用户看不到"这工具还有 32 位版本 /
  另有一个 exe"。现多变体（>1）时列出「可选版本（N）：x86(xxx.exe) · x64(yyy.exe)」，
  只显示文件名不铺全路径。
- i18n `toolbelt.detail.archVariants`（zh/en 成对）；无新纯函数，测试维持
  519（37 files）。

### 2026-10-15 插件 min_app 兼容性校验（R168）

- `RegistryPluginVersion.min_app`（插件声明的最低应用版本）此前**两端都
  没使用**——后端不校验、前端不提示，用户可能在旧版 DiskPilot 上装到声明
  要求更高版本的插件（装了不一定能跑）。现前端在社区条目上做兼容性校验：
  不满足时显示「该插件要求应用 ≥ vX，当前 vY——先升级 DiskPilot 再安装」
  并禁用安装按钮（title 说明所需版本）。
- 新纯函数 `meetsMinApp(current, minApp)`（updateSkip.ts，复用
  `compareVersions`；未声明 / 当前版本为空 → true 不拦，避免锁死可用功能）；
  market.tsx 从 versions[] 取该版本的 min_app 判定；i18n
  `toolbelt.mkt.minAppTooOld(Title)`（zh/en 成对）。updateSkip.test.ts +4
  （519 总 / 37 files）。

### 2026-10-15 可选更新支持「只看驱动」（R167）

- Windows Update 可选更新列表里驱动与语言包/功能更新混在一起——`is_driver`
  字段此前前端从未使用，用户想找驱动更新得逐条看标题。现加「全部可选
  更新 / 只看驱动」chips（驱动数为 0 时后者禁用）+ 计数行「可选更新 N 项 ·
  其中驱动 M 项」；只有非驱动项时给「这批可选更新里没有驱动项」说明。
- 新纯函数 `driverOnlyUpdates(list)` / `wuUpdateCounts(list)`
  （SystemTools.tsx 导出）；i18n 4 个新键（zh/en 成对）。SystemTools.test.ts
  +4（515 总 / 37 files）。

### 2026-10-15 已装驱动加搜索与分类筛选（R166）

- 已装驱动列表此前整列展示（后端上限 200 条）——想确认"某设备的驱动装了
  哪个版本"只能一行行翻。现加分类 chips（按条数降序，含「未分类」）+ 搜索
  （匹配名称 / 厂商 / 版本）；0 命中给「换个关键词或切回全部分类」引导，
  与 R164 启动项同款口径。
- 新纯函数 `filterDrivers(list, query, cls)` / `driverClassCounts(list)`
  （SystemTools.tsx 导出）；i18n 4 个新键（zh/en 成对）；CSS 复用
  `.sys-filter-bar` 系列。SystemTools.test.ts +7（511 总 / 37 files）。

### 2026-10-15 启动项按来源分组（R165）

- 启动项列表此前混排（HKCU / 用户启动夹 / HKLM / 公共启动夹挤在一起）——
  「这条改的是我自己，还是全机」看不出来，而机器级改动影响所有用户、风险
  更高。现按来源分组并固定顺序：**用户级在前（HKCU → 用户启动夹）、机器级
  在后（HKLM → 公共启动夹）**，未知来源归「其他来源」排最后（不丢项）；
  组标题带条数。
- 新纯函数 `groupStartupByLocation(items)` + `STARTUP_LOCATION_ORDER`
  （StartupPanel.tsx 导出）；i18n `system.startup.locOther`（zh/en 成对）；
  CSS `.sys-group-head` / `.sys-group-n`。StartupPanel.test.ts +4
  （504 总 / 37 files）。

### 2026-10-15 启动项加搜索与状态筛选（R164）

- 启动项管理面板此前只能整列扫——几十项启动项时找某一项很累，也看不出
  「开机自启几项、停了几个」。现加顶部统计（「开机自启 N 项 · 已停用
  M 项」）+ 状态 chips（全部/自启中/已停用）+ 搜索框（匹配名称/命令/目标）；
  0 命中给「换个关键词或切回全部」引导，不退化成"没有启动项"。
- 新纯函数 `filterStartupItems(items, query, filter)` / `startupCounts(items)`
  （StartupPanel.tsx 导出）；i18n 6 个新键（zh/en 成对）；CSS
  `.sys-filter-bar` / `.sys-filter-chips` / `.sys-filter-search`。
  StartupPanel.test.ts（新）+8（500 总 / 37 files）。

### 2026-10-15 社区插件显示新版更新说明（R163）

- 社区条目在「有新版」时，于描述下追加该版本的更新说明
  （`RegistryPlugin.versions[].notes`）——用户在决定是否更新前该知道
  「这版改了什么」，否则只能盲点。没有说明的版本不出行（不编造）。
- 新纯函数 `versionNotes(plugin, version?)`（缺省取当前版本；版本对不上 /
  notes 为空 / 无 versions → null）；i18n `toolbelt.mkt.updNotesHead`
  （zh/en 成对）；CSS `.toolbelt-mkt-upd-notes`。market.test.ts +4
  （492 总 / 36 files）。

### 2026-10-15 已装台账排序（R162）

- 已安装台账此前按后端返回顺序排列——装多了以后「哪个该动」要一行行找。
  现按「有新版的最前 → 其余按安装时间倒序（最近装的在前）→ 同刻按名字
  升序」排序：用户唯一需要操作的对象（有新版那些）永远在顶部。
- 新纯函数 `sortInstalledPlugins(installed, hasUpdate)`（market.tsx 导出，
  hasUpdate 由调用方传入避免重复拉社区数据）；market.test.ts +4
  （488 总 / 36 files）。

### 2026-10-15 跳过更新带有效期（R161）

- R158 的「跳过此版本」是无限期的——安全补丁可能重要，用户也可能改主意。
  现记录跳过时刻，**满 90 天同版本也会重新提示**；出现更新版本仍立即提示。
  提示文案写明这一点（「90 天后或出现更新版本时会再提示」）。
- `updateSkip.ts`：新增 `parseSkipRecord` / `formatSkipRecord`（存储格式
  `版本@时刻ms`；**旧格式纯版本串仍能读**，atMs 0 视为不过期）+
  `UPDATE_SKIP_DAYS = 90`；`shouldPromptUpdate` 增加 skippedAtMs / nowMs
  参数。i18n updateSkipTitle 文案更新。updateSkip.test.ts +8（484 总 /
  36 files）。

### 2026-10-15 已装台账条目标「有新版」（R160）

- 已安装台账此前每行都有「更新」按钮，但没有任何「是否真有新版」的指示
  ——用户只能点一下试试（后端 update 会拉取最新版，无新版时是空操作）。
  现按 id 在社区数据里找同条目，远端版本严格更新时显示「有新版本 vX」
  徽标，且「更新」按钮从幽灵态升为强调态，一眼可见该动哪个。
- 复用 R158/R159 的 `hasRegistryUpdate` + `compareVersions`，无新纯函数，
  测试维持 476（36 files）。

### 2026-10-15 社区插件显示「有新版本」（R159）

- 插件市场的社区条目此前「已安装」即禁用按钮——本地版本落后于注册表时
  用户看不出来，也没有更新入口。现按 id 匹配已装台账，远端版本严格更新时
  显示「有新版本」实心徽标，按钮文案变「更新」且可点击（复用 R158 的
  `compareVersions`；仍走签名校验链路）。
- 新纯函数 `hasRegistryUpdate(plugin, installed)`（未装 / 缺版本号 / id 对
  不上 / 版本不新 → false，不误报）；i18n `toolbelt.mkt.updateAvailable` /
  `updateHint`（zh/en 成对，`update` 复用既有键）；CSS
  `.toolbelt-mkt-update-badge`。updateSkip.test.ts +3（476 总 / 36 files）。

### 2026-10-15 「跳过此版本」（R158）

- 设置页的更新提示新增「跳过此版本」——不想装某个版本的用户不必每次打开
  设置都被提示；跳过后该版本不再提示，**但出现更新的版本仍会提示**（跳过
  是版本相关的，不是永久静音）。已跳过时如实显示「已跳过 vX」并给「恢复
  提示」入口，不静默消失。
- 新文件 `src/updateSkip.ts`：纯函数 `compareVersions`（点分数字段比较、
  缺位补 0、非数字段按 0 不抛）+ `shouldPromptUpdate`（latest 严格新于跳过
  版本才提示）；localStorage 键 `settings.updateSkipped`。i18n
  `settings.general.updateSkip(SkipTitle/Skipped/Unskip)`（zh/en 成对）。
  updateSkip.test.ts（新）+8（473 总 / 36 files）。

### 2026-10-15 清理项显示上次清理的目录覆盖度（R157）

- 承接 R156：同一 scaffold 常有多个命中目录（多账号 / 多个缓存位置），
  只报「上次清理 3 天前」会让人以为整个 scope 都清过了。现多目录且未清全
  时补「N/M 个目录清过」，覆盖度不足一眼可见。
- 新纯函数 `cleanedHitStats(byPath, matchPaths)`（cleaned / total /
  latestTs；台账 null → cleaned 0 且 latestTs null 不编造）；CleanupBody
  的 lastCleanTs 改由它计算。i18n `cleanup.lastCleanedPartial`（zh/en
  成对）。cleanup/model.test.ts +4（465 总 / 35 files）。

### 2026-10-15 清理项显示该目录上次清理时间（R156）

- 清理页每个 scope 行在勾选前显示「上次清理 N 天前」——「这个目录刚清过
  还是很久没清」直接影响该不该再清；此前只有打开预览窗才能看到全局年龄。
- 新纯函数 `lastCleanByPath(entries, matchPaths)`（cleanup/model.ts）：按
  目录前缀把台账 source 归到扫描命中目录，取最新时刻；大小写/斜杠不敏感、
  嵌套目录取最长匹配根、相似前缀不误匹配（C:\App 不吞 C:\Apple）；glob
  展开只有后端能做，前端只做可靠的前缀匹配。store 在大小探测成功时顺带
  拉一次 list_undo(200)，失败保持 null（不显示而非编造）。
- i18n `cleanup.lastCleaned`（zh/en 成对）；CSS `.cleanup-row-last`。
  cleanup/model.test.ts +4（461 总 / 35 files）。

### 2026-10-15 预览窗显示上次清理时间（R155）

- dry-run 预览窗在用户决定删除前显示「上次清理：今天 / N 天前 / 超过一个
  月」——刚清完又清一遍是常见误操作；只调一次 list_undo(50) 取台账最新
  时间戳，读失败则不显示（不编造），不拦确认流程。
- 新纯函数 `lastCleanAgeKey(entries, nowMs)`（返回 i18n 键，文案由组件求值；
  空台账 / 时间戳不可解析 → null）；i18n `cleanup.previewLastClean` +
  `history.ageToday/ageDays/ageOld`（zh/en 成对）。model.test.ts +4
  （457 总 / 35 files）。

### 2026-10-15 周报环比显示间隔天数（R154）

- 周巡检简报的环比对比标题补「· 间隔 N 天」——对比结论的强度取决于跨度：
  间隔 7 天是标准周节奏，间隔 30 天则是「月度对比」，笼统说成「本周 vs
  上周」会误导；两份任缺一份、同刻或倒序时如实不显示。
- 新纯函数 `weeklyGapDays(cur, prev)`（ts 差换算天数；无效 → null）；
  i18n `history.weekly.vsGap`（zh/en 成对）。WeeklyHealthCard.test.ts +3
  （453 总 / 35 files）。

### 2026-10-15 AI 工具卡「描述待补」标注（R153）

- agent-server 的 MCP 工具描述可能为空或全是虚词——此前卡片静默缺一行、
  同类推荐也匹配不到，用户不知道为什么。现描述充分度不足（有效词 <2）
  时显示「描述待补」虚线徽标，悬停说明不影响使用、详情以参数表为准。
- 新纯函数 `agentDescQuality(desc)`（复用 descTokens：0 有效词 none /
  1 个 sparse / ≥2 rich）；i18n `toolbelt.wall.aiDescWarn(Title)`（zh/en
  成对）；CSS `.toolwall-ai-descwarn`。similar.test.ts +3（450 总 /
  35 files）。

### 2026-10-14 压测叠画标题显示次数与跨度（R152）

- 历次压测温度叠画（CPU/GPU）标题从只有「历次压测温度对比」扩展为
  「· 共 N 次 · 跨 M 天」——叠了几条、跨了多久决定散热对比结论的强度
  （2 次同小时的对比与 8 次跨一个月的演变不是一回事）。
- `StressHistorySeries` 新增可选 `at`（该次压测开始时刻，来自
  BenchHistoryEntry.at）；跨度复用 R144 的 `scoreSeriesSpanDays`（<2 次
  或全同刻 → null 不显示，与跑分图同口径）。i18n
  `stressChart.histCount` / `histSpan`（zh/en 成对）。

### 2026-10-14 周报巡检到期提示（R151）

- 周巡检简报卡的日期行新增「巡检到期」提示：距上次生成满 7 天显示
  「该重新巡检了」（警示色），未到期显示「N 天后到期」（弱化）——周报
  按「周」节奏，用户此前只能自己算「上次是几天前」。
- 新纯函数 `nextInspectionDue(ts, nowSec)`（满 7 天 due / 未满 daysLeft /
  时间戳无效 → null 不编造）；i18n `history.weekly.dueNow` / `dueIn`
  （zh/en 成对）；CSS `.wh-due` / `.wh-due-ok`。WeeklyHealthCard.test.ts
  +5（447 总 / 35 files）。

### 2026-10-14 清理历史统计跟随筛选集（R150）

- 清理历史页头部「累计释放」「最早日期」从永远基于全量改为基于**当前
  筛选集**——筛选 recycle / 关键词后，统计反映筛选结果而不是与列表脱节；
  同时「共 N 条」在有筛选时补一句「筛出 M 条」，避免用户以为漏了记录。
- 各动作 chip 计数仍显示全量（筛选器需展示每个动作各有多少条，否则筛
  选后其他 chip 变 0 无法切换）。i18n `history.filteredN`（zh/en 成对）。

### 2026-10-14 清理历史台账显示时间范围（R149）

- 清理历史页头部「累计释放 X」旁新增「最早 YYYY-MM-DD」——台账的累计
  数字现在有尺度：这些记录是从哪天开始攒的（悬停显示起止范围）。读取
  失败 / 空台账 / 时间戳全不可解析时不显示（诚实：没读到就不编日期）。
- `summarizeUndo` 新增 `oldestAt` / `newestAt`（按可解析时间戳取 min/max，
  不依赖列表顺序）；`fmtDay` 格式化本地日期。i18n `history.rangeFrom` /
  `rangeTitle`（zh/en 成对）。model.test.ts +2（442 总 / 35 files）。

### 2026-10-14 空间耗尽预测按样本充分度降级（R148）

- 承接 R145 的采样充分度：空间趋势行样本少（2–4 点，sparse）时，原有的
  「约 N 天满」外推预测从「≈ full in N days」降级为「样本少，粗估 N 天满」
  ——外推建立在首末两点上，样本少时预测不可靠，照常显示等于默认它可信。
- 新纯函数 `forecastConfidence(samples)`（rich→high / sparse→low /
  none→none）；i18n `overview.trend.fullInRough`（zh/en 成对）。
  AssetOverview.test.ts +3（440 总 / 35 files）。

### 2026-10-14 exe 工具详情显示「AI 里也能做」（R147）

- exe 工具详情卡在「同类建议」（tags 重叠）之下新增「AI 里也能做」区块
  ——用该工具的名称 + 描述 + 标签拼成查询串，与 agent-server 的 MCP 工具
  描述做词袋匹配（复用 R134 的 `similarAgentTools`），命中则列出可点击的
  AI 工具名，点击直接切到该 AI 工具的详情卡。
- 补上了「exe 工具墙 ↔ AI 分区」两个此前互不相通的入口。i18n
  `toolbelt.detail.aiSimilarTitle`（zh/en 成对）。

### 2026-10-14 硬件趋势每条线显示有效读数个数（R146）

- 硬件趋势标题的「N 份快照」是全部快照数，不代表每条线有多少真实读数
  ——温度列可能只有 2 个有效值而通电时长有 20 个，两条线可信度不能混为
  一谈。现每条折线行尾显示自己的有效读数个数，2–4 个（sparse）用警示色
  并悬停说明「该列缺值的快照不着点」。
- 新纯函数 `hwSeriesQuality(vals)`（≥5 rich / 2–4 sparse / <2 none，非
  有限值不计）；i18n `hw.trendPointsTitle`（zh/en 成对）；CSS
  `.hw-trend-points(.sparse)`。HwPanels.test.ts +4（437 总 / 35 files）。

### 2026-10-14 空间趋势行显示采样点充分度（R145）

- 空间趋势每行在「+X GB / N 天」旁显示采样点数（悬停说明）——2 个点
  算出的「每天涨 X GB」与 30 个点算出的完全不是一回事，样本少时趋势
  与外推不该当结论。点数 2–4 时用警示色标记（`sparse`）。
- 新纯函数 `spaceTrendSamples(pts)`（≥5 rich / 2–4 sparse / <2 none，
  非有限 at 滤掉）；i18n `overview.trend.samplesRich` / `samplesSparse`
  （zh/en 成对）；CSS `.ao-trend-samples(.sparse)`。AssetOverview.test.ts
  +4（433 总 / 35 files）。

### 2026-10-14 跑分历史显示时间跨度（R144）

- 分数历史条形图图例行从「共 N 次 · 单位 X」扩展为「· 跨 D 天」——跨度
  长说明机器状态有漂移，「巅峰 vs 最近差距」的解读要打折扣（承接 R138
  的尺度标注思路）。
- 新纯函数 `scoreSeriesSpanDays(series)`（at 毫秒 → 天数；<2 次 / 全同
  时间 / 非有限值 → null）；不足一天不显示。i18n
  `toolbelt.scoreChart.spanLabel`（zh/en 成对）。charts.test.ts +2
  （429 总 / 35 files）。

### 2026-10-14 分布色阶条显示样本跨度（R143）

- 分布色阶条（CPU 温度 / GPU 温度 / 内存占用）的摘要行追加「· 历时 X」
  ——占比要有尺度参照：1 小时里的 3% 与 1 个月里的 3% 含义完全不同。
- `DistributionBar` 新增可选 `spanSec` prop（≥60s 才显示，复用 R138 的
  `formatSpan`）；bench.tsx 三处分布条共用一处 `trendSpanSec` 计算（替代
  此前评级卡里的内联调用）。i18n `toolbelt.dist.span`（zh/en 成对）。

### 2026-10-14 Esc 关窗推广到工具墙/基准确认窗（R142）

- 承接 R141 的 `useEscapeKey`，推广到 6 个此前只能用鼠标关闭的弹层：
  基准面板的压测确认 / 磁盘真跑确认 / 甜甜圈确认，工具墙的启动确认 /
  exe 详情卡 / AI 工具详情。Esc 一律是「取消」——不会启动任何工具，
  启动确认在 `launching` 期间禁用 Esc（避免中途打断）。

### 2026-10-14 Esc 关窗收敛为共用 hook（R141）

- ContextMenu / SteamWorkshopModal / DryRunPreviewDialog / gpu-donut 此前
  各自手写一份 Esc keydown 判断（4 处重复、细节不一致），收敛到新文件
  `src/escapeKey.ts`：纯函数 `isEscapeClose(key, enabled)` +
  `useEscapeKey(onClose, enabled)`（onClose 走 ref 存最新值，effect 只依赖
  enabled，避免反复重挂监听）。DryRunPreviewDialog 的 Esc 分支改用 hook，
  Enter 逻辑（进预备态 / 真删）保留不变。
- escapeKey.test.ts（新）：+3 tests（427 总 / 35 files）。

### 2026-10-14 预览窗键盘快捷键（R140）

- dry-run 预览窗支持键盘流（桌面应用惯性预期，且比鼠标更防误触）：
  **Esc** 关窗返回（任何时候都安全，绝不会删东西）；**Enter** 未预备时
  「进预备态」（与点按钮等价，不直接真删），已预备时「真删」（等价于
  5 秒内再点一次，仍是第二步确认）。未勾备份确认时 Enter 同样被拦。
- 用 ref 存最新闭包，effect 只挂一次监听；页脚追加弱化提示
  （i18n `cleanup.kbdHint` zh/en + `.cleanup-kbd-hint`）。

### 2026-10-14 评级卡样本偏短提示（R139）

- 长期健康度评级卡新增诚实提示：样本跨度不足一天（`spanSec < 86400`）时
  显示「样本跨度仅 X——样本偏短，评级仅供参考」——几小时的样本称不上
  「长期」，结论的前提条件必须说清（承接 R138 的 trendSpan）。
- HealthRatingCard 新增可选 `spanSec` prop（bench.tsx 传
  `trendSpan(trendData.points)?.spanSec`）；i18n
  `toolbelt.healthRating.shortSpan`（zh/en）；CSS `.health-rating-warn`
  用 warn 语义色。charts.test.ts +2（424 总 / 34 files）。

### 2026-10-14 长期趋势图显示时间跨度（R138）

- 传感器趋势折线图图例行从只显示「N 个采样点」扩展为「· 跨度」（如
  · 3d 5h）——500 条样本跨 3 小时还是 3 周，直接决定「长期健康度」
  结论的可信度；悬停显示平均采样间隔（`toolbelt.trend.spanTitle`）。
- 新纯函数 `trendSpan`（首末 ts 差 = 跨度、间隔数均差；<2 点 / 全同 ts /
  非有限 ts → null）+ `formatSpan`（天/时/分/秒四档人话）。charts.test.ts
  +5（422 总 / 34 files）。分钟级以下跨度不显示（无信息量）。

### 2026-10-14 健康度评级卡行渲染收敛（R137）

- HealthRatingCard 内三段几乎逐字重复的 JSX（label + 档位徽标 / 无数据）
  收敛为 `RatingRow` 单行组件 + 数据驱动 rows 渲染——改文案或结构从
  改 3 处变改 1 处；GPU 维度 `gpuTemps ?? []` 走同一条 null 判定路径
  （空样本 → distSummary null → 不评级，行为不变）。

### 2026-10-14 长期历史三路 series 一次提取（R136）

- 长期历史视图里 cpu/gpu/mem 三路提取（`.map().filter()`）此前在
  HealthRatingCard + 3 个分布条各自内联（5 处重复同一段），收敛为
  纯函数 `extractLongSeries(points)` 一次提取、五处复用——读不到的
  字段（null/缺失/非有限值）如实滤掉不造 0，分布卡各自判空渲染。
- charts.test.ts +3（滤 null/空点列/纯函数，417 总 / 34 files）。

### 2026-10-14 AI 分区卡片显示相关工具（R135）

- 「AI 可操作」分区的每张卡片底部显示 top-2 相关工具名（复用 R134 的
  `similarAgentTools` 描述词袋打分，useMemo 缓存 O(n²) 结果）——列表层
  即见关联，不必逐个点开详情弹窗。无关联的卡不显示该行。
- i18n `toolbelt.wall.aiRelated`（zh/en 成对）；CSS `.toolwall-ai-related`
  单行截断防撑破卡片。

### 2026-10-13 AI 工具详情「同类推荐」（R134）

- AI 可操作工具的详情弹窗（agentDetail）底部新增「同类 AI 工具」——
  agent-server 的 MCP 工具没有 tags/category 元数据，同类判断靠描述词袋
  重叠打分（新纯函数 `similarAgentTools` + `descTokens`/`descOverlap`）。
  点击推荐项直接切换详情。
- 分词策略：拉丁/数字连续串一个 token；CJK 走双字滑窗组词（「温度」
  「获取」），比单字切分更贴近概念——「温度」vs「湿度」共享单字不再
  误判同类。中英停用词表（虚词/高频词），无标签描述不硬凑。
- 新文件 similar.ts + similar.test.ts（+9 tests，414 总 / 34 files）；
  i18n `toolbelt.wall.aiSimilar`（zh/en 成对）。

### 2026-10-13 甜甜圈压测温度曲线全长历史（R133）

- 退出摘要的「温度曲线」行从「最近 60 个采样窗口」改为全长历史——
  手动长压测（R111 支持任意时长）不再丢前段升温过程（室温→爬升→
  稳态）；峰值也从全长取（开头瞬时高温不会丢）。HUD 趋势线仍为
  60s 滚动窗口（画布小，只需近段）。
- 熔断场景（90℃ 必然在 60s 内触发）不受影响。gpu-donut.test.ts +1
  （>60 样本完整往返，405 总）。

### 2026-10-13 清理预览窗大小口径说明（R132）

- dry-run 预览窗的路径列表上方加一行说明：行内标注的是本次清理实际
  释放空间（dry-run 实测），头部合计含扫描估算，两者可能略有出入——
  诚实呈现口径差异，避免用户对「行内大小之和 ≠ 合计」产生困惑。
- i18n：`cleanup.previewSizeNote`（zh/en 成对）；样式
  `.cleanup-preview-size-note`。

### 2026-10-13 清理预览窗显示单文件大小（R131）

- 清理页 dry-run 预览窗的路径清单每条在行尾显示「实际释放字节」——
  大文件一眼可见；bytes_freed 缺失（后端未统计）的行省略大小。
- `DryRunPreview` 新增 `sampleEntries`（与 samplePaths 同序的路径+字节
  条目）；抽纯函数 `sampleEntriesByBytes`（与 samplePathsByBytes 同排序
  规则的条目版本）+4 tests（404 总 / 33 files）。预览行 flex 布局：
  路径省略 + 大小右对齐。

### 2026-10-13 清理预览路径按字节降序（R130）

- 清理页 dry-run 预览窗的路径清单（前 80 条）从「后端返回顺序」改为
  「实际释放字节降序」——大文件优先展示，用户一眼看到本次清理占最多
  空间的匹配项；bytes_freed 缺失（后端未统计）的沉底且保持相对顺序。
- 探活时保留 UndoEntry.bytes_freed（此前类型注解丢弃）；抽纯函数
  `samplePathsByBytes(entries, cap)`（cleanup/model.ts，默认 cap=80）+
  （+4 tests，400 总 / 33 files）。

### 2026-10-13 压测叠画 y 轴动态上界（R129）

- 历次压测叠画（CPU/GPU）y 轴从固定 0–100 改为动态上界 = max(100, 全部
  系列峰值)——长时间压测曲线罕见超 100°C 不再被顶部截断；正常 ≤100
  的显示与固定 0–100 完全一致（视觉口径不变）。
- 抽纯函数 `stressHistYMax` + 新测试文件 charts.test.ts（+3 tests，
  396 总 / 33 files）。

### 2026-10-13 dry-run 预览按字节降序（R128）

- 总览「建议清理」展开明细按可回收字节降序——大文件优先展示
  （PEEK_LIMIT 截断时留下的是占用最大的项，而不是后端返回顺序里的
  前几条）；bytes 缺失（后端未统计）的项沉底且保持相对顺序。
- 抽纯函数 `sortPreviewItemsByBytes`（不改入参数组）+ 单测
  （+3 tests，393 总）。

### 2026-10-13 GPU 压测历史叠画标题区分（R127）

- 甜甜圈历史叠画标题改为「历次 GPU 压测温度对比」——CPU（stress_test）
  与 GPU（gpu_donut）两个叠画区块同页并存，标题不再混淆；aria 同步。
- `StressHistoryChart` 加 `kind` prop（默认 cpu，向后兼容）。

### 2026-10-13 建议清理按可回收字节降序排序（R126）

- 总览「建议清理项目」按可回收字节降序排列——最大收益项（微信缓存/
  大块头）排最前，用户一眼看到哪些能清出最多空间；后端顺序不稳定
  （map 迭代序），前端统一口径。
- 抽纯函数 `sortCleanItemsByBytes`（不动原数组）+ 单测（+3 tests，390 总）。

### 2026-10-13 工具详情「同类建议」（R125）

- 详情卡新增「同类建议」区块：按 **tags 重叠**推荐其他工具（排除自身，
  按共享 tag 数排序取前 5），点击直接切换详情——看一个工具时顺带发现
  同场景的其它选择，不再需要退出搜索。
- 复用目录内既有 tags 数据（tool.tags / manifest.tags），无 tags 的工具
  如实不显示区块。

### 2026-10-13 甜甜圈压测温度曲线可视化 + 历次 GPU 压测叠画（R124）

- **曲线不丢**：甜甜圈退出把每秒温度采样序列化成「温度曲线」行（同
  stress_test 契约，`温度曲线：0.0s=…; 1.0s=…`），结果区渲染 GPU 温度
  折线（GPU 蓝，与传感器趋势图同色系）——之前曲线只在 HUD 短暂显示，
  退出即丢。
- **历次 GPU 压测叠画**：甜甜圈历史（localStorage）同图对比最近 8 次
  温度曲线（复用 R113 叠画组件，Y 轴 0–100℃ 可比），GPU 散热随时间的
  演变一眼可见。
- 纯函数 `tempHistCurveLine`（采样 → 曲线行）+ 双向契约单测：序列化
  后可被 `parseStressCurve` 原样解析回（+3 tests，387 总）。
- `StressTempChart` 加 `kind` 参数（cpu/gpu），图例/配色区分。

### 2026-10-13 甜甜圈压测摘要带 GPU 温度峰值 + GPU 压测温度曲线（R123）

- **甜甜圈（WebGL）**：退出摘要新增「峰值 GPU 温度 {temp}℃」——压测期间
  每秒轮询采样（R112 的温度趋势窗口）有值才如实带出，没采到温度（无 N
  卡 / hw_temperature 失败）不虚构，只报 FPS；温度窗口改由 ref 镜像，
  退出闭包能读到最新采样。
- **stress_test_gpu（后端）**：守门采样循环收集「温度曲线：0.0s=xx°C;
  1.0s=xx°C; …」行（同 stress_test 契约，前端 parseStressCurve 可直接
  复用），压测前温度作为 0s 起点基线。

### 2026-10-13 传感器通道纳入 GPU 温度（R122）

- **后端**：`sensor_trend` / `sensor_history` 快照新增 `gpu_temp_c` 字段
  （N 卡 nvidia-smi / A 卡 ADL 双通道，`hw::gpu_temp_c`）；JSON 契约
  `ts + cpu_temp_c + gpu_temp_c + mem_percent` 全可空，无 GPU/读失败记
  null 不虚构；text 输出加「/ GPU x.x℃」。历史旧行缺字段 → null，前端
  只画有值曲线（诚实性原则）。
- **前端**：趋势折线图加第三条 GPU 温度线（蓝）；长期视图健康度评级卡
  新增「长期 GPU 温度」维度（≥70℃ 高值占比，三档评级，无数据如实不
  评级）；修复护顶峰显示「CPU x℃ / GPU y℃」双值；新增 GPU 温度分布
  色阶条（同 heatDist 区间口径，无 GPU 读数不渲染）。
- 后端单测补 `gpu_temp_c` 契约断言；前端解析测试更新（R122 起含 GPU）。

### 2026-10-13 跑分峰值与最近差距结论（R121）

- **巅峰结论行**：跑分历史条形图（R115）下方新增「巅峰 {value} {unit}
  （{date}）」——历次最高分是哪次跑的、拿了多少，一眼落定；不足 2 次不
  对比。最近一次低于巅峰：「低 {pct}%」（琥珀）；已达或超过巅峰：「已达
  巅峰」（绿，不唱衰）。
- 纯函数 `scoreBest`（巅峰、并列取更近）/ `scoreRecentDelta`（低多少的
  正值，超巅峰 clamp 0）锁语义；3 个新单测（32 files · 384 tests）。

### 2026-10-13 工具墙搜索覆盖 AI 工具分区（R120）

- **搜索一次搜遍两个分区**：工具墙顶栏搜索此前只过滤 exe 工具墙，「AI 可操作」
  分区（agent-server 全部 MCP 工具）在搜索时整区消失——用户搜「sensor」「disk」
  看不到 AI 侧命中。现在搜索词同样按 name/description 过滤 AI 工具，两个分区
  一起搜；无词时照常整区展示，0 命中不显示空分区。
- 纯前端增强，复用现有搜索框与过滤口径。

### 2026-10-13 空间耗尽风险分级（R119）

- **耗尽风险分级**：R98 的「约 N 天满/已满」预测标签从单一琥珀色升级为三档
  视觉分级——≤30 天或已满红色加粗（critical）、≤90 天琥珀（warn）、余量
  充足绿色（ok）；预测不可用（释放中/不足 2 点）如实显示 ok 不唱衰。
- 纯函数 `spaceRisk` 锁定档位（null/30/90 边界）；4 个新单测
  （32 files · 380 tests）。

### 2026-10-13 长期健康度评级卡（R118）

- **评级卡**：长期历史视图顶部汇总一句人话结论——温度 / 内存各自把高值占比
  （≥70℃ / ≥85%）映射成评级：<5% 优秀、<20% 良好、否则关注；徽标按档位取
  --risk-safe / --risk-caution / --risk-danger 语义色。先出结论，再看下方
  分布明细；某一维度无有效样本如实显示「无数据」不评级。
- 纯函数 `rateHealth` 锁定档位（<5% / <20% / 否则）；2 个新单测
  （32 files · 376 tests）。
- 长期历史解读闭环收束：R114 只读历史 → R116 温度分布 → R117 内存分布 →
  R118 健康度评级。

### 2026-10-13 内存占用分布（R117）

- **分布条泛化**：R116 的温度分布条重构为通用 `DistributionBar`（ramp 区间
  色阶 + 高值阈值 + i18n 命名空间三参数注入），温度 / 内存两路复用；`buildBuckets`
  / `distSummary` 纯函数替代 `buildTempBuckets`/`tempSummary`（旧名薄封装保留，
  R116 单测与接线不动）。
- **内存占用分布条**：长期历史的 `mem_percent` 样本按 <50 / 50–70 / 70–85 /
  85–95 / ≥95% 分层（`MEM_RAMP` 单源，阈值口径与 sensor_alert 内存告警一致），
  摘要 + ≥85% 吃紧占比解读——「32GB 日常够不够挤」一眼可见。
- 单测 +4（泛化分层边界与阈值注入语义，32 files · 374 tests）。

### 2026-10-13 温度分布色阶条（R116）

- **长期历史温度分布条**：点「长期历史（只读）」后，除折线回放外再渲染一条
  分段色阶带——把全部历史 CPU 温度样本按 <50 / 50–60 / 60–70 / 70–80 / ≥80℃
  分层（色阶从清凉绿到过热红，阈值口径与告警一致），下方图例带各区间样本数
  + 峰值/均值/样本数摘要 + ≥70℃ 高温占比解读（>10% 才提示，否则如实「没有」）。
- 纯函数 `buildTempBuckets` / `tempSummary` 锁定分层与统计口径（非有限值跳过
  不猜 0、负值合法落入低温桶）；7 个新单测（32 files · 370 tests）。
- 色阶收敛到 colors.ts 单源 `TEMP_RAMP`（沿用 W4 色板收敛原则）。

### 2026-10-13 基准跑分历史对比（R115）

- **历次基准跑分对比条形图**：bench_cpu / bench_memory / bench_disk 的历史
  结果文本里已存分数（整数 ops/s、读带宽 MB/s、顺序读 MB/s），此前只折叠成
  文本逐条看。现在解析历次跑分画成「旧→新」条形图，最佳那次用强调色标出，
  Y 值缩放取最高/最低分——睿频/散热/后台负载对分数的影响、分数随时间漂移
  一眼可见。
- 解析纯函数 `parseScoreLine` / `buildScoreSeries` 锁定后端输出格式契约
  （含千分位逗号与科学计数法；无分数行如 dry-run 计划 / 被拒 / 失败条目如实
  跳过不猜 0）；6 个新单测（32 files · 363 tests）。
- 纯前端增强：零后端改动，数据源就是 localStorage 里已积累的跑分历史。

### 2026-10-13 压测温度可视化三件套（R111–R113）+ 只读传感器历史（R114）

- **压测实时温度监护 + 可选手动时长**（`b387d3d`）：CPU 压测期间并行轮询只读
  `sensor_trend`（每 2s，画实时温度曲线 + 峰值）；时长可选 5/30/60/120/300s
  （AIDA64 风格），确认弹窗同步显示所选时长。
- **GPU 甜甜圈 HUD 温度趋势线**（`841406b`）：GPU 压测 HUD 增加温度 spark 趋势
  （最近 60 点）+ 当前温度区间；温度读数缺失的点不虚构（诚实性约定）。
- **历次压测温度叠画对比**（`34b503a`）：把历史压测的温度曲线叠画在 0–100℃
  统一坐标系（≥2 条才渲染，最多 8 条，最近最亮），图例带时间 + 各次峰值——
  散热状态/峰值随时间的演变一眼可见。
- **只读 `sensor_history` 工具 + 长期历史视图**（本轮）：新增纯只读 MCP 工具
  `sensor_history`（读 `sensor-history.jsonl` 最多 500 条，不追加不裁剪——
  与会追加快照的 `sensor_trend` 分开，页面展示无写副作用）；趋势卡片新增
  「长期历史（只读）」按钮 + 非等时轴说明 + 空态区分；Rust 单测
  `points_from_rows_takes_tail_and_keeps_order` 锁定取尾/顺序/上限语义。

### 2026-10-13 维护

- **MCP 补齐 resources 与 prompts 能力面**：此前只实现 `tools`，现新增三个
  只读资源（`system://profile` 系统摘要 / `system://disk-usage` 磁盘分区用量 /
  `system://env` 关键环境变量，内容与同名工具同一口径）与两个任务模板
  （`system-report` 体检报告 / `cleanup-plan` 目录清理计划，供 AI 客户端一键填入），
  能力声明与 instructions 同步补全。协议冒烟实测通过。
- **Linux 交叉编译 cfg 技术债全量清偿**：Windows-only 子系统（hw/sys/sec/net/
  diskx 等）的 import / re-export / PS 常量 / helper 函数按消费方补
  `#[cfg(windows)]` 或 `#[cfg_attr(not(windows), allow(dead_code))]`，
  累计 49 个文件；`cargo check --target x86_64-unknown-linux-gnu --all-targets`
  从 25 errors + 120 warnings 清到 **0 errors 0 warnings**（agent-server 及
  全部 7 个 workspace crate），Windows 侧 check/test/clippy/fmt 全绿。

### 代码审查修复汇总（第 1–9 轮）

对全仓库（desktop 应用 / agent-server / scanner / executor / toolbelt）执行了
9 轮深度代码审查，覆盖：PS 注入面、写路径确认门、SSRF/路径守卫、资源上限、
二进制解析、前端 XSS/竞态、UI/UX 视觉、整体架构。以下为每轮修复清单。

#### 第 9 轮（整体架构优化）

**架构重构（消除跨进程重复）** 新增共享 crate `crates/psutil`（diskpilot-psutil），
把 desktop 与 agent-server 双份维护的 PowerShell 原语收敛为单源：

- **修复（高）** `ps_capture` 执行器双份漂移：desktop `hw.rs` 版无 UTF-8 首行
  注入（中文系统 GBK 乱码只能靠兜底解码），agent-server `ps.rs` 版有改良
  ——统一迁入 psutil（UTF-8 注入 + GBK 兜底 + CREATE_NO_WINDOW + 超时强杀），
  desktop 保留 `(script, timeout)` 两参薄封装转发，**desktop 同时获得 UTF-8
  根治能力**（此前缺失）；agent-server `ps.rs` 变为纯 re-export 薄壳，
  两调用面不变。
- **修复（高）** AMD ADL 显卡探针脚本 `ADL_PROBE_PS`（2927 字符）desktop
  `hw.rs` 与 agent-server `tools/hw.rs` **逐字节相同却双份维护**，任何一侧
  修改即漂移——迁入 psutil 单源，两侧 `use` 同一常量。
- **修复（中）** `escape_ps_single_quoted`（PS 单引号转义，命令注入铁律）
  desktop `startup.rs` 与 agent-server `sys.rs` 双份——迁入 psutil 单源；
  回归测试随迁（psutil 新增 +2 测试，desktop 侧删本地重复测试）。
- **净效果**：`-372/+66` 行（抽取共享），workspace 零横向依赖约束保持
  （psutil 为纯叶 crate，仅依赖 encoding_rs）。
- **验证**：`cargo check/clippy --workspace --all-targets` 0 新警告；
  workspace 测试全绿（psutil 2 项新测试通过）；前端 tsc/eslint/vitest 不受影响。

**架构审查结论（本轮确认健康，无需改动）**
- AppState 短生命周期作用域锁、无 await 持锁——并发模型无死锁面；
- 前端 `api.ts` 按域聚合、`format.ts` 字节格式化单源——无跨文件重复；
- workspace 各 crate 零横向依赖（ci.yml 声明与实现一致）；
- agent-server 13 个细分采集脚本与 desktop 3 个聚合脚本为不同封装层级
  （面向不同消费方），非重复，内容级比对无 >50% 重叠。

> 第 6–8 轮（UI/UX 视觉审查，`e403751` `d4a01bc` `740ba54`）见下方
> 「UI/UX 视觉审查」section。

#### 工程专项：fmt 存量清零（`b4c5a9a`）

- **修复（工程·中）** 全 workspace 43 处存量 fmt 差异（19 文件，HEAD 即
  dirty）——CI 三平台 `cargo fmt --all -- --check` 一直在红，护栏形同虚设；
  `cargo fmt --all` 机械重排清零，clippy/test 验证全绿。
- **修复（工程·低）** `rustfmt.toml` 的 `imports_granularity` / `group_imports`
  为 nightly-only 选项，stable rustfmt 静默忽略并持续告警，CI（stable）永远
  无法验证该声明——移除，stable 语义成为事实标准。

#### 工程专项：重复代码收敛（`b9cb596` `7f79811`）

- **修复（中）** agent-server `bsod.rs` 本地 `fmt_bytes`（GB/MB/B 三档）与
  `tools/mod.rs` 共享版（B/KB/MB/GB/TB 五档）重复——删除本地版改用共享版
  （净 -12 行；dump 大小 1KB~1MB 区间输出从原始字节变 KB，更可读）。
- **修复（低）** 前端 `extOf`（取扩展名）在 FileView/TreeView 各有一份私有
  实现——逻辑收敛 `format.ts` 单源（无扩展名返回 `''`），FileView 保留
  `'(none)'` 分组哨兵映射；+2 单测。
- **排查排除**：`emit` 3 行惯用模式（i18n/hwCache）不抽公共层（收益为零）；
  `ToolCard` 两组件同名不同义（目录卡 vs 图标卡）非重复；`fmtTime`/
  `relativeTime` 输出格式与 i18n key 不同，非真重复。

#### 工程专项：巨型文件拆分（`7568df9` `3af6564` `6e6f7d0`）

- **修复（工程·中）** desktop `hw.rs`（2259 行）按边界拆分：报告 Markdown/
  HTML 格式化（`html_escape`/`html_wrap`/`is_vram_overflow`/`fmt_gb`/`v_str`/
  `arr`/`build_hw_markdown`）与风扇建议规则（`build_fan_advice`）为无状态纯
  函数，连同 12 条单测搬入新模块 `hw_report.rs`；`hw.rs` 保留 Tauri 命令层，
  降到 1684 行。
- **修复（工程·中）** desktop `mcp.rs`（1156 行）拆分：stdio/http 连接与
  校验（`validate_http_url`/`validate_stdio`/`connect_stdio`/`connect_http`/
  `connect_server` + `McpService` 别名）连同 2 条单测搬入 `mcp_bridge.rs`；
  `mcp.rs` 降到 921 行。
- **修复（工程·中）** desktop `ai.rs`（944 行）拆分：免 key 联网搜索
  （`web_search` 命令 + `SearchHit` + `bing_parse`/`ddg_lite_parse`/
  `html_decode`/`strip_tags`/`attr_value`/`text_between`/`url_encode`/
  `fetch_limited`）搬入 `ai_net.rs`；`ai.rs` 保留请求代理层（SSRF 防护/
  手动重定向/取消），降到 723 行。命令注册改 `ai_net::web_search`。
- **方式**：同 crate 内模块移动、`pub(crate)` 导出、零行为变化——仅移动
  不改逻辑；130 passed / 2 ignored，clippy/fmt 全绿。
- **排查排除**：`cleanup.rs`（1009 行）命令层与逻辑层交织（命令内联引用
  多个逻辑函数、测试混居）；`plugin_registry.rs`（1546 行）类型/命令/
  持久化高度交织（`RegistryEnvelope`/`RegistryPlugin`/`LedgerEntry` 等
  结构体 + `verify` 验签内聚在头部），两者拆分收益递减风险上升，留待后续。

#### 第 5 轮（`d0815a5`）

- **修复（Bug 风险）** `agent-server report.rs` `extract_free_gb`：
  - 磨损行「寿命剩余约 92%」被误判为剩余容量（8 → 8.0 GB），健康 SSD 触发
    虚假的「磁盘剩余低于 20GB」告警；
  - 「已用 300GB / 剩余 120GB」结构取行首**已用**值而非**剩余**值；「扫同一行
    所有数字取最后」的兜底分支因首数字几乎总存在而**永远不可达**（死代码）。
  - 修复：跳过含 `%` 行、要求容量单位（GB/TB）、以关键词为锚取词后数值、
    兜底改取整行最后一个有效数字；+6 断言（含磨损反例与已用/剩余结构）。
- **修复（Bug 风险）** `mcp.rs` `mcp_call_tool` 无操作级超时：
  - 连接 + `tools/call` 无任何超时包裹，挂死的 MCP 服务器（stdio 子进程阻塞 /
    HTTP 无响应）会**永久卡住整个 AI 回合**（后端 await 不返回、前端 busy 永不清除）。
  - 修复：端到端 60s 兜底（probe/list 原有 10s 不变）；`timeout_secs=0` 在
    reqwest 语义是「立即超时」而非「不限时」——把一切请求打失败，显式按未设置处理。

#### 第 4 轮（`311f775`）

- **修复（安全漏洞·高）** `startup.rs` 注册表启动项值名 → **PowerShell 注入**：
  - 启停/删除启动项把**注册表值名**（外部数据——恶意软件可写成
    `x'; <任意 PS>`）直接拼进 PS 单引号字面量，操作本身成为代码执行入口；
  - 修复：新增 `escape_ps_single_quoted`（单引号翻倍，与 agent-server `sys.rs`
    同一条铁律）并接入三处拼接；+2 回归测试钉死恶意值名转义后无法闭合字面量。

#### 第 3 轮（`5ed2e10`，含第 1–2 轮全部修复）

**安全漏洞**
- `sys.rs`：AI 辅助工具拼接用户输入进 PowerShell 单引号串**未转义**（注入面，高）
  ——新增 `escape_ps_single_quoted` + 回归测试。
- `net.rs`：Wi-Fi 名称同理注入（高）——转义 + 测试。
- `mcp.rs`：本地命令/参数含 `\x00` 时 spawn 永远失败并遗留僵尸服务器（中）
  ——配置层拒绝 NUL + 测试。
- `advisor agent.ts/provider.ts`：AI 请求头 `x-goog-api-key` 提供方写死，改配置透传。

**Bug 风险**
- `executor.rs`：dry-run 清理把「未确认的路径」写进 undo 记录，撤销时错误恢复（高）
  ——dry-run 不产 undo；已确认路径才记录。
- `store.ts`：bytes_freed 在多轮清理后重复累加（清理统计虚高）。
- `ai.rs`：流式响应 **10MB 无上限**（多请求共吃内存）。
- `hwinfo.rs`：共享内存布局解析 `try_into().unwrap()` 前**偏移无界**（长度前检）。
- `mft.rs`：MFT 解析两处 `try_into().unwrap()` 前长度边界（加前检）。

**性能问题**
- `ai.rs`：跨主机重定向跟随（凭证/私有地址泄露风险）——禁自动重定向 + 逐跳校验。
- `cleanup.rs`：目录 walk 无上限（超大目录 OOM）——加深度/条目上限。
- `files.rs`：读文件**整块进内存**——改流式 + `MAX_DIR_ENTRIES=100_000` 上限。

**代码风格 / 改进建议**
- `process.rs`：`$pid`（PS 自动只读变量）改 `$targetPid`。
- `hw.rs`：`is_vram_overflow` 用 float 算术比较判断 vram 溢出（改用整型）。
- `bench.tsx`：GPU 压测历史 shape 与后端不一致（修复持久化/回读）。
- `lib.rs`：scaffold 加载遇坏文件直接失败（改跳过坏文件）。
- `gpu-donut` / `SteamInspector` / `market` / `ChatPanel` 等前端 9 处
  useCallback 依赖链 + 未用变量（lint 警告清零）。

#### 前置架构审计（`81xxx` 序列，审查前基础）

- 删死接口 / 去重工具 / 实记清理字节 / 后台禁全盘 walk。
- agent-server 采集收敛（去重 + TTL 缓存 + 温度口径统一）。
- 详见各提交：`808770d` `0ead78a` `c47d278` `21b846a` `9c431b0` `21ba5d7` `6291e5f`。

### 巨型文件拆分 + 前端 bundle 优化（`eb7a0d1`~`64b81b1`）

对全部点名巨型 Rust 文件执行**域拆分**（子模块化，函数按域迁入同名目录），
消除 400–520 行的单文件，按 crate 逐一提交，每提交前 check 0 警告 /
test 全绿 / clippy 0 / fmt 干净：

- **desktop（`apps/desktop/src-tauri/src`）**
  - `scan.rs` 470→422：抽 scaffold 标记域至 `scan/tag`；
  - `hw_report.rs` 440→342：抽风扇建议域（`build_fan_advice`）至 `hw_report/fan`；
  - `hw_test.rs` 413→371：抽温度采样域（TTL 缓存 + `sample_thermal_c`）至 `hw_test/thermal`；
  - `ai.rs` 488→245：抽单测至 `ai_tests`。
- **scanner（`crates/scanner/src`）**：`scan_core.rs` 504→405（`scan_core/build_tree`）、
  `walker.rs` 522→396（`walker/tally`）、`mft.rs` 452→332（`mft_tests`）、
  `usn.rs` 456→245（`usn/win`）、`usn_tree.rs` 436→237（`usn_tree/apply`）、
  `mft_tree.rs` 400→268（`mft_tree/build_node`）。
- **steam-inspector**：`lib.rs` 452→220（库扫描域 → `inspect`）。
- **scaffold**：`winapp2.rs` 490→295（Winapp2.ini 解析域 → `winapp2/parse`）。
- **agent-server（`crates/agent-server/src/tools`）**
  - `sys_boot.rs` 455→295：抽注册表枚举域至 `sys_boot/registry`；
  - `sys_services.rs` 475→394：抽解析辅助域至 `sys_services/parse`；
  - `bench_stress.rs` 408→305：抽 GPU 压测域（温度熔断守门）至 `bench_stress/gpu`。
- **toolbelt（`crates/toolbelt/src`）**：`tree.rs` 456→380（`tree/tree_helpers`）、
  `plugin.rs` 416→286（`plugin/validate` + `plugin/green`）。
- **executor**：`lib.rs` 447→261（回收站域 → `recycle` + 受保护路径判定 → `protect`，
  保持 `pub use` 顶层重导出，外部 crate 调用面不变）。

**前端 bundle 优化（`eb7a0d1`）**：页面级代码分割，主入口 **765KB→428KB（-44%）**，
11 个懒加载页面 chunk 288KB，预加载 vendor-markdown/vendor-react/vendor-tauri；
`tsc -b` 0 / eslint 0 / vitest 224 passed。

**净效果**：30 个拆分提交（`eb7a0d1` 起 15 个，含此前 scanner/toolbelt 系列），
全仓最大 src 文件从 522 行降至 429 行（`scan_core.rs`），无 >450 文件。
workspace `cargo check --workspace --all-targets` 0 警告（唯一可接受项 binrw
v0.11.3 future-incompat）、`cargo test --workspace` 全绿、clippy 0、fmt 干净。

### 工具清单扩容（`9517e2c`）

- toolbelt 工具清单 **30 → 69 条**（内存/外设/硬盘/烤机检测工具）；
- 移除 FPT64（BIOS 刷写，L3 永禁高危）与 USBDeview（可禁用/卸载 USB 设备）——
  缩小默认执行面，与安全模型一致；
- 测试断言同步扩容。

### UI/UX 视觉审查（第 6–8 轮，`e403751` `d4a01bc` `740ba54`）

对全前端做了 3 轮视觉审查（浏览器 1280×800 实渲染 + DOM 像素级取证 +
WCAG 对比度量化 + 视觉子代理交叉验证），修复清单按严重度排序：

- **修复（高）** `button.primary` 等 accent 填充文字用 `var(--ink)` 近黑字，
  在 vercel(#1447e6)/doubao/google/trae 等深色 accent 主题下对比仅 **2.9:1**：
  新增语义 token `--on-accent`（accent 上文字色，按亮度自动选白/深字，
  WCAG ≥4.5），`applyTheme` 预设路径与 `applyAccent` 自定义主色路径均计算；
  primary / `.seg-opt.active` / `.tree-row.selected` / `::selection` /
  `.trace-chip` / `.toolbelt-nav.active` 六类统一改用它。
  实测 vercel 2.9→**6.83**、claude 浅 accent 自动深字 **4.46**。
- **修复（中）** 系统优化页「刷新电源计划」按钮误复用 `system.startup.refresh`
  （显示「刷新启动项」）——新增 `optimizer.power.refresh` 中英 key。
- **修复（中）** `settings-group-title` 组标题 11px+ink-3 比正文还小还淡
  （层级倒挂）——统一 12px+ink-2。
- **修复（中）** 无效 token `--ink-1`（theme.ts 仅 ink/ink-2/ink-3）4 处引用
  全部静默回退失效——permOff 权限提示改 `--risk-caution` 琥珀强调色；
  设置页 select 改 `--ink`。
- **修复（中）** 清理页 21 条脚本 10 条长名被 ellipsis 截断且 hover 无提示
  ——`cp-script` 补 `title`。
- **修复（低）** 工作台无盘布局塌陷（`drives=[]` 时 ws-strip 不渲染，
  `aside.left` 落进 grid auto 行、1fr 行 479px 空白）——三栏+splitter
  显式 `grid-row: 2`，实测 227px→706px 铺满。
- **修复（低）** 设置页主题选择器 17 套主题 3 列分段控件溢出截断长中文名
  ——改自适应 flex 换行块（17 项全显 0 截断）；清理/工具墙空态弱色
  ink-3→ink-2（对比 3.5→10.09）。

**取证排除（非缺陷）**：脚本名单行省略为侧栏列宽物理限制；历史页空态
内容流式正常；字号滑杆数值与轨道精确对齐；「设置」在侧栏底部固定区为
成熟桌面工具标准布局。

---

## 较早版本

### v0.2.3

- AI 实事求是约束 + 守卫只读盘根放行 + 会话续跑。
- 并发会话遗留批量收尾（winapp2 预览 / Studio 重构 / McpServers 重排 /
  只读工具切 check_read / 样式 token）。
- 深色主题 5 主题全修复 + 布局/设置控件统一（见历史提交）。

## 格式说明

- `[未发布]`：当前工作区 HEAD 之后的累积变更（含已提交的 5 轮审查修复）。
- 严重度标注：`高/中/低` 遵循审查报告分级；修复均附回归测试。
