import { useEffect, useState, useCallback } from 'react';
import { Cpu, HardDrive, Gauge, Loader2, ShieldCheck, ShieldAlert, ShieldX, X, Zap, StopCircle, ThermometerSun, Clock, RefreshCw } from 'lucide-react';
import { isTauri } from '../env';
import { formatBytes } from '../format';
import { api } from '../api';
import { LEVEL_COLORS } from '../colors';
import { t, useT } from '../i18n';
import type { HwInfo, HwDiskHealth, HwSnapshotMeta, HwCompareOut } from '../api';

/// 硬件报告 GPU 实时行来源标签：按 `gpu_live.name` 判定厂商（desktop 端 AMD ADL
/// 兜底时 name 以「AMD 显卡（…」开头，NVIDIA 则 nvidia-smi）。无 live → 空串。
/// 返回文案表键对应的译文（切语言即时生效）；工具名本身不译。
export function gpuLiveSource(live?: HwInfo['gpu_live']): string {
  if (!live?.name) return '';
  return t(String(live.name).includes('AMD') ? 'hw.liveSourceAdl' : 'hw.liveSourceNvsmi');
}

// ── 硬件报告卡片 ────────────────────────────────────────────────────────
// AI 调用 get_hardware_info / generate_hw_report 时，把结构化数据挂到
// ChatTurn 上渲染为可视化卡片，不把原始 JSON 塞进正文。

function num(v: unknown): number {
  if (typeof v === 'number') return v;
  if (typeof v === 'string') {
    const n = Number(v.replace(/[^0-9.\-]/g, ''));
    return Number.isFinite(n) ? n : 0;
  }
  return 0;
}

function arr(v: unknown): unknown[] {
  return Array.isArray(v) ? v : [];
}

function pick(o: unknown, k: string): unknown {
  return (o as Record<string, unknown>)?.[k];
}

// 从 GPU 列表里选出"最像物理显卡"的那块：
// 优先 NVIDIA/AMD/Intel 真实 GPU 名，跳过虚拟显示/远程桌面/模拟器驱动。
const VIRTUAL_GPU_HINTS = ['oray', 'idd', 'virtual', 'mumu', 'remote', 'display adapter'];

function pickPhysicalGpu(gpus: unknown[]): Record<string, unknown> | undefined {
  if (!gpus.length) return undefined;
  // 先按名字特征找物理卡
  const byName = gpus.find((g) => {
    const n = String(pick(g, 'Name') ?? '').toLowerCase();
    if (!n) return false;
    if (VIRTUAL_GPU_HINTS.some((v) => n.includes(v))) return false;
    return n.includes('nvidia') || n.includes('radeon') || n.includes('intel') || n.includes('arc');
  });
  if (byName) return byName as Record<string, unknown>;
  // 其余情形回到第一块
  return gpus[0] as Record<string, unknown>;
}

function vramStr(v: unknown): string {
  const n = num(v);
  if (n <= 0) return t('hw.unknown');
  if (n >= 4_294_967_296 || n <= 4_293_918_720) return t('hw.vramOverflow');
  return formatBytes(n);
}

