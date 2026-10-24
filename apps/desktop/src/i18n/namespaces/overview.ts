// 资产总览（AssetOverview）与其下的三个视图：目录树（TreeView）、
// 矩形树图（Treemap）、文件列表（FileView）。
//
// 有意留中文、不进文案表的（属设计，不是欠账）：
// - '手动回收'：作为 reason 参数发给后端、并写进回收/undo 记录，属数据值不属文案；
// - FileView 的 NO_EXT = '(none)'：分组哨兵值，同时当 Map key 与过滤条件用，
//   必须语言无关，显示时再换 overview.fileview.extNone。
// spaceWeekDelta() 的「记录不足 / 今日 / N 天」已进表（trend.noRecords/today/daysSpan），
// 它的单测按中文原文断言，故测试文件里 setLang('zh') 钉死语言，避免假失败。
//
// 安全口径（改动前先读 AGENTS.md 铁律 1/6）：
// - 「立即清理」两步确认：firstTitle（第一次）→ armTitle/confirmArmed（第二次），
//   中途可 cancel，去向（进系统回收站）必须写在字面上；
// - 结果三态不许含糊：done（成功）+ someFailed（部分失败，条数可见）、
//   allFailed（全失败，带最后一条错误）、nothingMatched（一条都没匹配到）；
// - 字节数一律带「约 / 预计」标注，不伪装成精确值。
export const overview = {
  ns: 'overview',
  zh: {
    // 磁盘详情卡（含使用率环）
    'overview.gauge.used': '已使用',
    'overview.gauge.unknown': '未读到',
    'overview.detail.title': '磁盘详情',
    'overview.detail.name': '{drive} 盘',
    'overview.detail.scanned': '{path} · 容量为系统实时 · 明细已扫描',
    'overview.detail.unscanned': '{path} · 容量为系统实时 · 明细未扫描',
    'overview.detail.total': '总容量',
    'overview.detail.used': '已用空间',
    'overview.detail.free': '可用空间',
    'overview.detail.mapped': '映射盘（空间实际在）',
    'overview.noDrive': '未检测到磁盘',

    // 磁盘空间趋势（R6）
    'overview.trend.title': '磁盘空间趋势',
    'overview.trend.empty': '扫描后自动记录空间变化，这里会显示多日曲线',
    'overview.trend.loadFailed': '没能读取空间趋势历史（不代表还没扫过）：{msg}',
    'overview.trend.note': '每次扫描自动记录 · 点曲线看工作台',
    'overview.trend.driveUnit': '盘',
    'overview.trend.noRecords': '记录不足',
    'overview.trend.today': '今日',
    'overview.trend.daysSpan': '{n} 天',
    'overview.trend.samplesRich': '{n} 个采样点',
    'overview.trend.samplesSparse': '仅 {n} 个采样点，趋势仅供参考',
    // R90 时间窗口：7 天 / 30 天 / 全部（历史里有数据但窗口过滤后为空时也复用 rangeAll）
    'overview.trend.range7': '7 天',
    'overview.trend.range30': '30 天',
    'overview.trend.rangeAll': '全部',
    'overview.trend.rangeEmpty': '这段时间还没有扫描记录——换个时间范围看看',
    'overview.locateInWorkspace': '在工作台定位查看',
    // R98 空间耗尽预测：按趋势增速外推「多少天后用满」（0=已满；增速≤0 不显示）
    'overview.trend.fullIn': '约 {n} 天满',
    'overview.trend.fullInRough': '样本少，粗估 {n} 天满',
    'overview.trend.fullNow': '已满',

    // 快速操作
    'overview.quick.title': '快速操作',
    'overview.quick.ai': '一键智能清理',
    'overview.quick.aiTip': '打开工作台，让 AI 生成清理清单',
    'overview.quick.big': '大文件扫描',
    'overview.quick.bigTip': '在工作台按大小定位大文件',
    'overview.quick.dup': '重复文件查找',
    'overview.quick.dupTip': '扫描选中盘 ≥1MB 的重复文件，出清单后经确认窗回收副本',
    'overview.quick.junk': '系统垃圾清理',
    'overview.quick.junkTip': '打开清理页：按脚本勾选要清的目录再预览',

    // 文件分类占用（按选中盘聚合，柱条可回收潜力与建议清理同源）
    'overview.cat.title': '文件分类占用',
    'overview.cat.noHit': '未命中已知分类，可扫其他盘',
    'overview.cat.needScan': '扫描后显示文件分类占用',
    'overview.cat.collapse': '收起分类',
    'overview.cat.expand': '展开其余 {n} 个分类',
    'overview.cat.reclaim': '可回收 {size}',
    'overview.cat.reclaimTip': '该分类在这些目录里可清理的预计空间（进系统回收站）',
    'overview.cat.reclaimNote': '{size} 可回收',
    // R219：分类行有可回收潜力时点击直达清理页预选该脚本
    'overview.cat.gotoCleanup': '在清理页预选「{name}」脚本',

    // 建议清理项目（两步确认 + 结果三态，措辞不许软化）
    'overview.clean.title': '建议清理项目',
    'overview.clean.openPage': '打开清理页',
    // R217：逐条建议项「去清理」——直达清理页并预选该脚本
    'overview.clean.goto': '去清理',
    'overview.clean.gotoTip': '在清理页预选「{label}」脚本',
    'overview.clean.checking': '正在检查可清理项…',
    'overview.clean.checkingList': '正在检查可清理项目…',
    'overview.clean.estimated': '预计可释放 {size}',
    'overview.clean.noneOnDrive': '当前磁盘无可清理项',
    'overview.clean.needScan': '扫描后自动计算可回收空间（不扫描不占磁盘 IO）',
    'overview.clean.failed': '没能读取可清理项',
    'overview.clean.loadFailed': '没能读取可清理项（不代表没有；可能是后端未就绪或统计失败）：{msg}',
    'overview.clean.spotless': '{drive} 盘很干净，暂无可清理项目',
    // health-butler P0：建议清理跨次对比。措辞纪律——没有上次快照就说"首次"，
    // 读失败就说"未读到"，绝不用"持平"假装有比较过。
    'overview.clean.diffMore': '比上次多出 {size}',
    'overview.clean.diffLess': '比上次少了 {size}',
    'overview.clean.diffFlat': '与上次持平',
    'overview.clean.diffGap': '距上次 {n} 天',
    'overview.clean.diffGapOn': '距上次 {n} 天（上次 {date}）',
    'overview.clean.diffTop': '主要在 {label}（{prev} → {now}）',
    'overview.clean.diffNew': '新增 {n} 类',
    'overview.clean.diffGone': '减少 {n} 类',
    'overview.clean.diffFirst': '首次统计，下次扫描后可对比变化',
    'overview.clean.diffUnread': '没能读取上次统计（不代表没有记录）：{msg}',
    // 拆前缀/后缀是为了保住中间的 <b> 加粗数字，英文靠两个片段间的空格拼成整句。
    'overview.clean.selPrefix': '已选',
    'overview.clean.selSuffix': '项，共',
    'overview.clean.cancel': '取消',
    'overview.clean.armTitle': '5 秒内再点一次确认清理（进系统回收站）',
    'overview.clean.firstTitle': '先点一次确认，再点一次才真正清理（默认进回收站）',
    'overview.clean.running': '清理中…',
    'overview.clean.confirmArmed': '确认清理 {n} 项（进回收站）',
    'overview.clean.now': '立即清理',
    // R240：总览可清理卡底部「全选 / 清空」一键切换——逐项勾选或取消后
    // 不必逐个点，一个按钮在全选与清空间切换（真实后端的批量勾选习惯）。
    'overview.clean.selectAll': '全选',
    'overview.clean.clearAll': '清空',
    'overview.clean.done': '已清理 {n} 项 · 释放约 {size}（已进回收站）',
    'overview.clean.someFailed': ' · {n} 项失败',
    'overview.clean.allFailed': '{n} 项清理失败：{err}（详见控制台）',
    'overview.clean.nothingMatched': '本次未匹配到可清理文件（可能刚清理过，或应用正在占用）',
    'overview.clean.wechatAll': '已勾选微信缓存：从总览一键清理会覆盖本机所有微信账号，如需仅清指定账号请到清理页勾选',
    // R102 建议项展开逐文件预览（dry-run，与执行同口径）
    'overview.clean.peekShow': '展开将删除的文件',
    'overview.clean.peekHide': '收起文件清单',
    'overview.clean.peekLoading': '正在读取文件清单…',
    'overview.clean.peekFailed': '没能读取文件清单（不代表没有；可能是后端未就绪）：{msg}',
    'overview.clean.peekEmpty': '该分类当前没有匹配到可清理文件（可能刚清理过）',
    'overview.clean.peekFiles': '{n} 个文件',
    'overview.clean.peekHint': '仅展示；与执行使用相同的保留天数口径',
    'overview.clean.peekLastClean': '上次清理 {age}',
    'overview.clean.peekMore': '…另有 {n} 条',
    'overview.clean.wechatRedirect': '微信缓存涉及多账号，请到清理页勾选要清的账号后再清理，避免误清其他账号',

    // 红盘救援横幅（使用率 >=85%）
    'overview.rescue.title': '{drive} 盘已用 {pct}%，快满了',
    'overview.rescue.scannedHas': '下面已列出约 {size} 可安全清理项，勾选后默认进系统回收站',
    'overview.rescue.scannedNone': '已扫描当前盘：先清大文件，或看看下面有没有可清理项',
    'overview.rescue.notScanned': '先扫描一次，DiskPilot 会直接列出可安全清理项（缓存 / 临时文件等，默认进回收站）',
    'overview.rescue.scanning': '扫描中…',
    'overview.rescue.scanNow': '立即扫描',
    'overview.rescue.viewClean': '查看可清理项',

    // 今日体检条（health-butler P0）：全只读汇总，每项可点进对应流程
    'overview.checkup.title': '今日体检',
    'overview.checkup.subtitle': '只读汇总 · 点各项进对应处理',
    'overview.checkup.redDrive': '{n} 块盘已用 ≥85%',
    'overview.checkup.redDriveUnread': '{n} 块盘容量未读到',
    'overview.checkup.redDriveNone': '无红盘',
    'overview.checkup.reclaimable': '可清理 {size}',
    'overview.checkup.reclaimableNone': '暂无可回收项',
    'overview.checkup.reclaimUnread': '可回收项未读到',
    'overview.checkup.temp': '最高温 {temp}℃',
    'overview.checkup.tempNone': '未读到温度',
    'overview.checkup.tempHigh': '温度偏高（≥{warn}℃）',
    'overview.checkup.startup': '启动项 {n} 个',
    'overview.checkup.startupDisabled': '已禁用 {n}',
    'overview.checkup.startupNone': '未检测',
    'overview.checkup.startupUnread': '启动项未读到（不代表没有）',
    'overview.checkup.goStartup': '去系统优化看启动项明细',
    'overview.checkup.goRescue': '查看红盘',
    'overview.checkup.goRescanCapacity': '重新扫描读取容量',
    'overview.checkup.goStaleDrive': '选中这块盘看详情（要扫它点盘卡片）',
    // R308 温度格落点：最高温是结论，明细（采样点/区间峰值/最新采样时间）在温度采样卡
    'overview.checkup.goThermal': '滚到温度采样历史看明细',
    'overview.checkup.goClean': '去清理',
    'overview.checkup.diskAllGood': '{n} 块硬盘健康',
    'overview.checkup.diskRisky': '{n} 块硬盘 SMART 告警',
    'overview.checkup.diskUnknown': '{n} 块硬盘健康未上报',
    'overview.checkup.diskPartial': '{good} 块健康 · {unknown} 块未上报',
    'overview.checkup.diskNone': '硬盘健康未读到',
    'overview.checkup.goHardware': '查看硬盘健康',
    // R293 扫描时效：三态措辞必须分开（never ≠ 很久没扫 ≠ 未读到）
    'overview.checkup.scanAgeNever': '{n} 块盘还没扫描过',
    'overview.checkup.scanAgeStale': '{n} 块盘超过 {days} 天没扫了',
    'overview.checkup.scanAgeStaleDays': '最久 {days} 天没扫',
    // R303：后端返回 last_at（「上次扫于 X」），此前只留天数把日期丢了
    'overview.checkup.scanAgeStaleAt': '{days} 天没扫（上次 {date}）',
    'overview.checkup.scanAgeStaleAtShort': '最久 {days} 天没扫（{date}）',
    'overview.checkup.scanAgeSkew': '扫描时间戳晚于当前时刻（时钟异常），天数不可信',
    'overview.checkup.scanAgeSkewShort': '扫描时间异常',
    'overview.checkup.scanAgeUnread': '{n} 块盘的扫描记录未读到（不代表没扫过）',
    'overview.checkup.scanAgeUnreadAll': '扫描记录未读到',
    'overview.checkup.scanAgeOk': '扫描记录较新',
    // R299 温度采样历史：开关开了多久、采到几个点、区间峰值分别是三件事
    'overview.thermal.title': '温度采样',
    'overview.thermal.note': '只读历史 · 5 分钟一点',
    'overview.thermal.empty': '还没有采样点（开关没开，或刚开还没到一个周期）',
    'overview.thermal.unread': '温度历史未读到（不代表没采样）',
    'overview.thermal.single': '仅 1 点 · 当前 {temp}℃',
    'overview.thermal.range': '近 {n} 点 · {span} 分钟',
    'overview.thermal.latestAt': '最新采样 {when}',
    'overview.thermal.latestOn': '最新采样于 {date}',
    'overview.thermal.ageMin': '{n} 分钟前',
    'overview.thermal.ageHour': '{n} 小时前',
    'overview.thermal.ageDay': '{n} 天前',
    'overview.thermal.peak': '区间峰值 {temp}℃',
    'overview.thermal.peakHigh': '区间峰值 {temp}℃（曾到阈值）',
    'overview.thermal.hint': '采样开关在「设置 → 提醒 → 温度持续偏高提醒」。这里只展示已采到的点，不写入、不改采样频率。',

    // 重复文件查找 → 清理提案（提案四件套：是什么 / 干什么用 / 删了影响）
    'overview.dup.noneFound': '未发现 ≥1MB 的重复文件（按大小 + 头部哈希）',
    'overview.dup.allRunning': '重复组都只有运行中的文件（跳过），无可回收项',
    'overview.dup.skippedRunning': '另有 {n} 个重复文件正在使用（已跳过，不列入这份清单）',
    'overview.dup.scanFailed': '重复文件扫描失败（路径被守卫拒绝或未扫描该盘）',
    'overview.dup.title': '重复文件清理 · {path}（约 {size} 可回收）',
    'overview.dup.reason': '重复文件（组内 {n} 份 · {size}）',
    'overview.dup.what': '与其他文件内容相同（头部 64KB 哈希一致），属于冗余副本',
    'overview.dup.purpose': '组内保留了原件一份（{keep}）',
    'overview.dup.impact': '删除后组内仍保留一份，不丢数据；仅头部哈希判定，不排除极小概率误判',

    // 空间大头 Top 10 与底部统计
    'overview.top.title': '空间大头 Top 10',
    'overview.top.note': '点击目录定位查看',
    'overview.top.files': '{count} 文件',
    'overview.foot.used': '本机已用 {size}',
    'overview.foot.usedUnread': '{n} 块盘容量未计入',
    'overview.foot.scanned': '已扫 {n} / {total} 个盘',
    'overview.foot.rescanTitle': '重新扫描当前磁盘（不用缓存）',
    'overview.foot.rescan': '重新扫描',
    'overview.foot.rescanDrive': ' {drive} 盘',

    // 目录树
    'overview.tree.colName': '文件夹',
    'overview.tree.colParentPct': '父级 %',
    'overview.tree.colSize': '大小',
    'overview.tree.colCount': '项目',
    'overview.tree.truncatedTip': '共 {n} 项 · 展开后加载完整子项',
    'overview.tree.loading': '加载中…',

    // 矩形树图悬浮提示
    'overview.treemap.tip': '{path}\n{size} · {share}% of 父级',
    // R312：空间图只画根的前 300 个子项，折叠目录也没展开——说出来并指向树视图
    'overview.treemap.folded': '只画前 300 项、折叠目录未展开 · 完整清单见树视图',

    // 文件列表
    'overview.fileview.search': '搜索文件… 支持 *  ?  通配符',
    'overview.fileview.count': '{count} 个文件',
    'overview.fileview.truncated': '（过多，已截断）',
    'overview.fileview.showTitle': '显示条数',
    'overview.fileview.showN': '显示 {n} 条',
    'overview.fileview.colName': '文件名',
    'overview.fileview.colSize': '大小',
    'overview.fileview.empty': '没有匹配的文件',
    'overview.fileview.menuReveal': '在文件管理器中打开',
    'overview.fileview.menuCopy': '复制路径',
    'overview.fileview.menuRecycle': '移入回收站',
    'overview.fileview.recycleConfirm': '确定把「{name}」移入回收站吗？\n释放约 {size}，可从系统回收站找回。',
    'overview.fileview.extTitle': '扩展名占用 TOP {n}',
    'overview.fileview.clickFilter': '点击过滤 {ext}',
    'overview.fileview.extNone': '(无)',

    // TreeView / FileView 行悬浮提示共用
    'overview.rightClickHint': '{path}  ·  右键查看选项',
  },
  en: {
    // Drive detail card (with the usage ring)
    'overview.gauge.used': 'Used',
    'overview.gauge.unknown': 'Not read',
    'overview.detail.title': 'Drive details',
    'overview.detail.name': '{drive} drive',
    'overview.detail.scanned': '{path} · capacity is live from Windows · contents scanned',
    'overview.detail.unscanned': '{path} · capacity is live from Windows · contents not scanned',
    'overview.detail.total': 'Capacity',
    'overview.detail.used': 'Used',
    'overview.detail.free': 'Free',
    'overview.detail.mapped': 'Mapped drive (space lives on)',
    'overview.noDrive': 'No drives detected',

    // Space trend (R6)
    'overview.trend.title': 'Space over time',
    'overview.trend.empty': 'Every scan records a space snapshot — multi-day curves show up here',
    'overview.trend.loadFailed': 'Could not read the space-trend history (this does not mean no scan happened yet): {msg}',
    'overview.trend.note': 'Recorded on each scan · click a row to open the workbench',
    'overview.trend.driveUnit': 'drive',
    'overview.trend.noRecords': 'Not enough history',
    'overview.trend.today': 'Today',
    'overview.trend.daysSpan': '{n} days',
    'overview.trend.samplesRich': '{n} sample points',
    'overview.trend.samplesSparse': 'only {n} sample points — trend is a rough hint',
    // R90 time range: 7 days / 30 days / all (rangeAll is also the exit when a window filters everything out)
    'overview.trend.range7': '7 days',
    'overview.trend.range30': '30 days',
    'overview.trend.rangeAll': 'All',
    'overview.trend.rangeEmpty': 'No scans in this period — try another range',
    'overview.locateInWorkspace': 'Show this in the workbench',
    // R98 days-to-full forecast: extrapolated from the trend slope (0 = already full; hidden when not growing)
    'overview.trend.fullIn': '≈ full in {n} days',
    'overview.trend.fullInRough': 'few samples — rough: full in ≈ {n} days',
    'overview.trend.fullNow': 'Full',

    // Quick actions
    'overview.quick.title': 'Quick actions',
    'overview.quick.ai': 'Smart cleanup',
    'overview.quick.aiTip': 'Open the workbench and let the AI build a cleanup list',
    'overview.quick.big': 'Find large files',
    'overview.quick.bigTip': 'Locate big files by size in the workbench',
    'overview.quick.dup': 'Find duplicates',
    'overview.quick.dupTip': 'Scan the selected drive for ≥1MB duplicates, then recycle the copies through the confirmation dialog',
    'overview.quick.junk': 'System junk cleanup',
    'overview.quick.junkTip': 'Open the cleanup page: tick scopes per script, then preview',

    // Space by category (aggregated over the selected drive; the reclaim tag shares
// the same source as Suggested cleanup)
    'overview.cat.title': 'Space by file category',
    'overview.cat.noHit': 'No known categories here — try another drive',
    'overview.cat.needScan': 'Scan a drive to see space by category',
    'overview.cat.collapse': 'Collapse categories',
    'overview.cat.expand': 'Show {n} more categories',
    'overview.cat.reclaim': '{size} reclaimable',
    'overview.cat.reclaimTip': 'Estimated space cleanable from this category via these folders (goes to the Recycle Bin)',
    'overview.cat.reclaimNote': '{size} reclaimable',
    // R219: a row with reclaimable potential opens the cleanup page preselecting this script
    'overview.cat.gotoCleanup': 'Preselect “{name}” on the cleanup page',

    // Suggested cleanup (two-step confirm + three-state result; wording must stay explicit)
    'overview.clean.title': 'Suggested cleanup',
    'overview.clean.openPage': 'Open cleanup page',
    // R217: per-item jump to cleanup page, preselecting this script
    'overview.clean.goto': 'Clean up',
    'overview.clean.gotoTip': 'Preselect “{label}” on the cleanup page',
    'overview.clean.checking': 'Checking what can be cleaned…',
    'overview.clean.checkingList': 'Checking cleanup items…',
    'overview.clean.estimated': 'About {size} to free',
    'overview.clean.noneOnDrive': 'Nothing to clean on this drive',
    'overview.clean.needScan': 'Space reclaimable is computed after a scan (no scan, no disk IO)',
    'overview.clean.failed': 'Could not read cleanup items',
    'overview.clean.loadFailed': 'Could not read cleanup items (this does not mean there are none; backend may be unavailable or stats failed): {msg}',
    'overview.clean.spotless': 'The {drive} drive looks clean — nothing to remove',
    // health-butler P0: cross-session cleanup delta. No fake "unchanged" when
    // there is no previous snapshot to compare against.
    'overview.clean.diffMore': '{size} more than last time',
    'overview.clean.diffLess': '{size} less than last time',
    'overview.clean.diffFlat': 'Same as last time',
    'overview.clean.diffGap': '{n} days since last check',
    'overview.clean.diffGapOn': '{n} days since last check (on {date})',
    'overview.clean.diffTop': 'Mostly {label} ({prev} → {now})',
    'overview.clean.diffNew': '{n} new',
    'overview.clean.diffGone': '{n} cleared',
    'overview.clean.diffFirst': 'First snapshot — changes can be compared after the next scan',
    'overview.clean.diffUnread': 'Could not read the previous snapshot (this does not mean there is none): {msg}',
    // Split around the bold numbers; the space between the two fragments joins the sentence.
    'overview.clean.selPrefix': 'Selected',
    'overview.clean.selSuffix': 'items, freeing about',
    'overview.clean.cancel': 'Cancel',
    'overview.clean.armTitle': 'Click once more within 5 seconds to clean (goes to the system Recycle Bin)',
    'overview.clean.firstTitle': 'Click once to confirm, once more to actually clean (Recycle Bin by default)',
    'overview.clean.running': 'Cleaning…',
    'overview.clean.confirmArmed': 'Clean {n} items (to Recycle Bin)',
    'overview.clean.now': 'Clean now',
    // R240：overview cleanup card bottom "Select all / Clear" toggle.
    'overview.clean.selectAll': 'Select all',
    'overview.clean.clearAll': 'Clear',
    'overview.clean.done': 'Cleaned {n} items · about {size} freed (moved to Recycle Bin)',
    'overview.clean.someFailed': ' · {n} failed',
    'overview.clean.allFailed': 'Cleanup failed for {n} items: {err} (see console)',
    'overview.clean.nothingMatched': 'No matching files this time (maybe already cleaned, or the app is holding them)',
    'overview.clean.wechatAll': 'WeChat cache is checked: one-click clean from the overview covers every WeChat account on this machine. To clean only specific accounts, use the cleanup page',
    // R102 per-item expandable file preview (dry-run, same retention as execution)
    'overview.clean.peekShow': 'Show files that would be removed',
    'overview.clean.peekHide': 'Hide file list',
    'overview.clean.peekLoading': 'Reading file list…',
    'overview.clean.peekFailed': 'Could not read the file list (this does not mean there are none; backend may be unavailable): {msg}',
    'overview.clean.peekEmpty': 'No matching files to clean in this category right now (may have been cleaned recently)',
    'overview.clean.peekFiles': '{n} file(s)',
    'overview.clean.peekHint': 'Preview only; uses the same retention days as execution',
    'overview.clean.peekLastClean': 'Last cleaned {age}',
    'overview.clean.peekMore': '…{n} more',
    'overview.clean.wechatRedirect': 'WeChat cache spans multiple accounts — pick the accounts on the cleanup page before cleaning, so other accounts are not touched accidentally',

    // Low-space banner (usage >= 85%)
    'overview.rescue.title': 'The {drive} drive is {pct}% full',
    'overview.rescue.scannedHas': 'About {size} is safe to clean below — ticked items go to the system Recycle Bin',
    'overview.rescue.scannedNone': 'This drive is scanned: start with large files, or check the cleanup list below',
    'overview.rescue.notScanned': 'Scan it once and DiskPilot lists what is safe to clean (caches / temp files, Recycle Bin by default)',
    'overview.rescue.scanning': 'Scanning…',
    'overview.rescue.scanNow': 'Scan now',
    'overview.rescue.viewClean': 'See cleanup items',

    // Today's health checkup banner (health-butler P0): read-only digest, each item is a shortcut
    'overview.checkup.title': 'Today checkup',
    'overview.checkup.subtitle': 'Read-only digest — click an item to open its flow',
    'overview.checkup.redDrive': '{n} drives ≥85% full',
    'overview.checkup.redDriveUnread': '{n} drives with unread capacity',
    'overview.checkup.redDriveNone': 'No red drives',
    'overview.checkup.reclaimable': '{size} reclaimable',
    'overview.checkup.reclaimableNone': 'Nothing to reclaim',
    'overview.checkup.reclaimUnread': 'Reclaimable not read',
    'overview.checkup.temp': 'Peak temp {temp}℃',
    'overview.checkup.tempNone': 'No temperature read',
    'overview.checkup.tempHigh': 'Temp high (≥{warn}℃)',
    'overview.checkup.startup': '{n} startup items',
    'overview.checkup.startupDisabled': '{n} disabled',
    'overview.checkup.startupNone': 'Not detected',
    'overview.checkup.startupUnread': 'Startup items not read (that does not mean there are none)',
    'overview.checkup.goStartup': 'Manage startup items in System Optimizer',
    'overview.checkup.goRescue': 'See drives',
    'overview.checkup.goRescanCapacity': 'Rescan to read capacity',
    'overview.checkup.goStaleDrive': 'Select this drive to inspect (click its card to scan)',
    // R308 temperature cell target: the peak is the verdict, the details live in the thermal card
    'overview.checkup.goThermal': 'Jump to temperature sampling history',
    'overview.checkup.goClean': 'Clean up',
    'overview.checkup.diskAllGood': '{n} drives healthy',
    'overview.checkup.diskRisky': '{n} drives with SMART warnings',
    'overview.checkup.diskUnknown': '{n} drives with no health reported',
    'overview.checkup.diskPartial': '{good} healthy · {unknown} not reported',
    'overview.checkup.diskNone': 'Drive health not read',
    'overview.checkup.goHardware': 'See drive health',
    // R293 scan staleness: three states must read differently (never ≠ stale ≠ unread)
    'overview.checkup.scanAgeNever': '{n} drive(s) never scanned',
    'overview.checkup.scanAgeStale': '{n} drive(s) not scanned for over {days} day(s)',
    'overview.checkup.scanAgeStaleDays': 'last scan {days} day(s) ago',
    // R303: the backend returns last_at (last scanned at); only the day count was kept before
    'overview.checkup.scanAgeStaleAt': 'not scanned for {days} day(s) (last {date})',
    'overview.checkup.scanAgeStaleAtShort': 'last scan {days} day(s) ago ({date})',
    'overview.checkup.scanAgeSkew': 'Scan timestamps are ahead of the clock (time skew); day counts are unreliable',
    'overview.checkup.scanAgeSkewShort': 'Scan time looks wrong',
    'overview.checkup.scanAgeUnread': '{n} drive(s) with unread scan history (that does not mean never scanned)',
    'overview.checkup.scanAgeUnreadAll': 'Scan history not read',
    'overview.checkup.scanAgeOk': 'Scan history is recent',
    // R299 thermal sampling history: how long it has run, how many points, and the peak are three different facts
    'overview.thermal.title': 'Thermal samples',
    'overview.thermal.note': 'Read-only history · one point per 5 min',
    'overview.thermal.empty': 'No samples yet (the switch is off, or it was just turned on)',
    'overview.thermal.unread': 'Thermal history not read (that does not mean nothing was sampled)',
    'overview.thermal.single': '1 point · now {temp}℃',
    'overview.thermal.range': '{n} points · {span} min',
    'overview.thermal.latestAt': 'Latest sample {when}',
    'overview.thermal.latestOn': 'Latest sample on {date}',
    'overview.thermal.ageMin': '{n} min ago',
    'overview.thermal.ageHour': '{n} h ago',
    'overview.thermal.ageDay': '{n} d ago',
    'overview.thermal.peak': 'Peak {temp}℃',
    'overview.thermal.peakHigh': 'Peak {temp}℃ (reached the threshold)',
    'overview.thermal.hint': 'The sampling switch lives in Settings, Reminders, Sustained high temperature. This view only shows points already sampled; it never writes or changes the sampling rate.',

    // Duplicate scan → cleanup proposal (what / purpose / impact)
    'overview.dup.noneFound': 'No duplicates ≥1MB found (by size + header hash)',
    'overview.dup.allRunning': 'Every duplicate group only holds in-use files (skipped) — nothing to recycle',
    'overview.dup.skippedRunning': 'Plus {n} duplicate(s) currently in use — skipped, not listed here',
    'overview.dup.scanFailed': 'Duplicate scan failed (path rejected by the guard, or the drive was never scanned)',
    'overview.dup.title': 'Duplicate cleanup · {path} (about {size} reclaimable)',
    'overview.dup.reason': 'Duplicate file ({n} copies in the group · {size})',
    'overview.dup.what': 'Identical to other files (same 64KB header hash) — a redundant copy',
    'overview.dup.purpose': 'The group keeps one original ({keep})',
    'overview.dup.impact': 'One copy stays in the group after deletion, so no data is lost; judged by header hash only, so a tiny misjudgement risk remains',

    // Top 10 space hogs and the footer stats
    'overview.top.title': 'Top 10 space hogs',
    'overview.top.note': 'Click a folder to locate it',
    'overview.top.files': '{count} files',
    'overview.foot.used': 'This PC using {size}',
    'overview.foot.usedUnread': '{n} drives not counted',
    'overview.foot.scanned': 'Scanned {n} / {total} drives',
    'overview.foot.rescanTitle': 'Rescan the current drive (ignore the cache)',
    'overview.foot.rescan': 'Rescan',
    'overview.foot.rescanDrive': ' {drive} drive',

    // Tree view
    'overview.tree.colName': 'Folder',
    'overview.tree.colParentPct': '% of parent',
    'overview.tree.colSize': 'Size',
    'overview.tree.colCount': 'Items',
    'overview.tree.truncatedTip': '{n} items in total · expand to load all children',
    'overview.tree.loading': 'Loading…',

    // Treemap hover tooltip
    'overview.treemap.tip': '{path}\n{size} · {share}% of parent',
    // R312: the map lays out only the first 300 top-level items and folded folders stay collapsed
    'overview.treemap.folded': 'Top 300 items only, folded folders collapsed · see Tree view for all',

    // File list
    'overview.fileview.search': 'Search files… * and ? wildcards work',
    'overview.fileview.count': '{count} files',
    'overview.fileview.truncated': ' (too many, truncated)',
    'overview.fileview.showTitle': 'Rows to show',
    'overview.fileview.showN': 'Show {n}',
    'overview.fileview.colName': 'Name',
    'overview.fileview.colSize': 'Size',
    'overview.fileview.empty': 'No matching files',
    'overview.fileview.menuReveal': 'Open in File Explorer',
    'overview.fileview.menuCopy': 'Copy path',
    'overview.fileview.menuRecycle': 'Move to Recycle Bin',
    'overview.fileview.recycleConfirm': 'Move “{name}” to the Recycle Bin?\nFrees about {size}, and you can restore it from the Bin.',
    'overview.fileview.extTitle': 'Space by extension · TOP {n}',
    'overview.fileview.clickFilter': 'Click to filter by {ext}',
    'overview.fileview.extNone': '(none)',

    // Shared row tooltip (TreeView / FileView)
    'overview.rightClickHint': '{path}  ·  right-click for options',
  },
};
