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
import { SCRIPT_SEARCH_ALIASES } from '../../cleanup/searchAliases';
import { formatBytes } from '../../format';
import { RiskBadge } from './RiskBadge';
import { CleanupBody } from './CleanupBody';
import { useT } from '../../i18n';

export function CleanupPage({ tagFilter }: { tagFilter?: string }) {
  const t = useT();
  const root = useStore((s) => s.root);
  const scaffolds = useStore((s) => s.scaffolds);
  const session = useCleanupStore((s) => s.session);
  const select = useCleanupStore((s) => s.select);
  // R220：脚本列表本地搜索——输入即过滤，无需后端；空查询显示全部。
  const [query, setQuery] = useState('');

  const allCards = useMemo(() => collectScaffoldCards(root, scaffolds), [root, scaffolds]);
  const cards = useMemo(
    () => (tagFilter ? allCards.filter((c) => c.scaffold.tags?.includes(tagFilter)) : allCards),
    [allCards, tagFilter],
  );
  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return cards;
    const haystack = (c: { scaffold: { id: string; name: string; tags?: string[] } }) => {
      const parts = [c.scaffold.name, ...(c.scaffold.tags ?? []), ...(SCRIPT_SEARCH_ALIASES[c.scaffold.id] ?? [])];
      return parts.join(' ').toLowerCase();
    };
    return cards.filter((c) => haystack(c).includes(q));
  }, [cards, query]);
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
          <div className="cp-list-rows">
            {cards.length === 0 ? (
              <div className="cp-empty">{emptyText}</div>
            ) : filtered.length === 0 ? (
              <div className="cp-empty">{t('cleanup.scriptsSearchEmpty', { q: query.trim() })}</div>
            ) : (
              filtered.map((c) => {
                const active = session?.scaffold.id === c.scaffold.id;
                const detected = c.matches.length > 0;
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
            <CleanupBody />
          ) : (
            <div className="cp-empty">{t('cleanup.pickScript')}</div>
          )}
        </section>
      </div>
    </div>
  );
}
