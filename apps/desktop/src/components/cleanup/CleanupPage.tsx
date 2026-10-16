// 清理页整页壳：左侧脚本列表（含风险徽标 + 命中体量），右侧主体。
// 命中收集走 cleanup/matches（与 Studio 工作台卡片共用同一份 DFS）。
//
// 从总览/Studio 跳进来时调 useCleanupStore.select(id)；磁盘树换盘或重扫
// 后，下面的 effect 会按新 root 重建会话（rootRef 对比），保证不会
// 拿着旧盘的命中清新盘。
//
// tagFilter（P4-3）：传 scaffold 级 tag（如 "privacy"）时只展示带该 tag 的
// 脚本——隐私清理页复用同一个壳，只换过滤维度；select 仍按 id 全局查，
// 切页后 effect 会因当前会话不在过滤集里而自动改选首个。
import { useEffect, useMemo, useState } from 'react';
import { useStore } from '../../store';
import { useCleanupStore } from '../../useCleanupStore';
import { collectScaffoldCards } from '../../cleanup/matches';
import { formatLastActive } from '../../cleanup/model';
import { SCRIPT_SEARCH_ALIASES } from '../../cleanup/searchAliases';
import { formatBytes } from '../../format';
import { RiskBadge } from './RiskBadge';
import { CleanupBody } from './CleanupBody';
import { api } from '../../api';
import { useT } from '../../i18n';

// R241：台账 source（被清文件/目录绝对路径）→ 归属脚本卡：该路径落在某个
// 卡片任一命中目录之下（或与目录同源）。按目录前缀匹配（与 lastCleanByPath
// 的按目录聚合同思路），路径取小写后规范化反斜杠比较。
export function entriesScaffold(
  source: string,
  cards: { scaffold: { id: string }; matches: { path?: string }[] }[],
): string | null {
  const low = source.replace(/[\\/]+$/, '').toLowerCase();
  for (const c of cards) {
    for (const m of c.matches) {
      const dir = (m.path ?? '').replace(/[\\/]+$/, '').toLowerCase();
      if (dir && (low === dir || low.startsWith(dir + '\\') || low.startsWith(dir + '/'))) return c.scaffold.id;
    }
  }
  return null;
}