export function HwReportCard({ info, health, loading }: { info: HwInfo | null; health?: HwDiskHealth | null; loading?: boolean }) {
  const t = useT();
  const cpu = arr(info?.cpu)[0] as Record<string, unknown> | undefined;
  const gpu = arr(info?.gpu) as Record<string, unknown>[];
  const physGpu = pickPhysicalGpu(gpu);
  const mems = arr(info?.memory) as Record<string, unknown>[];
  const disks = arr(info?.disk) as Record<string, unknown>[];
  const thermal = arr(info?.thermal) as Record<string, unknown>[];
  const os = arr(info?.os)[0] as Record<string, unknown> | undefined;
  const bios = arr(info?.bios)[0] as Record<string, unknown> | undefined;
  const board = arr(info?.board)[0] as Record<string, unknown> | undefined;

  const totalMem = mems.reduce((s, m) => s + num(pick(m, 'Capacity')), 0);
  const maxSpeed = mems.reduce((s, m) => Math.max(s, num(pick(m, 'Speed'))), 0);
  const maxConf = mems.reduce((s, m) => Math.max(s, num(pick(m, 'ConfiguredClockSpeed'))), 0);
  const xmp = maxConf > 0 && maxConf < maxSpeed ? t('hw.xmpInactive') : maxConf > 0 ? t('hw.xmpActive') : '';

  const maxCpuMhz = num(pick(cpu, 'MaxClockSpeed'));
  const curCpuMhz = num(pick(cpu, 'CurrentClockSpeed'));
  const cores = pick(cpu, 'NumberOfCores');
  const threads = pick(cpu, 'NumberOfLogicalProcessors');
  const cpuLoad = num(pick(cpu, 'LoadPercentage'));
  const cpuUsage = num(pick(arr(info?.cpu_usage)[0], 'PercentProcessorTime'));

  const healthDisks = arr(health?.disks) as Record<string, unknown>[];
  const smartOk = health?.reliability ?? false;

  // 温度传感器最大值
  const maxTemp = thermal.reduce((s, sen) => {
    const raw = num(pick(sen, 'CurrentTemperature'));
    const c = raw / 10 - 273.15;
    return c > s ? c : s;
  }, -Infinity);
  const tempStr = Number.isFinite(maxTemp) ? `${maxTemp.toFixed(0)}℃` : t('hw.notReadable');

  // GPU 实时占用/温度（nvidia-smi / AMD atiadlxx ADL 补充，普通权限可读）
  const live = info?.gpu_live;
  const liveTemp = live ? num(live.temperature_c) : 0;
  const liveUtil = live ? num(live.utilization_pct) : 0;
  const liveSrc = gpuLiveSource(live);

  return (
    <div className="hw-card">
      <div className="hw-head">
        <Cpu size={15} /> {t('hw.reportTitle')}
        {os && <span className="hw-os">{String(pick(os, 'Caption'))}</span>}
      </div>

      {loading && !info && (
        <div className="hw-loading">
          <Loader2 size={14} className="spin" /> {t('hw.reportLoading')}
        </div>
      )}

      {info && (
        <>
      <div className="hw-grid">
        <div className="hw-cell">
          <div className="hw-cell-label">CPU</div>
          <div className="hw-cell-main">{String(cpu ? pick(cpu, 'Name') : t('hw.notReadable'))}</div>
          <div className="hw-cell-sub">
            {t('hw.cpuSpec', { cores: String(cores), threads: String(threads), mhz: maxCpuMhz })}
            {curCpuMhz ? t('hw.nowSuffix', { mhz: curCpuMhz }) : ''}
            {cpuUsage > 0 ? t('hw.loadSuffix', { pct: cpuUsage }) : cpuLoad > 0 ? t('hw.loadSuffix', { pct: cpuLoad }) : ''}
          </div>
        </div>

        <div className="hw-cell">
          <div className="hw-cell-label">{t('hw.cellMem')}</div>
          <div className="hw-cell-main">{formatBytes(totalMem)}</div>
          <div className="hw-cell-sub">
            {t('hw.memSpec', { count: mems.length, mhz: maxSpeed })}{maxConf ? t('hw.nowSuffix', { mhz: maxConf }) : ''} {xmp}
          </div>
        </div>

        <div className="hw-cell">
          <div className="hw-cell-label">{t('hw.cellGpu')}</div>
          <div className="hw-cell-main">{String(physGpu ? pick(physGpu, 'Name') : live?.name ?? t('hw.notReadable'))}</div>
          <div className="hw-cell-sub">
            {physGpu ? vramStr(pick(physGpu, 'AdapterRAM')) : live && live.memory_total_bytes ? formatBytes(num(live.memory_total_bytes)) : ''}
            {gpu.length > 1 ? t('hw.gpuCountSuffix', { count: gpu.length }) : ''}
            {liveUtil > 0 || liveTemp > 0 ? ` · ${liveUtil}% · ${liveTemp}℃` : ''}
          </div>
        </div>

        <div className="hw-cell">
          <div className="hw-cell-label">{t('hw.cellTemp')}</div>
          <div className="hw-cell-main">{liveTemp > 0 ? t('hw.tempGpu', { temp: liveTemp }) : tempStr}</div>
          <div className="hw-cell-sub">{liveTemp > 0 ? liveSrc || t('hw.liveSourceGpu') : t('hw.sensorCount', { count: thermal.length })}</div>
        </div>
      </div>

      {disks.length > 0 && (
        <div className="hw-section">
          <div className="hw-section-title"><HardDrive size={13} /> {t('hw.sectionDisk')}</div>
          <ul className="hw-list">
            {disks.map((d, i) => {
              const size = num(pick(d, 'Size'));
              return (
                <li key={i} className="hw-row">
                  <span className="hw-name">{String(pick(d, 'Model') ?? t('hw.unknown'))}</span>
                  <span className="hw-meta">{formatBytes(size)} · {String(pick(d, 'InterfaceType'))} · {String(pick(d, 'Status'))}</span>
                </li>
              );
            })}
          </ul>
        </div>
      )}
        </>
      )}

      {healthDisks.length > 0 && (
        <div className="hw-section">
          <div className="hw-section-title">
            <ShieldCheck size={13} /> {t('hw.diskHealthTitle', { state: smartOk ? t('hw.smartOk') : t('hw.smartLimited') })}
          </div>
          <ul className="hw-list">
            {healthDisks.map((d, i) => {
              const ph = num(pick(d, 'power_on_hours'));
              const wear = num(pick(d, 'wear_pct'));
              const tc = num(pick(d, 'temperature_c'));
              const tempC = tc > 60 ? (tc - 32) * 5 / 9 : tc;
              const health = String(pick(d, 'health') ?? '');
              // 'Good'/'良好'、'Caution'/'注意' 是后端 WMI 直出的健康原值（中英都认），
              // 只用于比对取图标/配色，属比较常量不译。
              const Icon = health === 'Good' || health === '良好' ? ShieldCheck // @i18n-keep 后端原值比对
                : health === 'Caution' || health === '注意' ? ShieldAlert : ShieldX; // @i18n-keep 后端原值比对
              const color = health === 'Good' || health === '良好' ? LEVEL_COLORS.L0 // @i18n-keep 后端原值比对
                : health === 'Caution' || health === '注意' ? LEVEL_COLORS.L2 : LEVEL_COLORS.L3; // @i18n-keep 后端原值比对
              return (
                <li key={i} className="hw-row">
                  <Icon size={13} style={{ color }} />
                  <span className="hw-name">{String(pick(d, 'friendly') ?? t('hw.unknown'))}</span>
                  <span className="hw-meta">
                    {String(pick(d, 'health'))} · {ph > 0 ? t('hw.powerOnHours', { hours: ph }) : ''}{' '}
                    {wear >= 0 ? t('hw.lifeSuffix', { pct: Math.max(0, 100 - wear) }) : ''}{' '}
                    {tempC > 0 ? `· ${tempC.toFixed(0)}℃` : ''}
                  </span>
                </li>
              );
            })}
          </ul>
        </div>
      )}

      {bios && info && (
        <div className="hw-section">
          <div className="hw-section-title">{t('hw.sectionBios')}</div>
          <div className="hw-row">
            <span className="hw-name">{String(board ? pick(board, 'Product') : t('hw.unknown'))}</span>
            <span className="hw-meta">{String(pick(bios, 'SMBIOSBIOSVersion'))}</span>
          </div>
        </div>
      )}

      <div className="hw-foot">
        <Gauge size={12} /> {t('hw.reportFoot')}
      </div>
    </div>
  );
}

