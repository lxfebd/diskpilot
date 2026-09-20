import { useEffect, useState } from 'react';
import { Check, Trash2, X, ShieldCheck, AlertTriangle, ShieldAlert } from 'lucide-react';
import { useStore, type CleanupProposalItem, type CleanupRisk } from '../store';
import { api } from '../api';
import { formatBytes } from '../format';
import { useT } from '../i18n';

// 风险分级元数据：对标 Dism++ 三级（安全 / 谨慎 / 高危）。文案走 cleanup 表，
// 这里只存键名，渲染时求值（模块顶层求值会把中文烤进首次 import）。
const RISK_META: Record<
  CleanupRisk,
  { labelKey: string; icon: typeof ShieldCheck; cls: string }
> = {
  safe: { labelKey: 'cleanup.riskSafe', icon: ShieldCheck, cls: 'risk-safe' },
  caution: { labelKey: 'cleanup.riskCaution', icon: AlertTriangle, cls: 'risk-caution' },
  danger: { labelKey: 'cleanup.riskDanger', icon: ShieldAlert, cls: 'risk-danger' },
};
const RISK_ORDER: CleanupRisk[] = ['safe', 'caution', 'danger'];

// 高危项解锁口令：参与逐字比较的数据常量，不随语言翻译（提示语里原样显示）。
const DANGER_WORD = '确认'; // @i18n-keep

