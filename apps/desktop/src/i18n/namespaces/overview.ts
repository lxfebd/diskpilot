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
    'overview.detail.title': '磁盘详情',
    'overview.detail.name': '{drive} 盘',
    'overview.detail.scanned': '{path} · 容量为系统实时 · 明细已扫描',
    'overview.detail.unscanned': '{path} · 容量为系统实时 · 明细未扫描',
    'overview.detail.total': '总容量',
    'overview.detail.used': '已用空间',
    'overview.detail.free': '可用空间',
    'overview.noDrive': '未检测到磁盘',

    // 磁盘空间趋势（R6）
    'overview.trend.title': '磁盘空间趋势',
    'overview.trend.empty': '扫描后自动记录空间变化，这里会显示多日曲线',
    'overview.trend.note': '每次扫描自动记录 · 点曲线看工作台',
    'overview.trend.driveUnit': '盘',
    'overview.trend.noRecords': '记录不足',
    'overview.trend.today': '今日',
    'overview.trend.daysSpan': '{n} 天',
    'overview.locateInWorkspace': '在工作台定位查看',

    // 快速操作
    'overview.quick.title': '快速操作',
    'overview.quick.ai': '一键智能清理',
    'overview.quick.aiTip': '打开工作台，让 AI 生成清理清单',
    'overview.quick.big': '大文件扫描',
    'overview.quick.bigTip': '在工作台按大小定位大文件',
    'overview.quick.dup': '重复文件查找',
    'overview.quick.dupTip': '扫描选中盘 ≥1MB 的重复文件，出清单后经确认窗回收副本',
    'overview.quick.junk': '系统垃圾清理',
    'overview.quick.junkTip': '打开工作台的分类清理',

    // 文件分类占用
    'overview.cat.title': '文件分类占用',
    'overview.cat.noHit': '未命中已知分类，可扫其他盘',
    'overview.cat.needScan': '扫描后显示文件分类占用',
    'overview.cat.collapse': '收起分类',
    'overview.cat.expand': '展开其余 {n} 个分类',

    // 建议清理项目（两步确认 + 结果三态，措辞不许软化）
    'overview.clean.title': '建议清理项目',
    'overview.clean.checking': '正在检查可清理项…',
    'overview.clean.checkingList': '正在检查可清理项目…',
    'overview.clean.estimated': '预计可释放 {size}',
    'overview.clean.noneOnDrive': '当前磁盘无可清理项',
    'overview.clean.needScan': '扫描后自动计算可回收空间（不扫描不占磁盘 IO）',
    'overview.clean.spotless': '{drive} 盘很干净，暂无可清理项目',
    // 拆前缀/后缀是为了保住中间的 <b> 加粗数字，英文靠两个片段间的空格拼成整句。
    'overview.clean.selPrefix': '已选',
    'overview.clean.selSuffix': '项，共',
    'overview.clean.cancel': '取消',
    'overview.clean.armTitle': '5 秒内再点一次确认清理（进系统回收站）',
    'overview.clean.firstTitle': '先点一次确认，再点一次才真正清理（默认进回收站）',
    'overview.clean.running': '清理中…',
    'overview.clean.confirmArmed': '确认清理 {n} 项（进回收站）',
    'overview.clean.now': '立即清理',
    'overview.clean.done': '已清理 {n} 项 · 释放约 {size}（已进回收站）',
    'overview.clean.someFailed': ' · {n} 项失败',
    'overview.clean.allFailed': '{n} 项清理失败：{err}（详见控制台）',
    'overview.clean.nothingMatched': '本次未匹配到可清理文件（可能刚清理过，或应用正在占用）',

    // 红盘救援横幅（使用率 >=85%）
    'overview.rescue.title': '{drive} 盘已用 {pct}%，快满了',
    'overview.rescue.scannedHas': '下面已列出约 {size} 可安全清理项，勾选后默认进系统回收站',
    'overview.rescue.scannedNone': '已扫描当前盘：先清大文件，或看看下面有没有可清理项',
    'overview.rescue.notScanned': '先扫描一次，DiskPilot 会直接列出可安全清理项（缓存 / 临时文件等，默认进回收站）',
    'overview.rescue.scanning': '扫描中…',
    'overview.rescue.scanNow': '立即扫描',
    'overview.rescue.viewClean': '查看可清理项',

    // 重复文件查找 → 清理提案（提案四件套：是什么 / 干什么用 / 删了影响）
    'overview.dup.noneFound': '未发现 ≥1MB 的重复文件（按大小 + 头部哈希）',
    'overview.dup.allRunning': '重复组都只有运行中的文件（跳过），无可回收项',
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
    'overview.detail.title': 'Drive details',
    'overview.detail.name': '{drive} drive',
    'overview.detail.scanned': '{path} · capacity is live from Windows · contents scanned',
    'overview.detail.unscanned': '{path} · capacity is live from Windows · contents not scanned',
    'overview.detail.total': 'Capacity',
    'overview.detail.used': 'Used',
    'overview.detail.free': 'Free',
    'overview.noDrive': 'No drives detected',

    // Space trend (R6)
    'overview.trend.title': 'Space over time',
    'overview.trend.empty': 'Every scan records a space snapshot — multi-day curves show up here',
    'overview.trend.note': 'Recorded on each scan · click a row to open the workbench',
    'overview.trend.driveUnit': 'drive',
    'overview.trend.noRecords': 'Not enough history',
    'overview.trend.today': 'Today',
    'overview.trend.daysSpan': '{n} days',
    'overview.locateInWorkspace': 'Show this in the workbench',

    // Quick actions
    'overview.quick.title': 'Quick actions',
    'overview.quick.ai': 'Smart cleanup',
    'overview.quick.aiTip': 'Open the workbench and let the AI build a cleanup list',
    'overview.quick.big': 'Find large files',
    'overview.quick.bigTip': 'Locate big files by size in the workbench',
    'overview.quick.dup': 'Find duplicates',
    'overview.quick.dupTip': 'Scan the selected drive for ≥1MB duplicates, then recycle the copies through the confirmation dialog',
    'overview.quick.junk': 'System junk cleanup',
    'overview.quick.junkTip': 'Open category cleanup in the workbench',

    // Space by category
    'overview.cat.title': 'Space by file category',
    'overview.cat.noHit': 'No known categories here — try another drive',
    'overview.cat.needScan': 'Scan a drive to see space by category',
    'overview.cat.collapse': 'Collapse categories',
    'overview.cat.expand': 'Show {n} more categories',

    // Suggested cleanup (two-step confirm + three-state result; wording must stay explicit)
    'overview.clean.title': 'Suggested cleanup',
    'overview.clean.checking': 'Checking what can be cleaned…',
    'overview.clean.checkingList': 'Checking cleanup items…',
    'overview.clean.estimated': 'About {size} to free',
    'overview.clean.noneOnDrive': 'Nothing to clean on this drive',
    'overview.clean.needScan': 'Space reclaimable is computed after a scan (no scan, no disk IO)',
    'overview.clean.spotless': 'The {drive} drive looks clean — nothing to remove',
    // Split around the bold numbers; the space between the two fragments joins the sentence.
    'overview.clean.selPrefix': 'Selected',
    'overview.clean.selSuffix': 'items, freeing about',
    'overview.clean.cancel': 'Cancel',
    'overview.clean.armTitle': 'Click once more within 5 seconds to clean (goes to the system Recycle Bin)',
    'overview.clean.firstTitle': 'Click once to confirm, once more to actually clean (Recycle Bin by default)',
    'overview.clean.running': 'Cleaning…',
    'overview.clean.confirmArmed': 'Clean {n} items (to Recycle Bin)',
    'overview.clean.now': 'Clean now',
    'overview.clean.done': 'Cleaned {n} items · about {size} freed (moved to Recycle Bin)',
    'overview.clean.someFailed': ' · {n} failed',
    'overview.clean.allFailed': 'Cleanup failed for {n} items: {err} (see console)',
    'overview.clean.nothingMatched': 'No matching files this time (maybe already cleaned, or the app is holding them)',

    // Low-space banner (usage >= 85%)
    'overview.rescue.title': 'The {drive} drive is {pct}% full',
    'overview.rescue.scannedHas': 'About {size} is safe to clean below — ticked items go to the system Recycle Bin',
    'overview.rescue.scannedNone': 'This drive is scanned: start with large files, or check the cleanup list below',
    'overview.rescue.notScanned': 'Scan it once and DiskPilot lists what is safe to clean (caches / temp files, Recycle Bin by default)',
    'overview.rescue.scanning': 'Scanning…',
    'overview.rescue.scanNow': 'Scan now',
    'overview.rescue.viewClean': 'See cleanup items',

    // Duplicate scan → cleanup proposal (what / purpose / impact)
    'overview.dup.noneFound': 'No duplicates ≥1MB found (by size + header hash)',
    'overview.dup.allRunning': 'Every duplicate group only holds in-use files (skipped) — nothing to recycle',
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
