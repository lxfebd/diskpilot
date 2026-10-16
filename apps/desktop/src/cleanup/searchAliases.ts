// ── 清理脚本搜索别名表 ─────────────────────────────────────────────
// 脚本名多为英文（WeChat (PC) / OBS Studio 缓存等），中文用户搜索
// 中文词（微信 / 直播 / 视频会议）时也能命中。
// 这是搜索索引数据，不是 UI 文案——按 namespaces/index.ts 的「不译清单」
// 先例（mocks.ts 同口径），本文件不进 MIGRATED_FILES，中文是数据非欠账。
export const SCRIPT_SEARCH_ALIASES: Record<string, string[]> = {
  'wechat-pc': ['微信', 'weixin'],
  'qq-pc': ['qq 聊天', '企鹅'],
  'browser-cache': ['浏览器'],
  'browser-privacy': ['浏览记录', '历史记录'],
  'dev-caches': ['开发', '编程'],
  'engine-caches': ['游戏引擎', 'unreal', 'unity', 'godot'],
  'steam-shadercache': ['steam'],
  'huggingface-cache': ['模型', 'ai'],
  'docker-buildx': ['docker'],
  'ide-caches': ['ide', 'vscode', 'cursor', 'intellij', '编辑器'],
  'system-temp': ['临时'],
  'windows-privacy': ['隐私', '痕迹'],
  'obs-cache': ['直播', '录屏'],
  'crash-dumps': ['崩溃', '转储'],
  'teams': ['teams', '会议'],
  'discord': ['discord'],
  'zoom': ['视频会议', 'zoom'],
  'firefox-cache': ['firefox'],
  'conda': ['conda', 'python', 'anaconda'],
};
