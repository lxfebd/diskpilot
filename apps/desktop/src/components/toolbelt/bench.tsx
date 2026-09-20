// ── 基准测试面板（AIDA64 复刻）：CPU / 内存 / 磁盘基准 + 稳定性压测 + 系统体检报告 ──
// 全部走 agent-server 已验收的 MCP 工具（bench_cpu / bench_memory / bench_disk /
// stress_test / system_report / sensor_trend），手动触发 + 结果历史对比（localStorage）。
// stress_test 是写操作（hw.stress 权限），走两步确认；其余只读 L0 直接跑。
import { useState } from 'react';
import {
  Activity, Copy, Flame, Gauge, Loader2, Monitor, ShieldAlert, Square, TrendingUp, X,
} from 'lucide-react';
import { api } from '../../api';
import { isTauri } from '../../env';
import { useT } from '../../i18n';
import { useStore } from '../../store';
import { ErrorBoundary } from '../ErrorBoundary';
import { GpuDonut, DeadPixelOverlay } from './gpu-donut';
import { TrendChart, StressTempChart } from './charts';
import type { TrendPoint } from './charts';

interface BenchHistoryEntry {
  at: number;
  label: string;
  result: string;
}

export function BenchPanel() {
  const t = useT();
  const toast = useStore((s) => s.toast);
  // 每个基准的结果与忙碌态（按工具名区分，可并发但 UI 上一次只跑一个更直观）
  const [busy, setBusy] = useState<string | null>(null);
  const [results, setResults] = useState<Record<string, string>>({});
  const [report, setReport] = useState<string | null>(null);
  const [confirmStress, setConfirmStress] = useState(false);
  // 历史：localStorage 追加式，每类基准独立数组（上限 12 条）
  const [history, setHistory] = useState<Record<string, BenchHistoryEntry[]>>(() => {
    try {
      return JSON.parse(localStorage.getItem('bench.history') ?? '{}');
    } catch {
      return {};
    }
  });
  // 压测温度曲线：stress_test 输出「温度曲线：0.0s=52.1°C; …」解析出的每秒样本
  const [stressCurve, setStressCurve] = useState<{ sec: number; temp: number }[] | null>(null);

  const persistHistory = (key: string, label: string, result: string) => {
    setHistory((prev) => {
      const next = { ...prev, [key]: [...(prev[key] ?? []), { at: Date.now(), label, result }].slice(-12) };
      try {
        localStorage.setItem('bench.history', JSON.stringify(next));
      } catch {
        /* 配额满静默 */
      }
      return next;
    });
  };

  const run = async (key: string, label: string, args: Record<string, unknown>, confirmed = false): Promise<string | null> => {
    if (busy) return null;
    if (!isTauri) {
      toast(t('toolbelt.bench.desktopOnly'), 'err');
      return null;
    }
    setBusy(key);
    setResults((r) => ({ ...r, [key]: t('toolbelt.bench.running') }));
    try {
      const r = await api.callTool(key, args, confirmed ? true : undefined);
      const text = r.is_error ? t('toolbelt.bench.rejected', { text: r.text }) : r.text;
      setResults((prev) => ({ ...prev, [key]: text }));
      persistHistory(key, label, text);
      return text;
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      setResults((prev) => ({ ...prev, [key]: t('toolbelt.bench.callFailed', { msg }) }));
      toast(t('toolbelt.bench.callFailedToast', { msg }), 'err');
      return null;
    } finally {
      setBusy(null);
    }
  };

  // 停止正在跑的压测（stress_test / stress_test_gpu）：agent.rs 每次调用重新
  // spawn agent-server 进程，压测与停止按钮是两个进程实例；stress_cancel 写
  // 跨进程停止文件（%TEMP%/diskpilot-stress-stop.flag），压测监控循环检测到
  // 即退出——跨进程天然有效。
  const stopStress = () => {
    if (busy !== 'stress_test') return;
    void api.callTool('stress_cancel', {}).then((r) => {
      toast(r.is_error ? t('toolbelt.bench.stopFailed', { text: r.text }) : t('toolbelt.bench.stopSent'));
    }).catch((e) => toast(t('toolbelt.bench.stopCallFailed', { msg: String(e) }), 'err'));
  };

  const runStress = async () => {
    setConfirmStress(false);
    setStressCurve(null);
    const text = await run('stress_test', t('toolbelt.bench.stress.name'), { secs: 5, max_temp: 95 }, true);
    // 从结果解析「温度曲线：0.0s=52.1°C; …」行，渲染压测实时温度折线
    if (text) {
      // 后端输出行前缀，用于匹配、不是文案，故保持原样。
      const line = text.split('\n').find((l) => l.startsWith('温度曲线：')); // @i18n-keep
      if (line) {
        const points: { sec: number; temp: number }[] = [];
        for (const m of line.matchAll(/(\d+(?:\.\d+)?)s=(\d+(?:\.\d+)?)°C/g)) {
          points.push({ sec: parseFloat(m[1]), temp: parseFloat(m[2]) });
        }
        if (points.length >= 2) setStressCurve(points);
      }
    }
  };

  // GPU 甜甜圈压测（WebGL 全屏渲染，压真实 GPU）：确认态 + 运行态
  const [confirmDonut, setConfirmDonut] = useState(false);
  const [donutOn, setDonutOn] = useState(false);
  const [donutResult, setDonutResult] = useState<string | null>(null);

  const runDonut = () => {
    setConfirmDonut(false);
    if (!isTauri) {
      toast(t('toolbelt.bench.donut.desktopOnly'), 'err');
      return;
    }
    setDonutResult(null);
    setDonutOn(true);
  };

  // 甜甜圈结束回调（组件内部调用）：记录平均 FPS/时长
  const onDonutDone = (summary: string, canceled: boolean) => {
    setDonutOn(false);
    const txt = canceled ? `${t('toolbelt.bench.donut.manualTag')}\n${summary}` : summary;
    setDonutResult(txt);
    persistHistory('gpu_donut', canceled ? t('toolbelt.bench.donut.histManual') : t('toolbelt.bench.donut.histName'), txt);
    if (canceled) toast(t('toolbelt.bench.donut.exited'));
  };

  const runReport = async () => {
    if (busy) return;
    if (!isTauri) {
      toast(t('toolbelt.bench.report.desktopOnly'), 'err');
      return;
    }
    setBusy('system_report');
    setReport(t('toolbelt.bench.report.generating'));
    try {
      const r = await api.callTool('system_report', {});
      setReport(r.is_error ? t('toolbelt.bench.rejected', { text: r.text }) : r.text);
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      setReport(t('toolbelt.bench.report.failed', { msg }));
      toast(t('toolbelt.bench.report.failedToast', { msg }), 'err');
    } finally {
      setBusy(null);
    }
  };

  const copyReport = async () => {
    if (!report) return;
    try {
      await navigator.clipboard.writeText(report);
      toast(t('toolbelt.bench.report.copied'));
    } catch {
      toast(t('toolbelt.bench.report.copyFailed'), 'err');
    }
  };

  const historyOf = (key: string) => history[key] ?? [];

  // ── G13 坏点检测：全屏色块覆盖层开关 ──
  const [deadPixelOn, setDeadPixelOn] = useState(false);

  // ── G15 传感器趋势：拉取 sensor_trend format=json 渲染折线图 ──
  const [trendLoading, setTrendLoading] = useState(false);
  const [trendData, setTrendData] = useState<{ points: TrendPoint[] } | null>(null);
  const [trendError, setTrendError] = useState<string | null>(null);

  const loadTrend = async () => {
    if (busy || trendLoading) return;
    if (!isTauri) {
      setTrendError(t('toolbelt.bench.trend.desktopOnly'));
      return;
    }
    setTrendLoading(true);
    setTrendError(null);
    try {
      const r = await api.callTool('sensor_trend', { n: 20, format: 'json' });
      if (r.is_error) {
        setTrendError(t('toolbelt.bench.rejected', { text: r.text }));
        setTrendData(null);
        return;
      }
      let parsed: { points?: TrendPoint[] } | null = null;
      try {
        parsed = JSON.parse(r.text);
      } catch {
        parsed = null;
      }
      if (!parsed || !Array.isArray(parsed.points)) {
        // 兜底：后端不支持 json 时退回文本展示
        setTrendError(t('toolbelt.bench.trend.noJson'));
        setTrendData(null);
        return;
      }
      setTrendData({ points: parsed.points });
      persistHistory('sensor_trend', t('toolbelt.bench.trend.histName'), r.text.slice(0, 400));
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      setTrendError(t('toolbelt.bench.trend.failed', { msg }));
      setTrendData(null);
    } finally {
      setTrendLoading(false);
    }
  };

  const benchCard = (key: string, title: string, desc: string, args: Record<string, unknown>, btnText: string) => (
    <div className="toolbelt-pane">
      <h3 className="toolbelt-h">{title}</h3>
      <p className="toolbelt-sub">{desc}</p>
      <div className="toolbelt-actions">
        <button className="primary" disabled={busy !== null} onClick={() => run(key, title, args)}>
          {busy === key ? <Loader2 size={14} className="spin" /> : null} {busy === key ? t('toolbelt.bench.running') : btnText}
        </button>
        {historyOf(key).length > 0 && (
          <span className="muted small">{t('toolbelt.bench.lastRuns', { n: historyOf(key).length })}</span>
        )}
      </div>
      {results[key] && (
        <pre className="bench-out">{results[key]}</pre>
      )}
      {historyOf(key).length > 1 && (
        <div className="bench-history">
          {historyOf(key).slice().reverse().slice(1).map((h, i) => (
            <details key={i}>
              <summary className="muted small">
                {h.label} · {new Date(h.at).toLocaleString()}
              </summary>
              <pre className="bench-out">{h.result}</pre>
            </details>
          ))}
        </div>
      )}
    </div>
  );

  return (
    <div className="toolbelt-main">
      <div className="toolbelt-pane">
        <h2 className="toolbelt-h"><Gauge size={16} /> {t('toolbelt.bench.title')}</h2>
        <p className="toolbelt-sub">{t('toolbelt.bench.intro')}</p>
      </div>

      {benchCard('bench_cpu', t('toolbelt.bench.cpu.title'), t('toolbelt.bench.cpu.desc'), { secs: 2 }, t('toolbelt.bench.cpu.btn'))}
      {benchCard('bench_memory', t('toolbelt.bench.mem.title'), t('toolbelt.bench.mem.desc'), { secs: 2 }, t('toolbelt.bench.mem.btn'))}
      {benchCard('bench_disk', t('toolbelt.bench.disk.title'), t('toolbelt.bench.disk.desc'), { dry_run: true, size_mb: 32 }, t('toolbelt.bench.disk.btn'))}
      {benchCard('bench_gpu', t('toolbelt.bench.gpuInfo.title'), t('toolbelt.bench.gpuInfo.desc'), {}, t('toolbelt.bench.gpuInfo.btn'))}

      <div className="toolbelt-pane">
        <h3 className="toolbelt-h"><Flame size={16} /> {t('toolbelt.bench.stress.title')}</h3>
        <p className="toolbelt-sub">{t('toolbelt.bench.stress.desc')}</p>
        <div className="toolbelt-actions">
          {busy === 'stress_test' ? (
            <button className="primary stop" onClick={stopStress} title={t('toolbelt.bench.stress.stopTitle')}>
              <Square size={14} /> {t('toolbelt.bench.stress.stop')}
            </button>
          ) : (
            <button className="primary" disabled={busy !== null} onClick={() => setConfirmStress(true)}>
              <Flame size={14} /> {t('toolbelt.bench.stress.start')}
            </button>
          )}
          {historyOf('stress_test').length > 0 && (
            <span className="muted small">{t('toolbelt.bench.lastRuns', { n: historyOf('stress_test').length })}</span>
          )}
        </div>
        {results['stress_test'] && <pre className="bench-out">{results['stress_test']}</pre>}
        {stressCurve && stressCurve.length > 1 && (
          <StressTempChart points={stressCurve} />
        )}
        {historyOf('stress_test').length > 1 && (
          <div className="bench-history">
            {historyOf('stress_test').slice().reverse().slice(1).map((h, i) => (
              <details key={i}>
                <summary className="muted small">{h.label} · {new Date(h.at).toLocaleString()}</summary>
                <pre className="bench-out">{h.result}</pre>
              </details>
            ))}
          </div>
        )}
      </div>

      <div className="toolbelt-pane">
        <h3 className="toolbelt-h"><Flame size={16} /> {t('toolbelt.bench.donut.title')}</h3>
        <p className="toolbelt-sub">
          {t('toolbelt.bench.donut.descA')}{' '}<b>{t('toolbelt.bench.donut.descHot')}</b>{' '}{t('toolbelt.bench.donut.descB')}
        </p>
        <div className="toolbelt-actions">
          <button className="primary" disabled={busy !== null} onClick={() => setConfirmDonut(true)}>
            <Flame size={14} /> {t('toolbelt.bench.donut.start')}
          </button>
          {historyOf('gpu_donut').length > 0 && (
            <span className="muted small">{t('toolbelt.bench.lastRuns', { n: historyOf('gpu_donut').length })}</span>
          )}
        </div>
        {donutResult && <pre className="bench-out">{donutResult}</pre>}
      </div>

      <div className="toolbelt-pane">
        <h3 className="toolbelt-h"><Monitor size={16} /> {t('toolbelt.bench.dead.title')}</h3>
        <p className="toolbelt-sub">{t('toolbelt.bench.dead.desc')}</p>
        <div className="toolbelt-actions">
          <button className="primary" onClick={() => setDeadPixelOn(true)}>
            <Monitor size={14} /> {t('toolbelt.bench.dead.start')}
          </button>
        </div>
      </div>

      <div className="toolbelt-pane">
        <h3 className="toolbelt-h"><TrendingUp size={16} /> {t('toolbelt.bench.trend.title')}</h3>
        <p className="toolbelt-sub">
          {t('toolbelt.bench.trend.descA')}{' '}<code>sensor_trend</code>{t('toolbelt.bench.trend.descB')}
        </p>
        <div className="toolbelt-actions">
          <button className="primary" disabled={busy !== null || trendLoading} onClick={loadTrend}>
            {trendLoading ? <Loader2 size={14} className="spin" /> : null} {trendLoading ? t('toolbelt.bench.trend.loading') : t('toolbelt.bench.trend.btn')}
          </button>
        </div>
        {trendError && <p className="muted small bench-trend-err">{trendError}</p>}
        {trendData && trendData.points.length > 0 && (
          <TrendChart points={trendData.points} />
        )}
        {trendData && trendData.points.length === 0 && (
          <p className="muted small">{t('toolbelt.bench.trend.empty')}</p>
        )}
      </div>

      <div className="toolbelt-pane">
        <h3 className="toolbelt-h"><Activity size={16} /> {t('toolbelt.bench.report.title')}</h3>
        <p className="toolbelt-sub">{t('toolbelt.bench.report.desc')}</p>
        <div className="toolbelt-actions">
          <button className="primary" disabled={busy !== null} onClick={runReport}>
            {busy === 'system_report' ? <Loader2 size={14} className="spin" /> : null} {busy === 'system_report' ? t('toolbelt.bench.report.generating') : t('toolbelt.bench.report.run')}
          </button>
          {report && <button className="ghost" onClick={copyReport}><Copy size={14} /> {t('toolbelt.bench.report.copy')}</button>}
        </div>
        {report && <pre className="bench-out">{report}</pre>}
      </div>

      {/* 压测两步确认（写操作）——非 window.confirm，符合破坏性命令 UI 约定 */}
      {confirmStress && (
        <div className="modal-bg" onClick={() => setConfirmStress(false)}>
          <div className="modal confirm-modal" onClick={(e) => e.stopPropagation()}>
            <div className="modal-head">
              <span><ShieldAlert size={14} /> {t('toolbelt.bench.stress.confirmTitle')}</span>
              <button className="ghost icon" onClick={() => setConfirmStress(false)} title={t('toolbelt.common.close')}><X size={15} /></button>
            </div>
            <div className="modal-body">
              <p>
                {t('toolbelt.bench.stress.confirmA')}{' '}<b>{t('toolbelt.bench.stress.confirmSecs')}</b>{t('toolbelt.bench.stress.confirmB')}{' '}<b className="tb-risk tb-risk-medium">{t('toolbelt.bench.stress.confirmRisk')}</b>{t('toolbelt.bench.stress.confirmC')}
              </p>
              <p className="muted small">{t('toolbelt.bench.stress.confirmHint')}</p>
            </div>
            <div className="modal-actions">
              <button className="ghost" onClick={() => setConfirmStress(false)}>{t('toolbelt.common.cancel')}</button>
              <button className="primary" onClick={runStress} disabled={busy === 'stress_test'}>
                {busy === 'stress_test' ? t('toolbelt.bench.stress.running') : t('toolbelt.bench.stress.confirmBtn')}
              </button>
            </div>
          </div>
        </div>
      )}

      {/* G13 显示器坏点检测：全屏纯色覆盖层，点击/按键切换颜色，Esc/双击退出 */}
      {deadPixelOn && <DeadPixelOverlay onExit={() => setDeadPixelOn(false)} />}

      {/* GPU 甜甜圈压测两步确认（写操作）——非 window.confirm，符合破坏性命令 UI 约定 */}
      {confirmDonut && (
        <div className="modal-bg" onClick={() => setConfirmDonut(false)}>
          <div className="modal confirm-modal" onClick={(e) => e.stopPropagation()}>
            <div className="modal-head">
              <span><ShieldAlert size={14} /> {t('toolbelt.bench.donut.confirmTitle')}</span>
              <button className="ghost icon" onClick={() => setConfirmDonut(false)} title={t('toolbelt.common.close')}><X size={15} /></button>
            </div>
            <div className="modal-body">
              <p>
                {t('toolbelt.bench.donut.confirmA')}{' '}<b>{t('toolbelt.bench.donut.confirmMode')}</b>{t('toolbelt.bench.donut.confirmB')}{' '}<b>{t('toolbelt.bench.donut.confirmEsc')}</b>{' '}{t('toolbelt.bench.donut.confirmC')}{' '}<b className="tb-risk tb-risk-medium">{t('toolbelt.bench.donut.confirmRisk')}</b>{t('toolbelt.bench.donut.confirmD')}
              </p>
              <p className="muted small">
                {t('toolbelt.bench.donut.guardA')}{' '}<code>hw_temperature</code>{' '}{t('toolbelt.bench.donut.guardB')}{' '}<b>{t('toolbelt.bench.donut.guardHot')}</b>{t('toolbelt.bench.donut.guardC')}
              </p>
            </div>
            <div className="modal-actions">
              <button className="ghost" onClick={() => setConfirmDonut(false)}>{t('toolbelt.common.cancel')}</button>
              <button className="primary" onClick={runDonut}>{t('toolbelt.bench.donut.confirmBtn')}</button>
            </div>
          </div>
        </div>
      )}

      {/* GPU 甜甜圈压测全屏渲染层（压真实 GPU） */}
      {donutOn && (
        <ErrorBoundary fallbackLabel={t('toolbelt.bench.donut.crashFallback')}>
          <GpuDonut onDone={onDonutDone} />
        </ErrorBoundary>
      )}
    </div>
  );
}

