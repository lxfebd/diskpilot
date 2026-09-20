// ── 服务工具 1/2：网络检测 + 系统信息 ──
// 网络走后端 Rust 真实 TCP 连接（reqwest），不受 WebView CSP connect-src 限制。
// 之前用前端 fetch(mode:'no-cors')，跨域请求被浏览器安全策略拦掉，永远不可达。
import { useCallback, useEffect, useRef, useState } from 'react';
import { Cpu, Globe, Loader2, RefreshCw } from 'lucide-react';
import { api } from '../../api';
import { isTauri } from '../../env';
import { useT } from '../../i18n';
import { formatBytes } from '../../format';
import { useStore } from '../../store';
import { ensureSysProbe } from '../../hwCache';
import type { NetworkProbe, SystemProbe } from '../../types';
import type { ToolContext, TFunc } from './shared';

// 待测站点：name 只是展示文案（走文案表）；查找后端结果用 url
// ——后端 normalize_site 回传的 name 是 host，与展示名对不上。
function pingSites(t: TFunc): { name: string; url: string }[] {
  return [
    { name: t('toolbelt.net.site.baidu'), url: 'https://www.baidu.com' },
    { name: t('toolbelt.net.site.bing'), url: 'https://www.bing.com' },
    { name: t('toolbelt.net.site.github'), url: 'https://github.com' },
  ];
}

