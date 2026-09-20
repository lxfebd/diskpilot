// 通用词：工具分类名 / 清理结果提示 / 会话通用文案，以及一批「不译」的数据字面量。
//
// 文件末尾 `common.data.*` / `common.mock.*` / `common.chatModel.*` 三组**不属于界面文案**：
// 它们或是与后端/跨模块约定的字符串（用于比较、匹配），或是发给 AI 模型的上下文模板，
// 按 namespaces/index.ts 的「不译清单」必须恒为中文 —— 故 en 与 zh 刻意同值（不是漏翻）。
// 消费方对这些键一律直接读中文表（`common.zh[...]`），不走 t()，以免语言切换打断匹配。
export const common = {
  ns: 'common',
  zh: {
    // 批量回收结果提示（store.recyclePaths / store.aiRecyclePaths 的成功 toast）
    'common.recycle.done': '已回收 {n} 项 · 释放约 {size}',
    'common.recycle.aiDone': '已按你的确认回收 {n} 项（回收站可找回） · 约 {size}',

    // 通用词
    'common.chat.untitled': '未命名',
    // 两步确认面板按钮（ConfirmDialog）
    'common.confirm.cancel': '取消',
    'common.confirm.ok': '确认',

    // 工具墙分类名（后端 toolbelt manifest 的 category 值；色值/图标按中文名匹配）
    'common.category.cpu': '处理器工具',
    'common.category.gpu': '显卡工具',
    'common.category.disk': '硬盘工具',
    'common.category.suite': '综合检测',
    'common.category.other': '其他工具',
    'common.category.stress': '烤鸡工具',
    'common.category.memory': '内存工具',
    'common.category.daily': '常用工具',
    'common.category.peripheral': '外设工具',
    'common.category.board': '主板工具',
    'common.category.game': '游戏工具',

    // ── 以下三组不译（en 与 zh 同值，见文件头说明）──
    // 多盘合并扫描的虚拟根 path（App.tsx 写入、store.ts 比较，是数据不是文案）
    'common.data.allDrivesRoot': '全部磁盘',
    // 浏览器 mock 环境冒充后端返回的探测说明（与真后端同口径，保持中文）
    'common.mock.fanReadonlyChannel': '浏览器 mock 环境无可写调速通道',
    // 喂给 AI 模型的会话上下文模板（advisor/chatHistory.ts 派生上下文用）
    'common.chatModel.procCalls': '调用了 {n} 个工具',
    'common.chatModel.procResults': '{n} 次工具返回',
    'common.chatModel.procWrap': '[过程：{text}]',
    'common.chatModel.procJoin': '，',
    'common.chatModel.noteJoin': '；',
    'common.chatModel.earlierUser': '（早前提问：{text}…）',
    'common.chatModel.earlierAssistant': '（早前回答：{text}…）',
  },
  en: {
    'common.recycle.done': 'Recycled {n} items · about {size} freed',
    'common.recycle.aiDone': 'Recycled {n} items as you confirmed (recoverable from the Recycle Bin) · about {size}',

    'common.chat.untitled': 'Untitled',
    'common.confirm.cancel': 'Cancel',
    'common.confirm.ok': 'Confirm',

    'common.category.cpu': 'CPU tools',
    'common.category.gpu': 'GPU tools',
    'common.category.disk': 'Disk tools',
    'common.category.suite': 'Full diagnostics',
    'common.category.other': 'Other tools',
    'common.category.stress': 'Stress tests',
    'common.category.memory': 'Memory tools',
    'common.category.daily': 'Essentials',
    'common.category.peripheral': 'Peripheral tools',
    'common.category.board': 'Motherboard tools',
    'common.category.game': 'Game tools',

    // ── Not UI copy — kept in Chinese on purpose (see header note) ──
    'common.data.allDrivesRoot': '全部磁盘',
    'common.mock.fanReadonlyChannel': '浏览器 mock 环境无可写调速通道',
    'common.chatModel.procCalls': '调用了 {n} 个工具',
    'common.chatModel.procResults': '{n} 次工具返回',
    'common.chatModel.procWrap': '[过程：{text}]',
    'common.chatModel.procJoin': '，',
    'common.chatModel.noteJoin': '；',
    'common.chatModel.earlierUser': '（早前提问：{text}…）',
    'common.chatModel.earlierAssistant': '（早前回答：{text}…）',
  },
};
