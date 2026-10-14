// ── 基准测试面板（AIDA64 复刻）：CPU / 内存 / 磁盘基准 + 稳定性压测 + 系统体检报告 ──
// 全部走 agent-server 已验收的 MCP 工具（bench_cpu / bench_memory / bench_disk /
// stress_test / system_report / sensor_trend），手动触发 + 结果历史对比（localStorage）。
// stress_test 是写操作（hw.stress 权限），走两步确认；其余只读 L0 直接跑。
import { useState, useRef } from 'react';
import {
  Activity, Copy, Flame, Gauge, History, Loader2, Monitor, ShieldAlert, Square, TrendingUp, X,
} from 'lucide-react';
import { api } from '../../api';
import { isTauri } from '../../env';
import { useT } from '../../i18n';
import { useStore } from '../../store';
import { ErrorBoundary } from '../ErrorBoundary';
import { GpuDonut, DeadPixelOverlay } from './gpu-donut';
import { TrendChart, StressTempChart, StressHistoryChart, ScoreHistoryChart, HeatDistributionBar, DistributionBar, HealthRatingCard, extractLongSeries, parseTrendJson, buildScoreSeries } from './charts';
import type { TrendPoint } from './charts';
import { MEM_RAMP, TEMP_RAMP } from '../../colors';

interface BenchHistoryEntry {
  at: number;
  label: string;
  result: string;
}

/// 各基准的「主分数」行前缀（与后端输出一致，数据不是文案——@i18n-keep）。
/// bench_cpu→整数 ops/s（主指标）；bench_memory→读带宽 MB/s；bench_disk→顺序
/// 读 MB/s（后端 dry-run 计划文本不含这些行，自然不出图）。
const BENCH_SCORE_LINES: Record<string, string> = {
  bench_cpu: '整数（质数筛）', // @i18n-keep
  bench_memory: '读带宽', // @i18n-keep
  bench_disk: '顺序读（128KB 块）', // @i18n-keep
};

/// 从压测结果文本解析「温度曲线：0.0s=52.1°C; …」行 → 实时温度点序列。
/// 后端输出行前缀是中文格式、不是文案，保持原样；解析失败/不足 2 点返回
/// null（调用方不渲染曲线）。导出以便单测锁定格式契约。
export function parseStressCurve(text: string): { sec: number; temp: number }[] | null {
  // 后端输出行前缀，用于匹配、不是文案，故保持原样。
  const line = text.split('\n').find((l) => l.startsWith('温度曲线：')); // @i18n-keep
  if (!line) return null;
  const points: { sec: number; temp: number }[] = [];
  for (const m of line.matchAll(/(\d+(?:\.\d+)?)s=(\d+(?:\.\d+)?)°C/g)) {
    points.push({ sec: parseFloat(m[1]), temp: parseFloat(m[2]) });
  }
  return points.length >= 2 ? points : null;
}