export function CleanupPage({ tagFilter, onOpenHistory }: { tagFilter?: string; onOpenHistory?: () => void }) {
  const t = useT();
  const root = useStore((s) => s.root);
  const scaffolds = useStore((s) => s.scaffolds);
  const session = useCleanupStore((s) => s.session);
  const select = useCleanupStore((s) => s.select);
  // R220：脚本列表本地搜索——输入即过滤，无需后端；空查询显示全部。
  const [query, setQuery] = useState('');
  // R221：命中状态过滤 chips（全部 / 已命中 / 未检测到），与搜索叠加。
  type StatusFilter = 'all' | 'hit' | 'miss';
  const [status, setStatus] = useState<StatusFilter>('all');

  const allCards = useMemo(() => collectScaffoldCards(root, scaffolds), [root, scaffolds]);
  const cards = useMemo(
    () => (tagFilter ? allCards.filter((c) => c.scaffold.tags?.includes(tagFilter)) : allCards),
    [allCards, tagFilter],
  );
  // R241：脚本卡「上次清理」——从操作台账聚合每个脚本命中目录的最新清理
  // 时刻（与清理页 scope 行的 lastCleanByPath 同一台账），用户扫一眼列表
  // 就知道哪些脚本刚清过（避免刚清完又清一遍），台账为空/读失败时无徽标。
  const [lastCleanTs, setLastCleanTs] = useState<Record<string, number> | null>(null);
  // R246：清理执行成功后（store.pruneRecycled → bumpScanSeq）重拉台账——
  // 刚清完的脚本卡徽标即时变「今天」，不必刷新页面。scanSeq 与 allCards
  // 任一变化都重算；台账读失败保持 null（诚实）。
  const scanSeq = useStore((s) => s.scanSeq);
  useEffect(() => {
    let cancelled = false;
    api
      .listUndo(200)
      .then((entries) => {
        if (cancelled) return;
        const byScaffold: Record<string, number> = {};
        for (const e of entries) {
          if (!e.source || e.timestamp == null) continue;
          const ts = typeof e.timestamp === 'number' ? e.timestamp : Math.floor(Date.parse(String(e.timestamp)) / 1000);
          if (!Number.isFinite(ts)) continue;
          // 最近一条覆盖：台账按时间追加，后到的 ts 一定更大（同秒取同值无妨）
          const sc = entriesScaffold(e.source, allCards);
          if (sc && (byScaffold[sc] === undefined || ts > byScaffold[sc])) byScaffold[sc] = ts;
        }
        if (!cancelled) setLastCleanTs(byScaffold);
      })
      .catch(() => {
        if (!cancelled) setLastCleanTs(null);
      });
    return () => {
      cancelled = true;
    };
  }, [allCards, scanSeq]);
  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    let out = cards;
    if (status === 'hit') out = out.filter((c) => c.matches.length > 0);
    else if (status === 'miss') out = out.filter((c) => c.matches.length === 0);
    if (!q) return out;
    const haystack = (c: { scaffold: { id: string; name: string; tags?: string[] } }) => {
      const parts = [c.scaffold.name, ...(c.scaffold.tags ?? []), ...(SCRIPT_SEARCH_ALIASES[c.scaffold.id] ?? [])];
      return parts.join(' ').toLowerCase();
    };
    return out.filter((c) => haystack(c).includes(q));
  }, [cards, query, status]);
  // 空态归因：没扫描树（root=null）时脚本无从匹配，引导先去扫描；有扫描树
  // 仍无卡片才是「没装脚本/没有该类脚本」。两种情况文案不同，避免误导。
  const emptyText = root === null ? t('cleanup.needScan') : t('cleanup.noScripts');

  // 进页 / 换盘自动选中第一个命中脚本；正在执行时不打断。
  useEffect(() => {
    const cur = useCleanupStore.getState().session;
    if (cur && (cur.running || cur.previewing)) return;
    if (cards.length === 0) {
      if (cur) select(undefined, { force: true });
      return;
    }
    if (!cur) {
      select(cards[0].scaffold.id);
      return;
    }
    if (cur.rootRef !== root) {
      const stillExists = cards.some((c) => c.scaffold.id === cur.scaffold.id);
      select(stillExists ? cur.scaffold.id : cards[0].scaffold.id, { force: true });
      return;
    }
    if (!cards.some((c) => c.scaffold.id === cur.scaffold.id)) {
      select(cards[0].scaffold.id, { force: true });
    }
  }, [root, cards, select]);

  return (
    <div className="cleanup-page">
      <div className="cp-shell">
        <aside className="cp-list">
          <div className="cp-list-head">
            <span>{t('cleanup.scriptsTitle')}</span>
            <span className="muted small">{t('cleanup.scriptsCount', { n: cards.length })}</span>
          </div>
          <input
            className="cp-search"
            type="search"
            placeholder={t('cleanup.scriptsSearch')}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            aria-label={t('cleanup.scriptsSearch')}
          />
          <div className="cp-status-chips" role="group" aria-label={t('cleanup.scriptsStatus')}>
            {(['all', 'hit', 'miss'] as const).map((st) => (
              <button
                key={st}
                type="button"
                className={'cp-status-chip' + (status === st ? ' active' : '')}
                onClick={() => setStatus(st)}
              >
                {st === 'all' ? t('cleanup.scriptsStatusAll')
                  : st === 'hit' ? t('cleanup.scriptsStatusHit')
                  : t('cleanup.scriptsStatusMiss')}
              </button>
            ))}
          </div>
          <div className="cp-list-rows">
            {cards.length === 0 ? (
              <div className="cp-empty">{emptyText}</div>
            ) : filtered.length === 0 ? (
              <div className="cp-empty">{t('cleanup.scriptsSearchEmpty', { q: query.trim() })}</div>
            ) : (
              filtered.map((c) => {
                const active = session?.scaffold.id === c.scaffold.id;
                const detected = c.matches.length > 0;
                const lastTs = lastCleanTs?.[c.scaffold.id];
                return (
                  <button
                    key={c.scaffold.id}
                    className={`cp-script${active ? ' active' : ''}${detected ? ' detected' : ''}`}
                    onClick={() => select(c.scaffold.id)}
                    disabled={!!session?.running}
                    title={c.scaffold.name}
                  >
                    <span className="cp-script-name">{c.scaffold.name}</span>
                    <RiskBadge risk={c.scaffold.risk} showLabel={false} />
                    <span className="cp-script-size">
                      {detected ? formatBytes(c.totalSize) : t('cleanup.notDetected')}
                    </span>
                    {/* R241：上次清理——台账聚合，刚清过的脚本一眼可见 */}
                    {lastTs !== undefined && (
                      <span className="cp-script-lastclean">{t('cleanup.lastCleaned', { age: formatLastActive(lastTs) })}</span>
                    )}
                  </button>
                );
              })
            )}
          </div>
        </aside>
        <section className="cp-main">
          {session?.blockedProcesses && session.blockedProcesses.length > 0 && (
            <div className="cleanup-preflight-banner" role="status">
              {t('cleanup.preflightRunning', { procs: session.blockedProcesses.join('、') })}
            </div>
          )}
          {cards.length === 0 ? (
            <div className="cp-empty">{emptyText}</div>
          ) : session ? (
            <CleanupBody onOpenHistory={onOpenHistory} />
          ) : (
            <div className="cp-empty">{t('cleanup.pickScript')}</div>
          )}
        </section>
      </div>
    </div>
  );
}