// ── 压测确认面板（L1 受控：必须用户在面板里点「确认」才执行）───────────────

export interface StressConfirm {
  testType: string;
  durationSeconds: number;
  tempLimitCelsius: number;
  label: string;
  tool: string;
}

// 表里存的是**文案键**而不是中文：模块顶层常量若在定义处求值，会把中文烤死在
// 首次 import，切语言不再生效。渲染时再 t()。
const TEST_NOTE_KEYS: Record<string, string> = {
  cpu_stress: 'hw.stressNoteCpu',
  gpu_stress: 'hw.stressNoteGpu',
  gpu_benchmark: 'hw.stressNoteGpuBench',
  cpu_benchmark: 'hw.stressNoteCpuBench',
};

// 确认词是确认门的比较常量（输入必须逐字命中），不译；en 的提示文案里原样给出该词。
const STRESS_CONFIRM_WORD = '确认'; // @i18n-keep 确认门比较常量：输入须逐字命中，改了就把门拆了

export function StressConfirmPanel({ t: stress, onClose, onConfirm }: { t: StressConfirm; onClose: () => void; onConfirm: (r: { run: boolean; remember: boolean }) => void }) {
  const t = useT();
  const [input, setInput] = useState('');
  const [remember, setRemember] = useState(false);
  const ok = input.trim() === STRESS_CONFIRM_WORD;

  return (
    <div className="modal-bg" onClick={onClose}>
      <div className="modal stress-modal" onClick={(e) => e.stopPropagation()}>
        <div className="modal-head">
          <span><Zap size={14} /> {t('hw.stressTitle')}</span>
          <button className="ghost icon" onClick={onClose} title={t('hw.close')} aria-label={t('hw.close')}><X size={15} /></button>
        </div>
        <div className="modal-body">
          <div className="stress-line">
            <b>{t('hw.stressTypeLabel')}</b>{t('hw.stressTypeValue', { label: stress.label, tool: stress.tool })}
          </div>
          <div className="stress-line">
            <b>{t('hw.stressDurationLabel')}</b>{t('hw.stressDurationValue', { sec: stress.durationSeconds })}
          </div>
          <div className="stress-line">
            <b>{t('hw.stressTempLabel')}</b>{t('hw.stressTempValue', { temp: stress.tempLimitCelsius })}
          </div>
          <div className="stress-note">
            <ThermometerSun size={13} /> {t(TEST_NOTE_KEYS[stress.testType] ?? 'hw.stressNoteFallback')}
          </div>
          <div className="stress-warn">
            {t('hw.stressWarn')}
          </div>
          <label className="stress-remember">
            <input type="checkbox" checked={remember} onChange={(e) => setRemember(e.target.checked)} />
            {t('hw.stressRemember')}
          </label>
          <div className="stress-input-row">
            <input
              className="stress-input"
              value={input}
              onChange={(e) => setInput(e.target.value)}
              placeholder={t('hw.stressInputHint')}
              autoFocus
            />
          </div>
        </div>
        <div className="modal-actions">
          <button className="ghost" onClick={onClose}>{t('hw.cancel')}</button>
          <button
            className="primary"
            disabled={!ok}
            onClick={() => onConfirm({ run: ok, remember })}
          >
            <Zap size={13} /> {t('hw.startTest')}
          </button>
        </div>
      </div>
    </div>
  );
}