export function BenchPanel() {
  const t = useT();
  const toast = useStore((s) => s.toast);
  // 每个基准的结果与忙碌态（按工具名区分，可并发但 UI 上一次只跑一个更直观）
  const [busy, setBusy] = useState<string | null>(null);
  const [results, setResults] = useState<Record<string, string>>({});
  const [report, setReport] = useState<string | null>(null);
  const [confirmStress, setConfirmStress] = useState(false);
  // 磁盘基准真跑是写操作（会在目标目录写临时文件再删，后端 bench_disk 要求
  // confirmed=true 硬校验）——两步确认 modal 与 stress/donut 同模式。
  const [confirmDiskRun, setConfirmDiskRun] = useState(false);
  // 历史：localStorage 追加式，每类基准独立数组（上限 12 条）
  const [history, setHistory] = useState<Record<string, BenchHistoryEntry[]>>(() => {
    try {
      // 形状校验：JSON.parse 只保证语法合法，不保证值是 { string: Entry[] }。
      // 若 localStorage 被写成 "null" 或 {"cpu":123}，后续
      // `...(prev[key] ?? [])` 对数字展开会抛 TypeError 击穿组件初始化。
      const raw: unknown = JSON.parse(localStorage.getItem('bench.history') ?? '{}');
      if (!raw || typeof raw !== 'object' || Array.isArray(raw)) return {};
      const out: Record<string, BenchHistoryEntry[]> = {};
      for (const [k, v] of Object.entries(raw as Record<string, unknown>)) {
        if (Array.isArray(v) && v.every((e) => e && typeof e === 'object')) {
          out[k] = v as BenchHistoryEntry[];
        }
      }
      return out;
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

  // 压测时长选择：AIDA64 风格可选手动时长（默认 5s 演示，上限 300s）。
  // 压测是同步 call，期间用实时温度监护（下方轮询）而非黑盒转圈。
  const STRESS_SECS_OPTS = [5, 30, 60, 120, 300] as const;
  const [stressSecs, setStressSecs] = useState<number>(5);
  // 实时温度监护：压测进行中并行轮询只读 sensor_trend（L0），画实时 CPU 温度
  // 折线 + 峰值/熔断阈值警戒；失败不拦主流程（监护只是增强，压测本身照常）。
  const [liveTemp, setLiveTemp] = useState<{ points: TrendPoint[]; err: string | null } | null>(null);
  const liveTimer = useRef<number | null>(null);

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

  // 磁盘基准真跑（dry_run=false）：在目标目录（默认系统临时目录）写临时文件测速
  // 后即删。写操作 → confirmed=true（后端硬校验），UI 走两步确认。
  const runDiskReal = async () => {
    setConfirmDiskRun(false);
    await run('bench_disk', t('toolbelt.bench.disk.name'), { dry_run: false, size_mb: 256 }, true);
  };

  const runStress = async () => {
    setConfirmStress(false);
    setStressCurve(null);
    // 压测开始：并行轮询只读 sensor_trend 做实时温度监护（每 2s 一次）。
    // 只读 L0 无需确认；失败只标记监护错误，压测本体照常。
    setLiveTemp({ points: [], err: null });
    const poll = async () => {
      try {
        const r = await api.callTool('sensor_trend', { n: 1, format: 'json' });
        if (r.is_error) { setLiveTemp((prev) => prev ? { ...prev, err: t('toolbelt.bench.stress.liveFail') } : prev); return; }
        const pts = parseTrendJson(r.text);
        if (pts) setLiveTemp((prev) => prev ? { ...prev, points: [...prev.points, ...pts].slice(-60) } : prev);
      } catch { setLiveTemp((prev) => prev ? { ...prev, err: t('toolbelt.bench.stress.liveFail') } : prev); }
    };
    void poll();
    liveTimer.current = window.setInterval(() => void poll(), 2000);
    const text = await run('stress_test', t('toolbelt.bench.stress.name'), { secs: stressSecs, max_temp: 95 }, true);
    if (liveTimer.current !== null) { window.clearInterval(liveTimer.current); liveTimer.current = null; }
    setLiveTemp(null);
    // 从结果解析「温度曲线：0.0s=52.1°C; …」行，渲染压测实时温度折线
    if (text) {
      const points = parseStressCurve(text);
      if (points) setStressCurve(points);
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
  const [trendData, setTrendData] = useState<{ points: TrendPoint[]; range: 'short' | 'long' } | null>(null);
  const [trendError, setTrendError] = useState<string | null>(null);
  // R136：cpu/gpu/mem 三路 series 一次提取（此前评级卡 + 3 个分布条 5 处
  // 各自内联 .map().filter()），五处复用；读不到的字段如实滤掉不造 0。
  const trendSeries = extractLongSeries(trendData?.points ?? []);

  // range='short' 走 sensor_trend（会追加一条新采样，语义是"记录并查看最近 20"）；
  // range='long' 走 sensor_history（R114 纯只读，取已积累的最多 500 条历史，
  // 不追加不裁剪——页面展示不该有写副作用）。
  const loadTrend = async (range: 'short' | 'long' = 'short') => {
    if (busy || trendLoading) return;
    if (!isTauri) {
      setTrendError(t('toolbelt.bench.trend.desktopOnly'));
      return;
    }
    setTrendLoading(true);
    setTrendError(null);
    try {
      const tool = range === 'long' ? 'sensor_history' : 'sensor_trend';
      const n = range === 'long' ? 500 : 20;
      const r = await api.callTool(tool, { n, format: 'json' });
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
      setTrendData({ points: parsed.points, range });
      if (range === 'short') {
        persistHistory('sensor_trend', t('toolbelt.bench.trend.histName'), r.text.slice(0, 400));
      }
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      setTrendError(t('toolbelt.bench.trend.failed', { msg }));
      setTrendData(null);
    } finally {
      setTrendLoading(false);
    }
  };

  const benchCard = (
    key: string,
    title: string,
    desc: string,
    args: Record<string, unknown>,
    btnText: string,
    extraActions?: { label: string; onClick: () => void }[],
  ) => (
    <div className="toolbelt-pane">
      <h3 className="toolbelt-h">{title}</h3>
      <p className="toolbelt-sub">{desc}</p>
      <div className="toolbelt-actions">
        <button className="primary" disabled={busy !== null} onClick={() => run(key, title, args)}>
          {busy === key ? <Loader2 size={14} className="spin" /> : null} {busy === key ? t('toolbelt.bench.running') : btnText}
        </button>
        {extraActions?.map((a) => (
          <button key={a.label} className="ghost" disabled={busy !== null} onClick={a.onClick}>
            {a.label}
          </button>
        ))}
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
      {/* R115：跑分历史对比——历次主分数（旧→新）条形图，峰值用强调色标出 */}
      {BENCH_SCORE_LINES[key] && (
        <ScoreHistoryChart series={buildScoreSeries(historyOf(key), BENCH_SCORE_LINES[key])} />
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
      {benchCard(
        'bench_disk',
        t('toolbelt.bench.disk.title'),
        t('toolbelt.bench.disk.desc'),
        { dry_run: true, size_mb: 32 },
        t('toolbelt.bench.disk.btn'),
        [{ label: t('toolbelt.bench.disk.runBtn'), onClick: () => setConfirmDiskRun(true) }],
      )}
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
              <Flame size={14} /> {t('toolbelt.bench.stress.start', { s: stressSecs })}
            </button>
          )}
          {/* R111：可选手动时长（AIDA64 风格）；压测进行中禁用 */}
          <label className="bench-stress-secs muted small">
            {t('toolbelt.bench.stress.secsLabel')}
            <select value={stressSecs} disabled={busy !== null} onChange={(e) => setStressSecs(Number(e.target.value))}>
              {STRESS_SECS_OPTS.map((s) => <option key={s} value={s}>{s}s</option>)}
            </select>
          </label>
          {historyOf('stress_test').length > 0 && (
            <span className="muted small">{t('toolbelt.bench.lastRuns', { n: historyOf('stress_test').length })}</span>
          )}
        </div>
        {/* R111 实时温度监护：压测进行中并行轮询只读 sensor_trend，画实时曲线 */}
        {busy === 'stress_test' && liveTemp && (
          <div className="bench-stress-live">
            <div className="muted small bench-stress-live-head">
              <Loader2 size={11} className="spin" /> {t('toolbelt.bench.stress.liveTitle')}
              {liveTemp.points.length > 0 && (
                <b className="bench-stress-live-peak">
                  {/* R122：实时监护含 GPU 峰值（老后端无 gpu_temp_c → 只显示 CPU） */}
                  {liveTemp.points.some((p) => typeof p.gpu_temp_c === 'number')
                    ? t('toolbelt.bench.stress.livePeakBoth', {
                        cpu: Math.max(...liveTemp.points.map((p) => p.cpu_temp_c ?? 0)).toFixed(1),
                        gpu: Math.max(...liveTemp.points.map((p) => p.gpu_temp_c ?? 0)).toFixed(1),
                      })
                    : t('toolbelt.bench.stress.livePeak', { temp: Math.max(...liveTemp.points.map((p) => p.cpu_temp_c ?? 0)).toFixed(1) })}
                </b>
              )}
            </div>
            {liveTemp.err && <p className="muted small bench-trend-err">{liveTemp.err}</p>}
            {liveTemp.points.length >= 2 && <TrendChart points={liveTemp.points} />}
            {liveTemp.points.length < 2 && !liveTemp.err && (
              <p className="muted small">{t('toolbelt.bench.stress.liveWaiting')}</p>
            )}
          </div>
        )}
        {results['stress_test'] && <pre className="bench-out">{results['stress_test']}</pre>}
        {stressCurve && stressCurve.length > 1 && (
          <StressTempChart points={stressCurve} />
        )}
        {/* R113：历史温度叠画对比——历次压测曲线同图（最近最亮），峰值/散热演变一眼可见 */}
        {(() => {
          const HISTORY_COLORS = ['#4af0a0', '#4a9af4', '#f2a33c', '#e05252', '#c9a0ff', '#7ee2a8', '#ff6d8f', '#facc15'];
          const hist = historyOf('stress_test').slice().reverse();
          const series = hist
            .map((h, i) => {
              const pts = parseStressCurve(h.result);
              if (!pts) return null;
              const time = new Date(h.at).toLocaleString();
              return { label: time, points: pts, color: HISTORY_COLORS[i % HISTORY_COLORS.length] };
            })
            .filter((s): s is NonNullable<typeof s> => s !== null)
            .slice(0, 8);
          return series.length >= 2 ? <StressHistoryChart series={series} /> : null;
        })()}
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
        {/* R124：甜甜圈 GPU 温度曲线（同 stress_test 契约）+ 历次 GPU 压测叠画 */}
        {donutResult && (() => {
          const curve = parseStressCurve(donutResult);
          return curve ? <StressTempChart points={curve} kind="gpu" /> : null;
        })()}
        {(() => {
          const HISTORY_COLORS = ['#4af0a0', '#4a9af4', '#f2a33c', '#e05252', '#c9a0ff', '#7ee2a8', '#ff6d8f', '#facc15'];
          const hist = historyOf('gpu_donut').slice().reverse();
          const series = hist
            .map((h, i) => {
              const pts = parseStressCurve(h.result);
              if (!pts) return null;
              const time = new Date(h.at).toLocaleString();
              return { label: time, points: pts, color: HISTORY_COLORS[i % HISTORY_COLORS.length] };
            })
            .filter((s): s is NonNullable<typeof s> => s !== null)
            .slice(0, 8);
          return series.length >= 2 ? <StressHistoryChart series={series} kind="gpu" /> : null;
        })()}
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
          <button
            className="primary"
            disabled={busy !== null || trendLoading}
            onClick={() => void loadTrend('short')}
          >
            {trendLoading ? <Loader2 size={14} className="spin" /> : null} {trendLoading ? t('toolbelt.bench.trend.loading') : t('toolbelt.bench.trend.btn')}
          </button>
          {/* R114：长期历史——纯只读 sensor_history，取已积累的最多 500 条 */}
          <button
            className="ghost"
            disabled={busy !== null || trendLoading}
            onClick={() => void loadTrend('long')}
            title={t('toolbelt.bench.trend.longTitle')}
          >
            <History size={13} /> {t('toolbelt.bench.trend.longBtn')}
          </button>
        </div>
        {trendError && <p className="muted small bench-trend-err">{trendError}</p>}
        {trendData && trendData.points.length > 0 && (
          <>
            {trendData.range === 'long' && (
              <div className="bench-trend-err muted small">{t('toolbelt.bench.trend.longNote')}</div>
            )}
            <TrendChart points={trendData.points} />
            {/* R118：长期健康度评级卡——温度/内存高值占比汇总成人话结论。
                R122：GPU 温度并入（可选维度，无数据如实不评级） */}
            {trendData.range === 'long' && (
              <HealthRatingCard
                cpuTemps={trendSeries.cpuTemps}
                gpuTemps={trendSeries.gpuTemps}
                memPercents={trendSeries.memPercents}
              />
            )}
            {/* R116：长期历史的温度分布色阶条——只统计有温度的样本 */}
            {trendData.range === 'long' && (
              <HeatDistributionBar temps={trendSeries.cpuTemps} />
            )}
            {/* R122：长期历史的 GPU 温度分布色阶条——无 GPU 读数时不渲染（如实缺省） */}
            {trendData.range === 'long' && trendSeries.gpuTemps.length > 0 && (
              <DistributionBar values={trendSeries.gpuTemps} ramp={TEMP_RAMP} highThreshold={70} ns="toolbelt.gpuDist" />
            )}
            {/* R117：长期历史的内存占用分布色阶条——回答「内存日常够不够挤」 */}
            {trendData.range === 'long' && (
              <DistributionBar
                values={trendSeries.memPercents}
                ramp={MEM_RAMP}
                highThreshold={85}
                ns="toolbelt.memDist"
              />
            )}
          </>
        )}
        {trendData && trendData.points.length === 0 && (
          <p className="muted small">
            {trendData.range === 'long'
              ? t('toolbelt.bench.trend.longEmpty')
              : t('toolbelt.bench.trend.empty')}
          </p>
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
                {t('toolbelt.bench.stress.confirmA')}{' '}<b>{t('toolbelt.bench.stress.confirmSecs', { s: stressSecs })}</b>{t('toolbelt.bench.stress.confirmB')}{' '}<b className="tb-risk tb-risk-medium">{t('toolbelt.bench.stress.confirmRisk')}</b>{t('toolbelt.bench.stress.confirmC')}
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

      {/* 磁盘基准真跑两步确认（写操作：在目标目录写临时文件测后即删）——
        非 window.confirm，符合破坏性命令 UI 约定 */}
      {confirmDiskRun && (
        <div className="modal-bg" onClick={() => setConfirmDiskRun(false)}>
          <div className="modal confirm-modal" onClick={(e) => e.stopPropagation()}>
            <div className="modal-head">
              <span><ShieldAlert size={14} /> {t('toolbelt.bench.disk.confirmTitle')}</span>
              <button className="ghost icon" onClick={() => setConfirmDiskRun(false)} title={t('toolbelt.common.close')}><X size={15} /></button>
            </div>
            <div className="modal-body">
              <p>
                {t('toolbelt.bench.disk.confirmA')}{' '}<b>{t('toolbelt.bench.disk.confirmSize')}</b>{t('toolbelt.bench.disk.confirmB')}{' '}<b className="tb-risk tb-risk-medium">{t('toolbelt.bench.disk.confirmRisk')}</b>{t('toolbelt.bench.disk.confirmC')}
              </p>
              <p className="muted small">{t('toolbelt.bench.disk.confirmHint')}</p>
            </div>
            <div className="modal-actions">
              <button className="ghost" onClick={() => setConfirmDiskRun(false)}>{t('toolbelt.common.cancel')}</button>
              <button className="primary" onClick={runDiskReal} disabled={busy === 'bench_disk'}>
                {busy === 'bench_disk' ? t('toolbelt.bench.running') : t('toolbelt.bench.disk.confirmBtn')}
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

