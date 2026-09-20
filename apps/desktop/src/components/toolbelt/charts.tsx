// ── 轻量 SVG 图表（无第三方图表库）：传感器趋势折线 + 稳定性压测温度折线 ──
import { useT } from '../../i18n';

// ── G15 传感器趋势折线图（轻量 SVG，无第三方图表库）──
export interface TrendPoint {
  ts: number;
  cpu_temp_c?: number | null;
  mem_percent?: number | null;
}

export function TrendChart({ points }: { points: TrendPoint[] }) {
  const t = useT();
  const W = 720;
  const H = 200;
  const PAD = 26;

  const cpuVals = points.map((p) => p.cpu_temp_c).filter((v): v is number => typeof v === 'number');
  const memVals = points.map((p) => p.mem_percent).filter((v): v is number => typeof v === 'number');
  const all = [...cpuVals, ...memVals];
  if (all.length === 0) return <p className="muted small">{t('toolbelt.trend.noData')}</p>;

  const lo = Math.min(...all);
  const hi = Math.max(...all);
  const span = hi - lo || 1;
  const xAt = (i: number, n: number) => PAD + (i / Math.max(n - 1, 1)) * (W - PAD * 2);

  const line = (vals: number[], color: string) => {
    if (vals.length < 2) return null;
    const pts = vals.map((v, i) => `${xAt(i, vals.length).toFixed(1)},${(H - PAD - ((v - lo) / span) * (H - PAD * 2)).toFixed(1)}`).join(' ');
    return <polyline key={color} points={pts} fill="none" stroke={color} strokeWidth={1.8} strokeLinejoin="round" strokeLinecap="round" />;
  };

  // 时间轴刻度：取首/中/尾 ts
  const ticks = points.length >= 3
    ? [points[0], points[Math.floor(points.length / 2)], points[points.length - 1]]
    : points;

  return (
    <div className="bench-trend">
      <svg viewBox={`0 0 ${W} ${H}`} role="img" aria-label={t('toolbelt.trend.aria')} className="bench-trend-svg">
        {[0.25, 0.5, 0.75].map((f) => (
          <line key={f} x1={PAD} x2={W - PAD} y1={PAD + (H - PAD * 2) * f} y2={PAD + (H - PAD * 2) * f} stroke="var(--line)" strokeDasharray="4 4" strokeWidth={1} />
        ))}
        {ticks.map((p, i) => (
          <text key={i} x={xAt(i, ticks.length)} y={H - 8} textAnchor="middle" className="bench-trend-ticks">
            {new Date(p.ts * 1000).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}
          </text>
        ))}
        <text x={PAD} y={12} fill="#888" fontSize={10}>max {hi.toFixed(0)}</text>
        <text x={PAD} y={H - PAD + 12} fill="#888" fontSize={10}>min {lo.toFixed(0)}</text>
        {line(cpuVals, 'var(--accent-strong)')}
        {line(memVals, 'var(--warn)')}
      </svg>
      <div className="bench-trend-legend">
        {cpuVals.length > 0 && <span><i style={{ background: 'var(--accent-strong)' }} /> {t('toolbelt.trend.legendCpu')}</span>}
        {memVals.length > 0 && <span><i style={{ background: 'var(--warn)' }} /> {t('toolbelt.trend.legendMem')}</span>}
        <span className="muted small">{t('toolbelt.trend.samples', { n: points.length })}</span>
      </div>
    </div>
  );
}

// ── 稳定性压测实时温度折线（SST 复刻）：stress_test 输出解析出的每秒 CPU 温度样本 ──
// 轻量 SVG，无第三方图表库；X=压测秒数，Y=温度℃，辅助线展示 压测前/峰值/熔断阈值。
interface StressCurvePoint {
  sec: number;
  temp: number;
}

export function StressTempChart({ points }: { points: StressCurvePoint[] }) {
  const t = useT();
  const W = 720;
  const H = 160;
  const PAD = 30;

  const vals = points.map((p) => p.temp);
  if (vals.length < 2) return null;
  const lo = Math.min(...vals, 0);
  const hi = Math.max(...vals, 100);
  const span = hi - lo || 10;
  const xAt = (i: number) => PAD + (i / Math.max(points.length - 1, 1)) * (W - PAD * 2);
  const yAt = (v: number) => H - PAD - ((v - lo) / span) * (H - PAD * 2);
  const pts = points.map((p, i) => `${xAt(i).toFixed(1)},${yAt(p.temp).toFixed(1)}`).join(' ');

  return (
    <div className="bench-trend" style={{ marginTop: 10 }}>
      <svg viewBox={`0 0 ${W} ${H}`} role="img" aria-label={t('toolbelt.stressChart.aria')} className="bench-trend-svg">
        {[0.25, 0.5, 0.75].map((f) => (
          <line key={f} x1={PAD} x2={W - PAD} y1={PAD + (H - PAD * 2) * f} y2={PAD + (H - PAD * 2) * f} stroke="var(--line)" strokeDasharray="4 4" strokeWidth={1} />
        ))}
        <text x={PAD} y={12} fill="#888" fontSize={10}>max {hi.toFixed(0)}°C</text>
        <text x={PAD} y={H - 8} fill="#888" fontSize={10}>min {lo.toFixed(0)}°C</text>
        <text x={PAD} y={H - PAD + 12} fill="#888" fontSize={10}>0s</text>
        <text x={W - PAD} y={H - 8} textAnchor="end" fill="#888" fontSize={10}>
          {(points[points.length - 1].sec).toFixed(0)}s
        </text>
        <polyline points={pts} fill="none" stroke="var(--accent-strong)" strokeWidth={1.8} strokeLinejoin="round" strokeLinecap="round" />
      </svg>
      <div className="bench-trend-legend">
        <span><i style={{ background: 'var(--accent-strong)' }} /> {t('toolbelt.stressChart.legendCpu')}</span>
        <span className="muted small">{t('toolbelt.stressChart.peak', { peak: hi.toFixed(1), n: points.length })}</span>
      </div>
    </div>
  );
}


