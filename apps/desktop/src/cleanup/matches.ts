// 脚本命中收集：从磁盘树找出每个 scaffold 的检测根，汇总成卡片数据。
// Studio（工作台卡片）与 CleanupPage（清理页左侧脚本列表）共用同一份逻辑，
// 避免两处 DFS 口径漂移。纯计算，不碰渲染。
import type { Node, Scaffold } from '../types';

/** 回退匹配：scaffold 显式声明 `match.name_contains` 时，按名字包含关系 DFS
 *  命中目录（大小写不敏感），命中即下钻。detect 路径扫不到时的兜底。 */
export function fallbackByNameContains(root: Node, sc: Scaffold): Node[] {
  const names = sc.match.name_contains?.map((n) => n.toLowerCase()) ?? [];
  if (names.length === 0) return [];
  const out: Node[] = [];
  const walk = (n: Node, depth: number) => {
    if (!n.is_dir) return;
    if (depth > 0 && names.some((nm) => n.name.toLowerCase().includes(nm))) {
      out.push(n);
      return;
    }
    if (depth < 4) for (const c of n.children) walk(c, depth + 1);
  };
  walk(root, 0);
  return out;
}

/** 一张「脚本卡片」= 一个 scaffold + 它的命中目录 + 合计体量。 */
export interface ScaffoldCard {
  scaffold: Scaffold;
  matches: Node[];
  totalSize: number;
  totalFiles: number;
}

/**
 * 一次 DFS 给所有 scaffold 分组命中：命中后不再下钻（每个目录只算给第一个
 * 匹配的脚本），避免嵌套目录被重复计入。返回按「命中优先 + 体量降序」排的卡片。
 */
export function collectScaffoldCards(root: Node | null, scaffolds: Scaffold[]): ScaffoldCard[] {
  if (!root) return [];

  const byId = new Map<string, Scaffold>();
  const seen = new Set<string>();
  for (const sc of scaffolds) {
    if (!seen.has(sc.id)) {
      seen.add(sc.id);
      byId.set(sc.id, sc);
    }
  }

  const hits = new Map<string, Node[]>();
  const visit = (n: Node) => {
    if (n.is_dir && n.scaffold_id && byId.has(n.scaffold_id)) {
      const list = hits.get(n.scaffold_id) ?? [];
      list.push(n);
      hits.set(n.scaffold_id, list);
      return;
    }
    for (const c of n.children) visit(c);
  };
  visit(root);

  const out: ScaffoldCard[] = [];
  for (const [id, sc] of byId) {
    const detected = hits.get(id) ?? [];
    const matches = detected.length > 0 ? detected : fallbackByNameContains(root, sc);
    matches.sort((a, b) => b.size - a.size);
    out.push({
      scaffold: sc,
      matches,
      totalSize: matches.reduce((s, m) => s + m.size, 0),
      totalFiles: matches.reduce((s, m) => s + m.file_count, 0),
    });
  }

  out.sort((a, b) => {
    const ad = a.matches.length > 0 ? 1 : 0;
    const bd = b.matches.length > 0 ? 1 : 0;
    if (ad !== bd) return bd - ad;
    if (a.totalSize !== b.totalSize) return b.totalSize - a.totalSize;
    return a.scaffold.name.localeCompare(b.scaffold.name);
  });
  return out;
}
