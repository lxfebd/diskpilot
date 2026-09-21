// 应用外壳：导航 / 磁盘条 / 侧栏 / 错误边界等跨页面文案
export const shell = {
  ns: 'shell',
  zh: {
    'shell.drivestrip.allScanned': '所有硬盘已扫描过，点一下秒开',
    'shell.drivestrip.scanAllTitle': '扫描所有硬盘（已扫过的秒开）',
    'shell.drivestrip.scanAll': '一键扫描全部硬盘',
    'shell.drivestrip.scanAllSub': '扫完所有盘 · 已扫过的秒开 · 只读扫描不改任何文件',
    'shell.drivestrip.openLast': '打开 {path} 的上次扫描结果（秒开）',
    'shell.drivestrip.scanNow': '扫描 {path}（还剩 {free} 可用）',
    'shell.drivestrip.rescan': '重新扫描 {path}',
    'shell.drivestrip.lowSpace': '空间告急',
    'shell.drivestrip.scanned': '已扫描',
    'shell.drivestrip.freeUsed': '可用 {free} · 已用 {used}',

    // 顶栏导航与状态
    'shell.nav.overview': '总览',
    'shell.nav.overviewTitle': '磁盘资产总览',
    'shell.nav.workspace': '工作台',
    'shell.nav.workspaceTitle': '目录树 + AI + 清理工作台',
    'shell.nav.tools': '工具',
    'shell.nav.toolsTitle': '工具箱：硬件信息 / 工具墙 / 网络检测 / 系统信息',
    'shell.nav.wsHint': '选择上方磁盘或文件夹后扫描',
    'shell.nav.reclaimed': '已释放 {size}',
    'shell.nav.reclaimedTitle': '本次使用 DiskPilot 清理释放的空间',
    'shell.nav.rootStatus': '{size} · {n} 文件',
    'shell.nav.settingsBound': '已绑定 {provider} · 点开管理',
    'shell.nav.settingsUnbound': 'AI 还没配置 · 点开设置',

    // 全局 AI 侧栏
    'shell.airail.title': 'AI 顾问',
    'shell.airail.collapse': '收起 AI 侧栏',
    'shell.airail.expand': '展开 AI 顾问',

    // 扫描入口与进度
    'shell.scan.start': '扫描',
    'shell.scan.cancel': '取消扫描',
    'shell.scan.pickFolder': '选择磁盘或文件夹',
    'shell.scan.picked': '已选 {path}',
    'shell.scan.browserPrompt': '浏览器预览模式：输入一个路径（任意值都可以）',
    'shell.scan.adminHint': '以管理员身份启动 DiskPilot 可走 MFT 快速通道，大幅缩短扫描时间。',
    'shell.scanbar.preparing': '准备扫描…',
    'shell.scanbar.progress': '{files} 个文件 · {bytes}',
    'shell.scanbar.done': '本次扫描 {files} 个文件 · {bytes}',

    // 扫描结果树根节点
    'shell.root.allDrives': '我的电脑（全部磁盘）',
    'shell.toast.openedFromCache': '已从上次扫描结果打开（点盘符上的刷新可重新扫描）',
    'shell.toast.cleanupReminder': '发现约 {size} 可清理空间（{n} 个盘），可在总览页查看并确认清理',

    // 左栏视图切换
    'shell.leftpanel.viewTree': '目录树',
    'shell.leftpanel.viewTreemap': '树状图',
    'shell.leftpanel.viewFiles': '文件',

    // 工作台空态
    'shell.empty.title': '👋 欢迎使用 DiskPilot',
    'shell.empty.howto': '在上方点一个硬盘（比如 C 盘），或点「扫描全部硬盘」，就能看到每个文件夹占了多少空间。',
    'shell.empty.layout': '扫完之后：左边是文件夹列表，中间可以问 AI，右边是一键清理。',
    'shell.empty.instant': '已扫过的盘再点会秒开上次结果，点盘符上的刷新图标才会重新扫描。',

    // 中部详情面板
    'shell.fdp.pickFolder': '👈 从左侧选一个文件夹',
    'shell.fdp.pickHint': '这里会显示选中项的占用详情、可回收项，以及 AI 分析入口。',
    'shell.fdp.locate': '在左侧定位',
    'shell.fdp.used': '占用',
    'shell.fdp.files': '文件',
    'shell.fdp.pctOfScan': '占总扫描',
    'shell.fdp.askTitle': '把该目录发给 AI 分析',
    'shell.fdp.askAi': 'AI 分析此目录',
    'shell.fdp.recycle': '回收',
    'shell.fdp.topChildren': '占用子项 Top 6',

    // 页脚
    'shell.footer.rules': 'DiskPilot v0.2.0 · 内置 {n} 条清理规则',
    'shell.footer.noScan': '还没扫描',

    // 错误边界
    'shell.boundary.fallback': '组件渲染失败',
    'shell.boundary.reset': '重置该面板',
    'shell.boundary.appRender': 'DiskPilot 渲染失败',
    'shell.boundary.overviewFailed': '资产总览渲染失败',
    'shell.boundary.detailFailed': '详情面板渲染失败',
    'shell.boundary.studioFailed': 'Studio 面板渲染失败',

    // 进度按钮 / 分隔条
    'shell.progressbutton.cleaning': '清理中',
    'shell.progressbutton.failed': '失败',
    'shell.splitter.hint': '拖动调整 · 双击重置',

    // 文件树右键菜单
    'shell.tooltree.openInExplorer': '在文件管理器中打开',
    'shell.tooltree.copyPath': '复制路径',
    'shell.tooltree.recycle': '移入回收站',
    'shell.tooltree.recycleConfirm': '确定把「{name}」移入回收站吗？\n释放约 {size}，可从系统回收站找回。',

    // 开屏免责声明（行为与文案语义不得改动）
    'shell.disclaimer.ariaLabel': '免责声明',
    'shell.disclaimer.title': '使用前必读 · 免责声明',
    'shell.disclaimer.sub': '请仔细阅读以下内容，再决定是否使用本软件',
    'shell.disclaimer.authorHeading': '作者信息',
    'shell.disclaimer.authorLabel': '作者：',
    'shell.disclaimer.authorName': '涙不再为你而流',
    'shell.disclaimer.qqLabel': 'QQ：',
    'shell.disclaimer.emailLabel': '邮箱：',
    'shell.disclaimer.githubLabel': 'GitHub：',
    'shell.disclaimer.h1': '一、软件性质与用途',
    'shell.disclaimer.p1': 'DiskPilot 是一款面向个人用户的 Windows 磁盘扫描与清理辅助工具。本软件基于 Tauri 2 + Rust + React 开发，主要功能包括：磁盘占用扫描、文件归类展示、按应用清理缓存（scaffold 机制）、AI 辅助分析（需用户自行配置 AI 服务商 API Key，BYOK 模式）。',
    'shell.disclaimer.h2': '二、数据与隐私',
    'shell.disclaimer.p2': '本软件**不收集、不上传、不存储**任何个人数据：无遥测、无账号系统、无云存储。所有扫描结果、清理记录、配置数据仅保存在本机。唯一可能的网络请求来自你主动配置的 AI 服务商接口——你发送的目录元数据（仅路径与大小，不含文件内容）会按你选择的厂商、以你配置的 Key 发送，用于生成分析建议。请自行评估所分析目录的敏感程度。',
    'shell.disclaimer.h3': '三、清理行为与安全边界',
    'shell.disclaimer.p3': '本软件遵循「宁可错放 1000GB，不可错删一个文件」的安全原则：',
    'shell.disclaimer.p3li1': '所有清理操作**先出清单、经你确认**后才执行，绝不自动清理；',
    'shell.disclaimer.p3li2': '清理默认进入**系统回收站**，可随时还原；「彻底删除」仅在你显式选择时使用；',
    'shell.disclaimer.p3li3': '扫描/清理/基准测试均**不触碰**系统目录、数据库文件、聊天记录、账号状态、加密物料等受保护路径；',
    'shell.disclaimer.p3li4': '压测类功能（CPU/GPU/甜甜圈）会显著占用资源并升温，请确认散热可靠后再运行。',
    'shell.disclaimer.h4': '四、风险与责任声明',
    'shell.disclaimer.p4': '本软件按「现状」提供，作者尽力保证可靠与安全，但**不承担**因以下情形导致的任何直接、间接或连带损失：',
    'shell.disclaimer.p4li1': '因用户误操作、未仔细确认清理清单、或擅自修改配置导致的文件误删；',
    'shell.disclaimer.p4li2': '因第三方 AI 服务（模型输出、接口故障、服务中断、隐私风险）引发的任何问题；',
    'shell.disclaimer.p4li3': '因硬件超频/压测/温度过高导致的硬件损伤；',
    'shell.disclaimer.p4li4': '因系统环境差异（驱动、权限、杀软拦截、Windows 更新）导致的功能异常；',
    'shell.disclaimer.p4li5': '在商用/关键业务环境使用本软件造成的业务损失——本软件仅面向个人非商用场景。',
    'shell.disclaimer.p4close': '本软件为**免费开源**项目，无任何商业承诺与售后义务。使用即视为已阅读并同意本声明全部条款。',
    'shell.disclaimer.h5': '五、开源许可与第三方声明',
    'shell.disclaimer.p5': '本软件以开源形式发布，遵循仓库内声明的许可协议。软件集成的第三方工具（如图吧工具箱内各硬件工具）版权归各自作者所有，仅作本机调用，不修改、不分发其二进制。若第三方工具侵犯你的权益，请通过上方联系方式联系作者处理。',
    'shell.disclaimer.tipAgreed': '我已阅读并同意以上条款',
    'shell.disclaimer.tipReadMore': '请再阅读 {s} 秒',
    'shell.disclaimer.tipScroll': '请先滚动到底部阅读全部内容',
    'shell.disclaimer.countdown': '我已阅读（{s}s）',
    'shell.disclaimer.accept': '我同意以上声明，开始使用',
    'shell.disclaimer.scrollFirst': '请滑动至底部',
  },
  en: {
    'shell.drivestrip.allScanned': 'All drives already scanned — click to open instantly',
    'shell.drivestrip.scanAllTitle': 'Scan every drive (already-scanned ones open instantly)',
    'shell.drivestrip.scanAll': 'Scan all drives',
    'shell.drivestrip.scanAllSub': 'Scan every drive · scanned ones open instantly · read-only, changes nothing',
    'shell.drivestrip.openLast': 'Open the last scan result for {path} (instant)',
    'shell.drivestrip.scanNow': 'Scan {path} ({free} free left)',
    'shell.drivestrip.rescan': 'Rescan {path}',
    'shell.drivestrip.lowSpace': 'Almost full',
    'shell.drivestrip.scanned': 'Scanned',
    'shell.drivestrip.freeUsed': '{free} free · {used} used',

    // 顶栏导航与状态
    'shell.nav.overview': 'Overview',
    'shell.nav.overviewTitle': 'Disk space overview',
    'shell.nav.workspace': 'Workbench',
    'shell.nav.workspaceTitle': 'Folder tree + AI + cleanup workbench',
    'shell.nav.tools': 'Tools',
    'shell.nav.toolsTitle': 'Toolbox: hardware info / tool wall / network checks / system info',
    'shell.nav.wsHint': 'Pick a drive above or a folder, then scan',
    'shell.nav.reclaimed': '{size} freed',
    'shell.nav.reclaimedTitle': 'Space you freed with DiskPilot in this session',
    'shell.nav.rootStatus': '{size} · {n} files',
    'shell.nav.settingsBound': 'Connected to {provider} · click to manage',
    'shell.nav.settingsUnbound': 'No AI provider set up yet · open settings',

    // 全局 AI 侧栏
    'shell.airail.title': 'AI Advisor',
    'shell.airail.collapse': 'Collapse the AI panel',
    'shell.airail.expand': 'Open the AI advisor',

    // 扫描入口与进度
    'shell.scan.start': 'Scan',
    'shell.scan.cancel': 'Cancel scan',
    'shell.scan.pickFolder': 'Choose a drive or folder',
    'shell.scan.picked': 'Selected {path}',
    'shell.scan.browserPrompt': 'Browser preview mode: type any path to scan',
    'shell.scan.adminHint': 'Start DiskPilot as administrator to use the fast MFT path and cut scan time dramatically.',
    'shell.scanbar.preparing': 'Preparing scan…',
    'shell.scanbar.progress': '{files} files · {bytes}',
    'shell.scanbar.done': 'This scan: {files} files · {bytes}',

    // 扫描结果树根节点
    'shell.root.allDrives': 'My Computer (all drives)',
    'shell.toast.openedFromCache': 'Opened from the last scan result — click the refresh badge on a drive to rescan it.',
    'shell.toast.cleanupReminder': 'About {size} can be cleaned up on {n} drive(s) — review and confirm on the Overview page',

    // 左栏视图切换
    'shell.leftpanel.viewTree': 'Tree',
    'shell.leftpanel.viewTreemap': 'Map',
    'shell.leftpanel.viewFiles': 'Files',

    // 工作台空态
    'shell.empty.title': '👋 Welcome to DiskPilot',
    'shell.empty.howto': 'Click a drive above (C: for example), or click "Scan all drives", to see how much space each folder takes.',
    'shell.empty.layout': 'When the scan finishes: folders on the left, ask the AI in the middle, one-click cleanup on the right.',
    'shell.empty.instant': 'Clicking a drive you already scanned opens the last result instantly; use the refresh badge to rescan it.',

    // 中部详情面板
    'shell.fdp.pickFolder': '👈 Pick a folder on the left',
    'shell.fdp.pickHint': 'This is where the selected item\'s size, what can be reclaimed, and the AI analysis entry point appear.',
    'shell.fdp.locate': 'Locate in the left panel',
    'shell.fdp.used': 'used',
    'shell.fdp.files': 'files',
    'shell.fdp.pctOfScan': 'of this scan',
    'shell.fdp.askTitle': 'Send this folder to the AI for analysis',
    'shell.fdp.askAi': 'Ask AI about this folder',
    'shell.fdp.recycle': 'Recycle',
    'shell.fdp.topChildren': 'Top 6 items by size',

    // 页脚
    'shell.footer.rules': 'DiskPilot v0.2.0 · {n} built-in cleanup rules',
    'shell.footer.noScan': 'Nothing scanned yet',

    // 错误边界
    'shell.boundary.fallback': 'This component failed to render',
    'shell.boundary.reset': 'Reset this panel',
    'shell.boundary.appRender': 'DiskPilot failed to render',
    'shell.boundary.overviewFailed': 'Asset overview failed to render',
    'shell.boundary.detailFailed': 'Detail panel failed to render',
    'shell.boundary.studioFailed': 'Studio panel failed to render',

    // 进度按钮 / 分隔条
    'shell.progressbutton.cleaning': 'Cleaning',
    'shell.progressbutton.failed': 'Failed',
    'shell.splitter.hint': 'Drag to resize · double-click to reset',

    // 文件树右键菜单
    'shell.tooltree.openInExplorer': 'Open in File Explorer',
    'shell.tooltree.copyPath': 'Copy path',
    'shell.tooltree.recycle': 'Move to Recycle Bin',
    'shell.tooltree.recycleConfirm': 'Move "{name}" to the Recycle Bin?\nAbout {size} will be freed, and you can restore it from the Recycle Bin.',

    // 开屏免责声明（行为与文案语义不得改动）
    'shell.disclaimer.ariaLabel': 'Disclaimer',
    'shell.disclaimer.title': 'Read before you start · Disclaimer',
    'shell.disclaimer.sub': 'Please read the following carefully before deciding to use this software',
    'shell.disclaimer.authorHeading': 'About the author',
    'shell.disclaimer.authorLabel': 'Author: ',
    'shell.disclaimer.authorName': '涙不再为你而流',
    'shell.disclaimer.qqLabel': 'QQ: ',
    'shell.disclaimer.emailLabel': 'Email: ',
    'shell.disclaimer.githubLabel': 'GitHub: ',
    'shell.disclaimer.h1': '1. What this software is',
    'shell.disclaimer.p1': 'DiskPilot is a Windows disk scanning and cleanup helper for individual users. Built on Tauri 2 + Rust + React, it scans disk usage, groups files for display, clears per-app caches (the scaffold mechanism), and offers AI-assisted analysis (BYOK — you configure your own AI provider API key).',
    'shell.disclaimer.h2': '2. Data & privacy',
    'shell.disclaimer.p2': 'This software does **not collect, upload, or store any personal data**: no telemetry, no accounts, no cloud storage. Every scan result, cleanup record, and setting stays on this machine. The only possible outbound request is to an AI provider you configure yourself — the folder metadata you send (paths and sizes only, never file contents) goes to the vendor you chose with the key you configured, in order to produce the analysis. Judge for yourself how sensitive a folder is before you send it.',
    'shell.disclaimer.h3': '3. Cleanup behaviour & safety boundaries',
    'shell.disclaimer.p3': 'This software follows one safety principle — "rather leave 1000GB of junk in place than delete a single wrong file":',
    'shell.disclaimer.p3li1': 'Every cleanup **lists what it will do and waits for your confirmation** first; nothing is ever cleaned automatically;',
    'shell.disclaimer.p3li2': 'Cleanups go to the **system Recycle Bin** by default and can be restored at any time; "delete permanently" is used only when you explicitly choose it;',
    'shell.disclaimer.p3li3': 'Scanning, cleanup, and benchmarks **never touch protected paths** such as system directories, database files, chat histories, account state, or key material;',
    'shell.disclaimer.p3li4': 'Stress tests (CPU/GPU/donut) load the machine heavily and raise temperatures — make sure your cooling is up to it before running them.',
    'shell.disclaimer.h4': '4. Risk & liability',
    'shell.disclaimer.p4': 'This software is provided "as is". The author does their best to keep it reliable and safe, but **accepts no** direct, indirect, or consequential loss arising from:',
    'shell.disclaimer.p4li1': 'files deleted through user mistakes, a cleanup list that was not reviewed carefully, or hand-edited configuration;',
    'shell.disclaimer.p4li2': 'any problem caused by third-party AI services (model output, API failures, outages, privacy risks);',
    'shell.disclaimer.p4li3': 'hardware damage from overclocking, stress testing, or overheating;',
    'shell.disclaimer.p4li4': 'malfunction caused by differences in your environment (drivers, permissions, antivirus interference, Windows updates);',
    'shell.disclaimer.p4li5': 'business loss in a commercial or mission-critical setting — this software is for personal, non-commercial use only.',
    'shell.disclaimer.p4close': 'This is a **free, open-source** project with no commercial warranty and no support obligations. Using it means you have read and accept every clause of this disclaimer.',
    'shell.disclaimer.h5': '5. Licensing & third-party tools',
    'shell.disclaimer.p5': 'This software is released as open source under the license declared in the repository. Third-party tools it integrates (such as the individual hardware utilities in 图吧工具箱) remain the copyright of their respective authors; they are only invoked locally, and their binaries are neither modified nor redistributed. If a third-party tool infringes on your rights, contact the author through the channels above.',
    'shell.disclaimer.tipAgreed': 'I have read and agree to the terms above',
    'shell.disclaimer.tipReadMore': 'Keep reading for {s} more second(s)',
    'shell.disclaimer.tipScroll': 'Scroll to the bottom and read everything first',
    'shell.disclaimer.countdown': 'I have read this ({s}s)',
    'shell.disclaimer.accept': 'I accept the statement above — start using it',
    'shell.disclaimer.scrollFirst': 'Scroll to the bottom',
  },
};