// ── 实时监控条（压测进行中时浮在 ChatPanel 底部）─────────────────────────

export interface HwProgress {
  test_type: string;
  tool: string;
  elapsed_secs: number;
  limit_secs: number;
  temp_c: number | null;
  peak_c: number | null;
  /** true = 终态事件（后端测试已结束），前端应清除监控条。 */
  done?: boolean;
}

export function HwTestMonitor({
  progress,
  onStop,
  onClose,
}: {
  progress: HwProgress | null;
  onStop: () => void;
  onClose: () => void;
}) {
  const t = useT();
  if (!progress) return null;
  const pct = progress.limit_secs > 0 ? Math.min(100, (progress.elapsed_secs / progress.limit_secs) * 100) : 0;
  const temp = progress.temp_c != null ? `${progress.temp_c.toFixed(0)}℃` : '—';
  const peak = progress.peak_c != null ? t('hw.peakTemp', { temp: progress.peak_c.toFixed(0) }) : t('hw.sensorUnavailable');
  return (
    <div className="hw-monitor">
      <div className="hw-mon-left">
        <Zap size={14} className="hw-mon-spin" />
        <div>
          <div className="hw-mon-title">
            {t('hw.monitorRunning', { type: progress.test_type, tool: progress.tool })}
          </div>
          <div className="hw-mon-time">
            {progress.elapsed_secs}s / {progress.limit_secs}s · {temp} · {peak}
          </div>
        </div>
      </div>
      <div className="hw-mon-bar">
        <div className="hw-mon-fill" style={{ width: `${pct}%` }} />
      </div>
      <button className="ghost icon" onClick={onStop} title={t('hw.stopTest')} aria-label={t('hw.stopTest')}><StopCircle size={15} /></button>
      <button className="ghost icon" onClick={onClose} title={t('hw.collapse')} aria-label={t('hw.collapse')}><X size={14} /></button>
    </div>
  );
}

