// Studio（右栏「清理脚本」工作台）：脚本卡片、社区仓库安装/卸载、
// 卡片内的路径与大小明细。工具卡（Steam 盘点）也归这里。
//
// 注意：这里的文案只描述入口与状态，真正的清理确认文案在 cleanup 命名空间
// （CleanupModal / CleanupProposalDialog / UndoPanel）。
export const studio = {
  ns: 'studio',
  zh: {
    // ── 面板与分区标题 ──
    'studio.title': '清理脚本',
    'studio.scriptCount': '{n} 个脚本',
    'studio.featured': '推荐',
    'studio.more': '更多',
    'studio.installTitle': '安装社区脚本（粘贴 toml）',
    'studio.fbSteamCard': 'Steam Inspector 卡片渲染失败',
    'studio.cardFailed': '{name} 卡片渲染失败',
    'studio.fbUndoPanel': '最近清理面板渲染失败',

    // ── 工具卡：Steam 盘点 ──
    'studio.steamName': 'Steam 游戏盘点',
    'studio.steamBlurb': '哪些游戏好久没玩 · 一键唤起 Steam 卸载',

    // ── 社区脚本仓库 ──
    'studio.communityRepo': '社区脚本仓库',
    'studio.communityHint': '一键安装更多清理脚本',
    'studio.loadingRegistry': '正在读取社区仓库…',
    'studio.registryEmpty': '社区仓库为空。',
    'studio.comingSoon': '即将上架',
    'studio.meta': 'v{version} · {author} · 风险 {risk}',
    'studio.signed': '✓ Ed25519 已签名',
    'studio.permMissing':
      '未开启「管理社区清理脚本」权限：请前往 设置 → AI 权限中心 开启后再安装。',
    'studio.installed': '已安装社区脚本 {id}',
    'studio.installedScript': '已安装脚本 {id}',
    'studio.refreshFailed': '刷新脚本列表失败：{msg}',
    'studio.confirmInstallTip': '安装会下载脚本到用户目录，确认？',
    'studio.installReadyTitle': '一键安装（后端校验签名 + 红线）',
    'studio.installNoUrl': '尚未提供下载地址',
    'studio.install': '安装',
    'studio.installing': '安装中…',
    'studio.confirmInstall': '确认安装',
    'studio.cancel': '取消',
    // R497：社区索引远程刷新
    'studio.refreshRegistry': '刷新索引',
    'studio.refreshingRegistry': '拉取最新索引…',
    'studio.refreshed': '已更新到 {count} 条社区脚本（来源 {url}）',
    'studio.refreshIndexFailed': '刷新索引失败：{msg}',
    'studio.registrySource': '来源 {url} · 更新于 {time}',
    'studio.registrySeed': '内置种子索引（尚未拉取远端）',
    'studio.unsignedIndex': '未签名索引',

    // ── 粘贴 toml 安装弹窗 ──
    'studio.installHead': '安装社区脚本',
    'studio.close': '关闭',
    'studio.needToml': '请粘贴脚本 toml 内容',
    'studio.installLead': '从社区仓库复制一份',
    'studio.installTail':
      '脚本内容粘贴到这里即可安装。脚本只定义「检测位置 + 可清理范围」，安装后不会自动执行任何清理。',
    'studio.tomlSample':
      'id = "my-tool"\nname = "我的工具缓存"\nrisk = "low"\n# detect / match / scopes ...',
    'studio.tomlAria': '清理规则 TOML',

    // ── Winapp2.ini 社区规则导入（BleachBit 生态取长补短）──
    'studio.tabToml': '粘贴 TOML',
    'studio.tabWinapp2': 'Winapp2.ini 导入',
    'studio.winapp2Lead':
      '粘贴 BleachBit / 社区维护的 winapp2.ini 文本（可直接用官方仓库最新版），解析成可清理脚本预览。危险条目会红字标注并拒绝安装。',
    'studio.winapp2Placeholder':
      '; winapp2.ini 内容（[Section] + FileKeyN= 等，可选）',
    'studio.winapp2Aria': 'winapp2.ini 内容',
    'studio.winapp2Parse': '解析预览',
    'studio.winapp2Parsing': '解析中…',
    'studio.winapp2Stale': '输入已改动，先重新解析一次——下面这份预览和勾选是按旧内容算的。',
    'studio.winapp2Reparse': '重新解析',
    'studio.winapp2Empty': '未解析出可清理文件规则（可能只含注册表项或内容为空）。',
    'studio.winapp2ParseFailed': '解析失败：{msg}',
    'studio.winapp2Scopes': '{n} 个清理项',
    'studio.winapp2RedLine': '红线拒绝：{msg}',
    'studio.winapp2InstallSelected': '安装 {n} 条',
    'studio.winapp2Installing': '安装中…',
    'studio.winapp2SelectAll': '全选',
    'studio.winapp2Clear': '清空',
    'studio.winapp2Installed': '已安装 {id}',
    'studio.winapp2InstallFailed': '安装失败：{msg}',
    'studio.winapp2NoneSelected': '请先勾选要安装的规则',
    'studio.winapp2AllSafe': '全部可安全安装（未命中红线）',
    'studio.winapp2SomeBlocked': '{n} 条被红线拒绝（自动跳过）',

    // ── 脚本卡片明细 ──
    'studio.reveal': '在文件管理器中打开',
    'studio.copyPath': '复制路径',
    'studio.locations': '{n} 个位置',
    'studio.notDetected': '未扫到 · 用脚本默认路径',
    'studio.pathLabel': '路径',
    'studio.sizeLabel': '大小',
    'studio.sizeMeta': '{size} · {files} 文件',
    'studio.acrossN': '（{n} 处合计）',
    'studio.dragTip': '拖到中间问 AI · 右键查看选项',
    'studio.topChildren': '占用最大的子项',
    'studio.collapse': '收起',
    'studio.expandAll': '展开全部（还有 {n}）',
    'studio.childTip': '{path}  ·  右键查看选项',
    'studio.configureCleanup': '配置清理…',
    'studio.askAI': '问 AI',
    'studio.defaultPaths': '脚本默认匹配路径',
    'studio.notesLabel': '说明',
    'studio.askAIWhere': '问 AI：它一般在哪、能不能删',

    // ── 卸载用户安装的脚本 ──
    'studio.uninstall': '卸载',
    'studio.uninstalling': '卸载中…',
    'studio.uninstallTitle': '卸载用户安装的脚本（内嵌脚本不可卸载）',
    'studio.uninstallConfirm':
      '确定卸载脚本「{id}」？\n\n只会删除用户安装的脚本文件，不影响内嵌内置脚本。',
    'studio.uninstalled': '已卸载脚本 {id}',
    'studio.uninstallFailed': '卸载失败：{msg}',
  },
  en: {
    // ── Panel and section headings ──
    'studio.title': 'Cleanup scripts',
    'studio.scriptCount': '{n:script|scripts}',
    'studio.featured': 'Recommended',
    'studio.more': 'More',
    'studio.installTitle': 'Install a community script (paste TOML)',
    'studio.fbSteamCard': 'Failed to render the Steam Inspector card',
    'studio.cardFailed': 'Failed to render the {name} card',
    'studio.fbUndoPanel': 'Failed to render the recent cleanups panel',

    // ── Tool card: Steam inventory ──
    'studio.steamName': 'Steam library inventory',
    'studio.steamBlurb': 'See which games you have not played in ages · launch the Steam uninstaller in one click',

    // ── Community script repository ──
    'studio.communityRepo': 'Community script repository',
    'studio.communityHint': 'Install more cleanup scripts in one click',
    'studio.loadingRegistry': 'Loading the community registry…',
    'studio.registryEmpty': 'The community registry is empty.',
    'studio.comingSoon': 'Coming soon',
    'studio.meta': 'v{version} · {author} · risk {risk}',
    'studio.signed': '✓ Signed with Ed25519',
    'studio.permMissing':
      'The “Manage community cleanup scripts” permission is off — enable it under Settings → AI Permissions, then install again.',
    'studio.installed': 'Installed community script {id}',
    'studio.installedScript': 'Installed script {id}',
    'studio.refreshFailed': 'Failed to refresh the script list: {msg}',
    'studio.confirmInstallTip': 'Installing downloads the script into your user folder. Continue?',
    'studio.installReadyTitle': 'One-click install (backend verifies signature and red lines)',
    'studio.installNoUrl': 'No download URL yet',
    'studio.install': 'Install',
    'studio.installing': 'Installing…',
    'studio.confirmInstall': 'Install',
    'studio.cancel': 'Cancel',
    // R497: community index remote refresh
    'studio.refreshRegistry': 'Refresh index',
    'studio.refreshingRegistry': 'Fetching latest index…',
    'studio.refreshed': 'Index updated: {count:community script|community scripts} (from {url})',
    'studio.refreshIndexFailed': 'Index refresh failed: {msg}',
    'studio.registrySource': 'Source {url} · updated {time}',
    'studio.registrySeed': 'Built-in seed index (remote not fetched yet)',
    'studio.unsignedIndex': 'unsigned index',

    // ── Paste-TOML install dialog ──
    'studio.installHead': 'Install a community script',
    'studio.close': 'Close',
    'studio.needToml': 'Please paste the script’s TOML content',
    'studio.installLead': 'Copy any',
    'studio.installTail':
      'script from the community repo and paste it here to install it. A script only declares where to look and what may be cleaned — installing it never runs a cleanup.',
    'studio.tomlSample':
      'id = "my-tool"\nname = "My tool cache"\nrisk = "low"\n# detect / match / scopes ...',
    'studio.tomlAria': 'Cleanup rule TOML',

    // ── Winapp2.ini community rule import (learning from the BleachBit ecosystem) ──
    'studio.tabToml': 'Paste TOML',
    'studio.tabWinapp2': 'Import Winapp2.ini',
    'studio.winapp2Lead':
      'Paste winapp2.ini text (the community-maintained rule file shared with BleachBit; the official repo’s latest version works) to parse it into installable cleanup scripts. Dangerous entries are flagged in red and refused.',
    'studio.winapp2Placeholder':
      '; winapp2.ini content ([Section] + FileKeyN= etc., optional)',
    'studio.winapp2Aria': 'winapp2.ini content',
    'studio.winapp2Parse': 'Parse preview',
    'studio.winapp2Parsing': 'Parsing…',
    'studio.winapp2Stale': 'Input changed — re-parse first. The preview below and its ticks came from the old text.',
    'studio.winapp2Reparse': 'Re-parse',
    'studio.winapp2Empty': 'No installable file-cleanup rules parsed (the text may contain only registry entries or be empty).',
    'studio.winapp2ParseFailed': 'Parse failed: {msg}',
    'studio.winapp2Scopes': '{n:cleanup scope|cleanup scopes}',
    'studio.winapp2RedLine': 'Refused by red line: {msg}',
    'studio.winapp2InstallSelected': 'Install {n}',
    'studio.winapp2Installing': 'Installing…',
    'studio.winapp2SelectAll': 'Select all',
    'studio.winapp2Clear': 'Clear',
    'studio.winapp2Installed': 'Installed {id}',
    'studio.winapp2InstallFailed': 'Install failed: {msg}',
    'studio.winapp2NoneSelected': 'Select the rules to install first',
    'studio.winapp2AllSafe': 'All safe to install (no red-line hits)',
    'studio.winapp2SomeBlocked': '{n} refused by the red line (skipped automatically)',

    // ── Script card details ──
    'studio.reveal': 'Open in File Explorer',
    'studio.copyPath': 'Copy path',
    'studio.locations': '{n:location|locations}',
    'studio.notDetected': 'Not in this scan · using the script’s default paths',
    'studio.pathLabel': 'Paths',
    'studio.sizeLabel': 'Size',
    'studio.sizeMeta': '{size} · {files:file|files}',
    'studio.acrossN': '(across {n:location|locations})',
    'studio.dragTip': 'Drag into the chat to ask the AI · right-click for options',
    'studio.topChildren': 'Largest sub-items',
    'studio.collapse': 'Collapse',
    'studio.expandAll': 'Show all ({n} more)',
    'studio.childTip': '{path}  ·  Right-click for options',
    'studio.configureCleanup': 'Configure cleanup…',
    'studio.askAI': 'Ask AI',
    'studio.defaultPaths': 'Paths the script matches by default',
    'studio.notesLabel': 'Notes',
    'studio.askAIWhere': 'Ask the AI: where it usually lives and whether it is safe to delete',

    // ── Uninstalling a user-installed script ──
    'studio.uninstall': 'Uninstall',
    'studio.uninstalling': 'Uninstalling…',
    'studio.uninstallTitle': 'Uninstall a script you installed (built-in scripts cannot be uninstalled)',
    'studio.uninstallConfirm':
      'Uninstall the “{id}” script?\n\nOnly the script file you installed is removed — the built-in scripts stay untouched.',
    'studio.uninstalled': 'Uninstalled script {id}',
    'studio.uninstallFailed': 'Uninstall failed: {msg}',
  },
};