export function NetworkPanel({ ctx: _ctx }: { ctx: ToolContext }) {
  const t = useT();
  const toast = useStore((s) => s.toast);
  const [busy, setBusy] = useState(false);
  const [pingResult, setPingResult] = useState<Record<string, NetworkProbe>>({});
  const sites = pingSites(t);

  const runPing = async () => {
    if (!isTauri) {
      toast(t('toolbelt.net.desktopOnly'), 'err');
      return;
    }
    setBusy(true);
    setPingResult({});
    try {
      const res = await api.networkProbe(sites.map((s) => s.url));
      // 后端 normalize_site 返回的 name 是 host（如 www.baidu.com），不是前端
      // 展示名（百度）——按 url 建索引才对得上。
      const map: Record<string, NetworkProbe> = {};
      for (const p of res) map[p.url] = p;
      setPingResult(map);
    } catch (e) {
      toast(String(e instanceof Error ? e.message : e), 'err');
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="toolbelt-pane">
      <h2 className="toolbelt-h"><Globe size={16} /> {t('toolbelt.net.title')}</h2>
      <p className="toolbelt-sub">{t('toolbelt.net.desc')}</p>
      <div className="toolbelt-actions">
        <button className="primary" onClick={runPing} disabled={busy}>
          <RefreshCw size={14} /> {busy ? t('toolbelt.net.testing') : t('toolbelt.net.start')}
        </button>
      </div>
      <div className="tb-ping">
        {sites.map((s) => {
          const p = pingResult[s.url];
          const state = p ? (p.ok ? ' ok' : ' bad') : '';
          const label = !p ? t('toolbelt.net.pending') : p.ok ? t('toolbelt.net.reachable', { ms: p.latency_ms ?? '?' }) : t('toolbelt.net.unreachable');
          return (
            <div key={s.name} className="tb-ping-row">
              <span>{s.name}</span>
              <span className="tb-ping-url">{s.url}</span>
              <span className={'tb-ping-state' + state} title={p?.error ?? ''}>
                {label}
              </span>
            </div>
          );
        })}
      </div>
    </div>
  );
}

export function SysInfoPanel({ ctx: _ctx }: { ctx: ToolContext }) {
  const t = useT();
  const [probe, setProbe] = useState<SystemProbe | null>(null);
  const [loading, setLoading] = useState(true);
  const timerRef = useRef<number | null>(null);

  const refresh = useCallback(() => {
    if (!isTauri) return;
    // ensureSysProbe 带 5 秒 TTL 缓存：5 秒内复用上次结果，不重复触发后端采样
    //（后端改用进程级基线算 CPU 占用，无需每次强制 sleep 双采样）。
    ensureSysProbe(false).then(setProbe).catch(() => setProbe(null)).finally(() => setLoading(false));
  }, []);

  useEffect(() => {
    const visible = () => document.visibilityState === 'visible';
    const startTimer = () => {
      if (timerRef.current !== null) return;
      timerRef.current = window.setInterval(refresh, 5000);
    };
    const stopTimer = () => {
      if (timerRef.current !== null) {
        window.clearInterval(timerRef.current);
        timerRef.current = null;
      }
    };
    // 窗口隐藏（最小化/切后台）时暂停轮询，恢复可见立即刷新——后台零占用
    const onVis = () => {
      if (visible()) {
        refresh();
        startTimer();
      } else {
        stopTimer();
      }
    };
    refresh();
    if (isTauri) {
      startTimer();
      document.addEventListener('visibilitychange', onVis);
    }
    return () => {
      stopTimer();
      document.removeEventListener('visibilitychange', onVis);
    };
  }, [refresh]);

  const ua = typeof navigator !== 'undefined' ? navigator.userAgent : '';
  const plat = ua.match(/Windows NT (\d+\.\d+)/)?.[1] ?? '';
  const winVer =
    plat === '10.0' ? 'Windows 10 / 11' :
    plat === '6.3' ? 'Windows 8.1' :
    plat === '6.2' ? 'Windows 8' :
    plat === '6.1' ? 'Windows 7' :
    plat ? `Windows (NT ${plat})` : 'Windows';
  const cores = probe?.cpu_cores ?? (navigator as { hardwareConcurrency?: number }).hardwareConcurrency ?? t('toolbelt.common.unknown');
  const mem = (navigator as { deviceMemory?: number }).deviceMemory;
  const upDays = probe ? Math.floor(probe.uptime_secs / 86400) : 0;
  const upHrs = probe ? Math.floor((probe.uptime_secs % 86400) / 3600) : 0;

  return (
    <div className="toolbelt-pane">
      <h2 className="toolbelt-h"><Cpu size={16} /> {t('toolbelt.sys.title')}</h2>
      <div className="tb-sysinfo">
        <div className="tb-sys-row"><span>{t('toolbelt.sys.os')}</span><b>{winVer}</b></div>
        <div className="tb-sys-row"><span>{t('toolbelt.sys.cores')}</span><b>{cores}</b></div>
        {probe ? (
          <>
            <div className="tb-sys-row"><span>{t('toolbelt.sys.cpuUsage')}</span><b>{probe.cpu_percent.toFixed(1)}%</b></div>
            <div className="tb-sys-row">
              <span>{t('toolbelt.sys.mem')}</span>
              <b>{t('toolbelt.sys.memUsed', { total: formatBytes(probe.mem_total_bytes), pct: probe.mem_percent.toFixed(0) })}</b>
            </div>
            <div className="tb-sys-row"><span>{t('toolbelt.sys.uptime')}</span><b>{t('toolbelt.sys.uptimeValue', { days: upDays, hrs: upHrs })}</b></div>
          </>
        ) : loading ? (
          <div className="tb-sys-row"><span>{t('toolbelt.sys.loadingLabel')}</span><b className="tb-loading"><Loader2 size={12} className="spin" /> {t('toolbelt.sys.loadingValue')}</b></div>
        ) : (
          <div className="tb-sys-row"><span>{t('toolbelt.sys.mem')}</span><b>{typeof mem === 'number' ? `${mem} GB` : mem ?? t('toolbelt.common.unknown')}</b></div>
        )}
        <div className="tb-sys-row"><span>{t('toolbelt.sys.env')}</span><b>{isTauri ? t('toolbelt.sys.envDesktop') : t('toolbelt.sys.envBrowser')}</b></div>
        <div className="tb-sys-row"><span>{t('toolbelt.sys.ua')}</span><b className="tb-sys-ua">{ua}</b></div>
      </div>
      {isTauri && (
        <p className="hint">
          {t('toolbelt.sys.adminHint')}
        </p>
      )}
    </div>
  );
}

/// Windows 风格切词：双引号内保留空格（与后端 parse_args 同语义）。
/// 导出供 AI 工具层（execCliTool）复用，避免两处切词语义漂移。
export function splitArgs(raw: string): string[] {
  const out: string[] = [];
  let cur = '';
  let inQ = false;
  for (let i = 0; i < raw.length; i++) {
    const c = raw[i];
    if (c === '\\' && raw[i + 1] === '"') { cur += '"'; i++; }
    else if (c === '"') inQ = !inQ;
    else if (/\s/.test(c) && !inQ) { if (cur) { out.push(cur); cur = ''; } }
    else cur += c;
  }
  if (cur || inQ) out.push(cur);
  return out;
}
