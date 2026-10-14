// R134：AI 工具「同类推荐」——agent-server 的 MCP 工具没有 tags/category 元数据，
// 只有 name + description，所以同类判断靠描述词袋重叠打分。纯函数，便于单测。
//
// ⚠️ 停用词表含中文：这是匹配用的常量字符串（非 UI 文案），按约定保持原生书写。
// @i18n-keep

/** 中文停用词：虚词/高频无信息量字。@i18n-keep */
const CJK_STOP = new Set([
  '的', '了', '与', '和', '或', '是', '在', '对', '从', '把', '被', '以',
  '为', '中', '上', '下', '及', '等', '个', '一', '不', '也', '就', '并',
  '而', '但', '其', '此', '之', '于', '它', '该', '将', '让', '用', '可',
  '能', '会', '有', '无', '到', '按', '经', '通过', '进行', '执行', '返回',
]);

/** 英文停用词：功能词/非常见描述词。 */
const EN_STOP = new Set([
  'the', 'a', 'an', 'of', 'to', 'in', 'on', 'for', 'and', 'or', 'with',
  'from', 'by', 'as', 'at', 'is', 'are', 'be', 'was', 'were', 'this',
  'that', 'it', 'its', 'get', 'gets', 'set', 'sets', 'all', 'any', 'into',
  'via', 'using', 'used', 'use', 'can', 'will', 'has', 'have', 'return',
  'returns', 'info', 'information', 'list', 'lists', 'show', 'shows',
]);

/** 把描述切成小写词集（去重、去停用）。
 *  分词策略：拉丁/数字连续串一个 token；CJK 走双向滑窗双字组词（如
 *  「温度」「获取」），比单字切分更贴近概念、减少「温/度」式过拟合——
 *  「温度」与「湿度」共享单字会被误判同类，双字组词后不再。 */
export function descTokens(desc: string): Set<string> {
  const out = new Set<string>();
  const lc = desc.toLowerCase();
  // ASCII 连续串（拉丁字母/数字）
  for (const m of lc.match(/[a-z0-9]+/g) ?? []) {
    if (!EN_STOP.has(m)) out.add(m);
  }
  // CJK 双字滑窗（去停用字对其）
  const cjk = (lc.match(/[\u4e00-\u9fff]+/g) ?? []).join('');
  for (let i = 0; i + 1 < cjk.length; i += 1) {
    const a = cjk[i];
    const b = cjk[i + 1];
    if (CJK_STOP.has(a) || CJK_STOP.has(b)) continue;
    out.add(a + b);
  }
  return out;
}

/** 两描述的词袋重叠数（不对称的 score(b) = |A ∩ B|）。 */
export function descOverlap(a: Set<string>, b: Set<string>): number {
  let n = 0;
  for (const tok of a) if (b.has(tok)) n += 1;
  return n;
}

/** R153：AI 工具描述充分度——描述为空或全是虚词（词袋空）时，卡片缺一行、
 *  同类推荐也匹配不到，用户不知道为什么。显式分档：rich（≥2 个有效词）/
 *  sparse（1 个）/ none（0 个，空描述或全停用词）。纯函数：复用 descTokens。 */
export function agentDescQuality(desc: string): 'rich' | 'sparse' | 'none' {
  const n = descTokens(desc).size;
  if (n === 0) return 'none';
  return n >= 2 ? 'rich' : 'sparse';
}

/** 同类 AI 工具：描述与 target 词袋重叠的（排除自身），按重叠数降序，取前 limit。
 *  无描述/空词袋的工具不参与（无法判断，不硬凑）。纯函数：不改入参。 */
export function similarAgentTools(
  target: { name: string; description: string },
  all: readonly { name: string; description: string }[],
  limit: number = 4,
): { name: string; description: string }[] {
  const mine = descTokens(target.description);
  if (mine.size === 0) return [];
  return all
    .map((t) => ({ t, hits: descOverlap(mine, descTokens(t.description)) }))
    .filter((x) => x.hits > 0 && x.t.name !== target.name)
    .sort((a, b) => b.hits - a.hits || a.t.name.localeCompare(b.t.name))
    .slice(0, limit)
    .map((x) => x.t);
}