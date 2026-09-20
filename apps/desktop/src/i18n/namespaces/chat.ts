// AI 聊天面板（ChatPanel）：会话列表 / 头部 / 空态引导 / 气泡与思考过程 /
// 三类确认面板（CLI 工具 · L2 会话操作 · 回收两步确认）/ 输入区。
//
// 有意留中文、不进文案表的（属设计，不是欠账）：
// - 发给模型的 prompt：扫描状态注入行、Studio 卡片提问与指令、目标对象行、
//   默认提问、quickAsk 的提问参数 —— 译了会改变模型行为；
// - 用于匹配的字符串常量：/已取消|abort/ 判定用户主动停止、/^调用\s*/ 剥前缀；
// - 后端 error string 原文（只把它当 {msg} 插进文案，不翻本体）。
export const chat = {
  ns: 'chat',
  zh: {
    // 异常与状态回合
    'chat.err.stopped': 'AI 回答已停止（你点了「停止」按钮）',
    'chat.err.callFailed': 'AI 调用失败：{msg}',

    // 气泡占位与思考过程折叠区
    'chat.trace.toolRunning': '正在{text}…',
    'chat.trace.analyzing': '分析结果中…',
    'chat.trace.thinking': '正在思考…',
    'chat.trace.thinkingShort': '思考中…',
    'chat.trace.head': '思考过程 · {n} 步',
    'chat.trace.inProgress': '（进行中…）',
    'chat.trace.elapsed': '已运行 {secs}s',
    'chat.trace.runningNow': '运行中…',
    'chat.chip.thinking': '思考',
    'chat.chip.toolCall': '工具',
    'chat.chip.toolResult': '结果',
    'chat.chip.note': '提示',

    // 建议标签卡
    'chat.advice.riskLabel': '风险',
    'chat.advice.needsInspection': '需要再看看',
    'chat.action.recycle': '回收 {size}',

    // AI 回合的开场与兜底文案
    'chat.studio.analyzing': '正在分析 {name}…',
    'chat.overview.scanned': '已扫完 {path} · {size} · {count} 个文件。AI 正在生成整体解析…',
    'chat.overview.generating': 'AI 正在生成整体解析…',
    'chat.overview.dragHint': '你可以从左边把任意文件夹/文件拖进来问。',
    'chat.drop.about': '（关于：{paths}）',
    'chat.drop.images': '（带 {n} 张图片）',

    // 系统回合（不进 AI 上下文，纯界面反馈）
    'chat.sys.imageTooBig': '图片 {name} 太大（>20MB），跳过',
    'chat.sys.imageReadFailed': '读取图片失败：{msg}',
    'chat.sys.recycled': '已回收 {path} · 释放 {size}',
    'chat.sys.recycleFailed': '回收失败：{msg}',

    // 会话列表与头部
    'chat.sessions.head': '对话列表',
    'chat.sessions.newTitle': '新对话',
    'chat.sessions.untitled': '未命名',
    'chat.sessions.deleteTitle': '删除会话',
    'chat.head.stats': '{path} · {size} · {count} 文件',
    'chat.head.idle': '随时可对话 · 也能扫盘让 AI 分析',
    'chat.head.clearTitle': '清空（同时清除本会话的权限免确认）',

    // 空态引导
    'chat.hero.intro': '不用先扫磁盘，直接跟我说就行。我能看电脑配置、查硬件健康、分析工具跑出来的数据、帮你清理磁盘。',
    'chat.hero.pcInfo': '查看电脑信息',
    'chat.hero.hwHealth': '查看硬件健康',
    'chat.hero.capabilities': '我能帮你做什么',
    'chat.hero.browserMode': '浏览器预览模式：扫描数据是模拟的，但 AI 会走真实接口；硬件类工具需真实桌面端才生效。',
    // AI 未配置时的空态引导：主句 + 三个带人话说明的快捷按钮（仍可用：点下去会弹设置引导而非报错）
    'chat.hero.aiNotSetup': '还没有配置 AI 助手。配置后可以问它：',
    'chat.hero.aiNotSetupDesc': 'AI 需要自己的 API key（Anthropic / OpenAI / Gemini / Ollama 任选其一）。',
    'chat.hero.aiGoSettings': '去配置 AI',
    'chat.hero.aiSamples': '配置前也能先用扫描与清理功能',
    'chat.typing': 'AI 正在打字…',

    // 确认面板共用 + 工具箱 CLI 工具（中/高风险）
    'chat.confirm.remember': '本次会话免确认',
    'chat.confirm.cancel': '取消',
    'chat.confirm.run': '确认执行',
    'chat.cli.head': '工具箱 CLI 工具 · 需确认',
    'chat.cli.riskLabel': '风险',
    'chat.cli.timeout': ' · 超时 {secs}s',
    'chat.cli.rememberTitle': '勾选后，本会话内其它中/高风险 CLI 工具不再弹确认',

    // L2 会话操作确认（插件管理 / 电源计划等）
    'chat.session.head': '{title} · 需确认',
    'chat.session.permLabel': '权限',
    'chat.session.everyRun': ' · 每次执行需确认',
    'chat.session.rememberTitle': '勾选后，本会话内同类操作（含其他插件管理/电源计划）不再弹确认',

    // 「回收此项」两步确认（安全闸：文案必须把去向和可还原说清）
    'chat.recycle.head': '移入回收站 · 需确认',
    'chat.recycle.pathLabel': '完整路径',
    'chat.recycle.note': '{reason} · 进系统回收站，可随时还原',
    'chat.recycle.confirm': '确认回收',

    // 输入区
    'chat.input.attachTitle': '加图片（也可以粘贴/拖进来）',
    'chat.input.placeholderScanned': '问 AI：这是什么？能删吗？把文件 / 图片拖进来…（图片粘贴也行）',
    'chat.input.placeholderIdle': '直接问 AI：查看电脑配置、硬件健康、清理建议，或贴张图片问',
    'chat.input.stopTitle': '停止 AI 与正在运行的压测',
    'chat.input.stop': '停止',
    'chat.input.send': '发送',
  },
  en: {
    // Errors and status bubbles
    'chat.err.stopped': 'AI response stopped (you clicked "Stop")',
    'chat.err.callFailed': 'AI call failed: {msg}',

    // Placeholder bubbles and the reasoning trace
    'chat.trace.toolRunning': 'Running {text}…',
    'chat.trace.analyzing': 'Analyzing the result…',
    'chat.trace.thinking': 'Thinking…',
    'chat.trace.thinkingShort': 'Thinking…',
    'chat.trace.head': 'Reasoning · {n} steps',
    'chat.trace.inProgress': ' (in progress…)',
    'chat.trace.elapsed': 'running {secs}s',
    'chat.trace.runningNow': 'Running…',
    'chat.chip.thinking': 'Thought',
    'chat.chip.toolCall': 'Tool',
    'chat.chip.toolResult': 'Result',
    'chat.chip.note': 'Note',

    // Advice card
    'chat.advice.riskLabel': 'Risk',
    'chat.advice.needsInspection': 'Needs a closer look',
    'chat.action.recycle': 'Recycle {size}',

    // AI turn openers and fallbacks
    'chat.studio.analyzing': 'Analyzing {name}…',
    'chat.overview.scanned': 'Scanned {path} · {size} · {count} files. Writing the overview…',
    'chat.overview.generating': 'Writing the overview…',
    'chat.overview.dragHint': 'You can drag any folder or file from the left into the chat.',
    'chat.drop.about': '(about: {paths})',
    'chat.drop.images': '({n} image attached)',

    // System turns (never sent back to the model)
    'chat.sys.imageTooBig': 'Skipped {name}: image is larger than 20MB',
    'chat.sys.imageReadFailed': 'Could not read the image: {msg}',
    'chat.sys.recycled': 'Recycled {path} · {size} freed',
    'chat.sys.recycleFailed': 'Recycle failed: {msg}',

    // Session list and header
    'chat.sessions.head': 'Conversations',
    'chat.sessions.newTitle': 'New chat',
    'chat.sessions.untitled': 'Untitled',
    'chat.sessions.deleteTitle': 'Delete chat',
    'chat.head.stats': '{path} · {size} · {count} files',
    'chat.head.idle': 'Chat anytime — or scan a drive and let the AI dig in',
    'chat.head.clearTitle': "Clear (also drops this session's no-confirm grants)",

    // Empty-state hero
    'chat.hero.intro': 'No need to scan first — just ask. I can read your PC specs, check hardware health, make sense of tool output, and clean up your disk.',
    'chat.hero.pcInfo': 'See PC specs',
    'chat.hero.hwHealth': 'Check hardware health',
    'chat.hero.capabilities': 'What can you do?',
    'chat.hero.browserMode': 'Browser preview mode: scan data is simulated, but AI calls are real. Hardware tools need the desktop app.',
    'chat.hero.aiNotSetup': 'The AI assistant isn\'t configured yet. Once it is, you can ask it:',
    'chat.hero.aiNotSetupDesc': 'AI needs its own API key (pick Anthropic / OpenAI / Gemini / Ollama).',
    'chat.hero.aiGoSettings': 'Configure AI',
    'chat.hero.aiSamples': 'Scanning and cleanup already work before you configure it',
    'chat.typing': 'AI is typing…',

    // Shared confirm-panel chrome + Toolbelt CLI tools (medium/high risk)
    'chat.confirm.remember': 'No more confirmations this session',
    'chat.confirm.cancel': 'Cancel',
    'chat.confirm.run': 'Confirm and run',
    'chat.cli.head': 'Toolbelt CLI tool · confirmation required',
    'chat.cli.riskLabel': 'Risk',
    'chat.cli.timeout': ' · timeout {secs}s',
    'chat.cli.rememberTitle': 'If checked, other medium/high-risk CLI tools stop asking for confirmation in this session',

    // L2 session action confirmation (plugin management / power plans)
    'chat.session.head': '{title} · confirmation required',
    'chat.session.permLabel': 'Permission',
    'chat.session.everyRun': ' · needs confirmation every time',
    'chat.session.rememberTitle': 'If checked, similar actions (other plugin management / power plans) stop asking for confirmation in this session',

    // "Recycle this item" two-step confirmation (safety gate: destination and
    // reversibility must stay explicit)
    'chat.recycle.head': 'Move to Recycle Bin · confirmation required',
    'chat.recycle.pathLabel': 'Full path',
    'chat.recycle.note': '{reason} · goes to the system Recycle Bin, restorable anytime',
    'chat.recycle.confirm': 'Move to Recycle Bin',

    // Input bar
    'chat.input.attachTitle': 'Add an image (pasting or dragging works too)',
    'chat.input.placeholderScanned': 'Ask the AI: what is this, can I delete it? Drop files / images here… (pasting works too)',
    'chat.input.placeholderIdle': 'Ask away: PC specs, hardware health, cleanup advice — or paste an image',
    'chat.input.stopTitle': 'Stop the AI and any stress test running',
    'chat.input.stop': 'Stop',
    'chat.input.send': 'Send',
  },
};
