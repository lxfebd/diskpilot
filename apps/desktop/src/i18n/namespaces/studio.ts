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
    'studio.hidden': '已隐藏',
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

    // ── 粘贴 toml 安装弹窗 ──
    'studio.installHead': '安装社区脚本',
    'studio.close': '关闭',
    'studio.needToml': '请粘贴脚本 toml 内容',
    'studio.installLead': '从社区仓库复制一份',
    'studio.installTail':
      '脚本内容粘贴到这里即可安装。脚本只定义「检测位置 + 可清理范围」，安装后不会自动执行任何清理。',
    'studio.tomlSample':
      'id = "my-tool"\nname = "我的工具缓存"\nrisk = "low"\n# detect / match / scopes ...',

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
    'studio.hidden': 'Hidden',
    'studio.scriptCount': '{n} scripts',
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

    // ── Paste-TOML install dialog ──
    'studio.installHead': 'Install a community script',
    'studio.close': 'Close',
    'studio.needToml': 'Please paste the script’s TOML content',
    'studio.installLead': 'Copy any',
    'studio.installTail':
      'script from the community repo and paste it here to install it. A script only declares where to look and what may be cleaned — installing it never runs a cleanup.',
    'studio.tomlSample':
      'id = "my-tool"\nname = "My tool cache"\nrisk = "low"\n# detect / match / scopes ...',

    // ── Script card details ──
    'studio.reveal': 'Open in File Explorer',
    'studio.copyPath': 'Copy path',
    'studio.locations': '{n} locations',
    'studio.notDetected': 'Not in this scan · using the script’s default paths',
    'studio.pathLabel': 'Paths',
    'studio.sizeLabel': 'Size',
    'studio.sizeMeta': '{size} · {files} files',
    'studio.acrossN': '(across {n} locations)',
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