// ── ChatPanel 侧的事件订阅与控制 ────────────────────────────────────────
// ChatPanel 把本 hook 的返回值渲染为监控条；事件到达时更新 progress，
// 后端进程退出后由调用方置 running=false。

export function useHwTestMonitor(onStop: () => void) {
  const [running, setRunning] = useState(false);
  const [progress, setProgress] = useState<HwProgress | null>(null);

  useEffect(() => {
    if (!isTauri) return;
    let cancelled = false;
    import('@tauri-apps/api/event').then(async (mod) => {
      const unlisten = await mod.listen<HwProgress>('hw-test-progress', (e) => {
        if (cancelled) return;
        // 终态事件（done=true）：测试结束，直接清掉监控条，不再等调用方置 running=false。
        if (e.payload.done) {
          setProgress(null);
          setRunning(false);
          return;
        }
        setProgress(e.payload);
        setRunning(true);
      });
      return () => { unlisten(); };
    });
    return () => { cancelled = true; };
  }, []);

  const stop = () => {
    if (isTauri) api.hwStopTest().catch(() => {});
    onStop();
  };

  const clear = () => {
    setProgress(null);
    setRunning(false);
  };

  return { running, progress, setRunning, stop, clear };
}

// ── 硬件历史快照区（R7）────────────────────────────────────────────
// 每次 hw_report 生成成功都会自动归档到 app_data_dir/hw-history/hw-*.json。
// 这里只读展示归档列表 + 两次归档并排对比温度 / SMART 磨损 / 通电时长。
// 只读查询，绝不触发硬件采集；对比走后端已有归档，不重跑 PowerShell。
export function HwHistorySection() {
  const t = useT();
  const [metas, setMetas] = useState<HwSnapshotMeta[] | null>(null);
  const [compare, setCompare] = useState<HwCompareOut | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [pickA, setPickA] = useState<string>('');
  const [pickB, setPickB] = useState<string>('');

  const runCompare = useCallback(async (a: string, b: string) => {
    setLoading(true);
    setError(null);
    try {
      const out = await api.compareHwSnapshots(a || undefined, b || undefined);
      setCompare(out);
      if (out.ok === false && out.reason) setError(out.reason);
    } catch (e) {
      setError(String(e instanceof Error ? e.message : e));
    } finally {
      setLoading(false);
    }
  }, []);

  const loadHistory = useCallback(async () => {
    if (!isTauri) {
      setMetas([]);
      return;
    }
    try {
      const list = await api.hwHistory();
      setMetas(list);
      // 默认勾选最近两份做对比（后端也会自动取），≥2 份时直接出对比结果
      if (list.length >= 2) {
        const a = list[1].file;
        const b = list[0].file;
        setPickA(a);
        setPickB(b);
        void runCompare(a, b);
      } else if (list.length === 1) {
        setPickA(list[0].file);
        setPickB('');
      }
      setError(null);
    } catch (e) {
      setError(String(e instanceof Error ? e.message : e));
    }
  }, [runCompare]);

  useEffect(() => {
    void loadHistory();
  }, [loadHistory]);

  // 选中两个不同快照后自动触发对比（读 meta 文件名的只读操作）
  const onPick = (which: 'a' | 'b', file: string) => {
    const na = which === 'a' ? file : pickA;
    const nb = which === 'b' ? file : pickB;
    if (which === 'a') setPickA(file);
    else setPickB(file);
    if (na && nb && na !== nb) void runCompare(na, nb);
  };

  if (metas === null) {
    return (
      <div className="hw-hist">
        <div className="hw-hist-title"><Clock size={13} /> {t('hw.histTitle')}</div>
        <div className="hw-hist-empty muted small"><Loader2 size={12} className="spin" /> {t('hw.histLoading')}</div>
      </div>
    );
  }

  if (metas.length === 0) {
    return (
      <div className="hw-hist">
        <div className="hw-hist-title"><Clock size={13} /> {t('hw.histTitle')}</div>
        <div className="hw-hist-empty muted small">
          {t('hw.histEmpty')}
        </div>
      </div>
    );
  }

  return (
    <div className="hw-hist">
      <div className="hw-hist-title">
        <Clock size={13} /> {t('hw.histTitle')}
        <span className="hw-hist-count muted small">{t('hw.histCount', { count: metas.length })}</span>
        <button className="ghost small" onClick={() => void loadHistory()} disabled={loading} title={t('hw.histRefreshTitle')}>
          <RefreshCw size={11} className={loading ? 'spin' : ''} /> {t('hw.refresh')}
        </button>
      </div>

      <div className="hw-hist-pick">
        <label className="hw-hist-pick-col">
          <span className="muted small">{t('hw.histOlder')}</span>
          <select value={pickA} onChange={(e) => onPick('a', e.target.value)}>
            {metas.map((m) => <option key={m.file} value={m.file}>{fmtHwTime(m.at)} · {m.machine}</option>)}
          </select>
        </label>
        <label className="hw-hist-pick-col">
          <span className="muted small">{t('hw.histNewer')}</span>
          <select value={pickB} onChange={(e) => onPick('b', e.target.value)}>
            {metas.map((m) => <option key={m.file} value={m.file}>{fmtHwTime(m.at)} · {m.machine}</option>)}
          </select>
        </label>
      </div>

      {error && <div className="hw-hist-err small">{error}</div>}

      {compare && compare.disks.length > 0 && (
        <div className="hw-hist-compare">
          <div className="hw-hist-compare-head muted small">
            {t('hw.histCompare', { from: fmtHwTime(compare.a) || compare.a, to: fmtHwTime(compare.b) || compare.b })}
          </div>
          {compare.disks.map((d) => (
            <div key={d.device} className="hw-hist-disk">
              <div className="hw-hist-disk-name"><HardDrive size={12} /> {d.device}</div>
              <div className="hw-hist-disk-fields">
                {d.fields.map((f) => (
                  <span key={f.name} className="hw-hist-field">
                    <span className="hw-hist-field-name">{f.name}</span>
                    <span className="hw-hist-field-old">{String(f.old)}</span>
                    <span className="hw-hist-field-arrow">→</span>
                    <span className="hw-hist-field-new">{String(f.new)}</span>
                    <span className={'hw-hist-field-delta' + (String(f.delta).startsWith('+') ? ' up' : '')}>{f.delta}</span>
                  </span>
                ))}
              </div>
            </div>
          ))}
        </div>
      )}

      {compare && compare.disks.length === 0 && !error && (
        <div className="hw-hist-empty muted small">{compare.reason || t('hw.histNoComparable')}</div>
      )}
    </div>
  );
}

export function fmtHwTime(at: number | string): string {
  const n = typeof at === 'number' ? at : Number(at);
  if (!Number.isFinite(n) || n <= 0) return '';
  return new Date(n * 1000).toLocaleString();
}