// AI 清理提案确认弹窗：把 AI 的清理建议逐项展示（是什么 / 干什么用 / 删了
// 影响 + 风险等级 + 实测大小），默认全部不勾选，由用户逐项核对后勾选、高危项
// 还需打字「确认」，最后只有勾选项会经 executeAiPlan（user_confirmed=true）
// 移入回收站。AI 只出清单，动手必须经过这里。
export function CleanupProposalDialog() {
  const t = useT();
  const proposal = useStore((s) => s.proposal);
  const setProposal = useStore((s) => s.setProposal);
  const aiRecyclePaths = useStore((s) => s.aiRecyclePaths);

  const [checked, setChecked] = useState<Set<string>>(new Set());
  // 高危项必须手动输入确认口令才解锁勾选；记录每条高危路径当前输入的文本。
  // 口令是**数据常量**（与 HwPanels 的烤机口令同口径），不进文案表：
  // 翻译表里谁改一个词就会悄悄松开这道安全门，提示语里原样给出该词即可。
  const [dangerInput, setDangerInput] = useState<Record<string, string>>({});
  const [sizes, setSizes] = useState<Record<string, number | null>>({});
  const [measuring, setMeasuring] = useState(true);
  const [busy, setBusy] = useState(false);

  const items = proposal?.items ?? [];

  const dangerOk = (it: CleanupProposalItem) =>
    it.risk !== 'danger' || (dangerInput[it.path] ?? '').trim() === DANGER_WORD;

  // proposal 变化（新清单 / 重新打开）时重置勾选、高危确认与测量状态。
  useEffect(() => {
    if (!proposal) return;
    setChecked(new Set());
    setDangerInput({});
    setSizes({});
    setMeasuring(true);
  }, [proposal]);

  // 逐项实测大小：AI 给的路径可能不准，字节数以执行前实测为准。
  useEffect(() => {
    if (!proposal) return;
    let cancelled = false;
    Promise.all(
      proposal.items.map(async (it) => {
        try {
          const b = await api.estimateSize(it.path);
          return [it.path, b] as const;
        } catch {
          return [it.path, null] as const;
        }
      }),
    ).then((pairs) => {
      if (cancelled) return;
      setSizes(Object.fromEntries(pairs));
      setMeasuring(false);
    });
    return () => {
      cancelled = true;
    };
  }, [proposal]);

  if (!proposal) return null;

  const close = () => setProposal(null);

  const toggle = (it: CleanupProposalItem) => {
    if (!dangerOk(it)) return; // 高危未确认，不允许勾选
    setChecked((prev) => {
      const next = new Set(prev);
      if (next.has(it.path)) next.delete(it.path);
      else next.add(it.path);
      return next;
    });
  };

  const selectSafe = () =>
    setChecked((prev) => {
      const next = new Set(prev);
      for (const it of items) if (it.risk === 'safe') next.add(it.path);
      return next;
    });

  const sizeOf = (it: CleanupProposalItem) => sizes[it.path] ?? it.size_bytes ?? 0;
  const selectedItems = items.filter((it) => checked.has(it.path));
  const selectedBytes = selectedItems.reduce((s, it) => s + sizeOf(it), 0);
  const hasDangerChecked = selectedItems.some((it) => it.risk === 'danger');

  const doRecycle = async () => {
    if (selectedItems.length === 0) return;
    setBusy(true);
    await aiRecyclePaths(
      selectedItems.map((it) => ({ path: it.path, size_hint: sizeOf(it) })),
    );
    setBusy(false);
    close();
  };

  // 按风险分组，空组不渲染。
  const groups = RISK_ORDER.map((risk) => ({
    risk,
    list: items.filter((it) => it.risk === risk),
  })).filter((g) => g.list.length > 0);

  return (
    <div className="modal-bg" onClick={close}>
      <div className="modal proposal-modal" onClick={(e) => e.stopPropagation()}>
        <div className="modal-head">
          <span>
            <Check size={14} /> {t('cleanup.proposalHead', { title: proposal.title })}
          </span>
          <button className="ghost icon" onClick={close} title={t('cleanup.close')} aria-label={t('cleanup.close')}>
            <X size={15} />
          </button>
        </div>
        <div className="proposal-body">
          <div className="proposal-hint">
            {t('cleanup.hintLead')} <b>{items.length}</b> {t('cleanup.hintCountUnit')}
            <b>{t('cleanup.hintNoneChecked')}</b>
            {t('cleanup.hintReview')}
            <b>{t('cleanup.riskDanger')}</b>
            {t('cleanup.hintDanger', { word: DANGER_WORD })}
            {t('cleanup.hintRecycleLead')}
            <b>{t('cleanup.hintRecycleBin')}</b>
            {t('cleanup.hintRecycleTail')}
          </div>
          <div className="proposal-toolbar">
            <button className="ghost" onClick={selectSafe}>
              <ShieldCheck size={12} /> {t('cleanup.selectAllSafe')}
            </button>
            <button className="ghost" onClick={() => setChecked(new Set())}>
              {t('cleanup.clearChecks')}
            </button>
            <span className="muted small">
              {measuring
                ? t('cleanup.measuring')
                : t('cleanup.selectedSummary', { n: selectedItems.length, size: formatBytes(selectedBytes) })}
            </span>
          </div>

          <ul className="proposal-list">
            {groups.map((g) => {
              const meta = RISK_META[g.risk];
              const Icon = meta.icon;
              return (
                <li key={g.risk} className="proposal-group">
                  <div className={`proposal-group-head ${meta.cls}`}>
                    <Icon size={13} /> {t('cleanup.groupHead', { label: t(meta.labelKey), n: g.list.length })}
                  </div>
                  <ul className="proposal-sublist">
                    {g.list.map((it) => {
                      const size = sizeOf(it);
                      const missing = !measuring && size === 0;
                      const itemDangerOk = dangerOk(it);
                      return (
                        <li key={it.path} className={'proposal-item' + (missing ? ' missing' : '')}>
                          <label className="proposal-item-row">
                            <input
                              type="checkbox"
                              checked={checked.has(it.path)}
                              disabled={!itemDangerOk}
                              onChange={() => toggle(it)}
                            />
                            <span className="proposal-path" title={it.path}>
                              {it.path}
                            </span>
                            <span className="proposal-size">
                              {measuring
                                ? t('cleanup.itemMeasuring')
                                : size === 0
                                  ? t('cleanup.pathMissing')
                                  : formatBytes(size)}
                            </span>
                          </label>
                          <div className="proposal-detail">
                            {it.what && (
                              <div className="proposal-detail-line">
                                <b>{t('cleanup.whatLabel')}</b>
                                {it.what}
                              </div>
                            )}
                            {it.purpose && (
                              <div className="proposal-detail-line">
                                <b>{t('cleanup.purposeLabel')}</b>
                                {it.purpose}
                              </div>
                            )}
                            {it.impact && (
                              <div className="proposal-detail-line">
                                <b>{t('cleanup.impactLabel')}</b>
                                {it.impact}
                              </div>
                            )}
                          </div>
                          {it.risk === 'danger' && !itemDangerOk && (
                            <div className="proposal-danger-confirm">
                              <AlertTriangle size={12} />
                              <span>{t('cleanup.dangerLead')}</span>
                              <input
                                className="proposal-danger-input"
                                value={dangerInput[it.path] ?? ''}
                                onChange={(e) =>
                                  setDangerInput((prev) => ({ ...prev, [it.path]: e.target.value }))
                                }
                                placeholder={DANGER_WORD}
                              />
                              <span>{t('cleanup.dangerTail')}</span>
                            </div>
                          )}
                        </li>
                      );
                    })}
                  </ul>
                </li>
              );
            })}
          </ul>
        </div>
        <div className="modal-actions">
          <button className="ghost" onClick={close}>
            {t('cleanup.skipAll')}
          </button>
          <button
            className="primary"
            onClick={doRecycle}
            disabled={busy || measuring || selectedItems.length === 0}
          >
            <Trash2 size={14} />
            {busy
              ? t('cleanup.recycling')
              : t('cleanup.executeSelected', {
                  n: selectedItems.length,
                  size: formatBytes(selectedBytes),
                  danger: hasDangerChecked ? t('cleanup.includesDanger') : '',
                })}
          </button>
        </div>
      </div>
    </div>
  );
}
