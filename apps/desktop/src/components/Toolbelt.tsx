// ── 工具墙宿主：服务工具注册 + 视图路由 ──
// 网络检测 / 系统信息 / 硬件信息聚合 / 基准测试 / 插件市场作为工具墙顶部的服务卡片。
// 各面板独立成文件（见 ./toolbelt/），宿主只负责共享上下文与切换视图。
import { useEffect, useMemo, useState } from 'react';
import {
  Activity, ArrowRight, Cpu, Gauge, Globe, LayoutGrid, Package,
} from 'lucide-react';
import { api } from '../api';
import type { ToolbeltCatalog } from '../api';
import { useT } from '../i18n';
import { useStore } from '../store';
import type { SettingsTab } from './Settings';
import { NetworkPanel, SysInfoPanel } from './toolbelt/services';
import { HardwarePanel, CatalogWall } from './toolbelt/toolwall';
import { BenchPanel } from './toolbelt/bench';
import { MarketPanel } from './toolbelt/market';
import type { DriveInfo, ToolContext } from './toolbelt/shared';

type Props = {
  drives: DriveInfo[];
  scanning: boolean;
  onScanDrive: (path: string) => void;
  onScanAll: () => void;
  onGoWorkspace: () => void;
  onOpenSettings?: (tab?: SettingsTab) => void;
};

export function Toolbelt({ drives, scanning, onScanDrive, onScanAll, onGoWorkspace, onOpenSettings }: Props) {
  const t = useT();
  const scanCache = useStore((s) => s.scanCache);
  // view: 'wall' = 主工具墙（默认）；'network' / 'sysinfo' / 'hardware' / 'bench' 为附带的检测工具
  const [view, setView] = useState<'wall' | 'network' | 'sysinfo' | 'hardware' | 'bench' | 'plugins'>('wall');
  const [catalog, setCatalog] = useState<ToolbeltCatalog | null>(null);

  useEffect(() => {
    // 非 Tauri（浏览器预览）走 api 的 mock catalog；Tauri 走真实后端。
    api.toolbeltCatalog().then(setCatalog).catch(() => setCatalog(null));
  }, []);

  const ctx: ToolContext = useMemo(
    () => ({ drives, scanning, scanCache, onScanDrive, onScanAll, onGoWorkspace, onOpenSettings }),
    [drives, scanning, scanCache, onScanDrive, onScanAll, onGoWorkspace, onOpenSettings],
  );

  return (
    <div className="toolbelt">
      {/* 顶部服务工具行：网络 / 系统信息 / 返回工具墙（主角是下面的工具墙） */}
      <div className="toolbelt-services">
        <button
          className={'toolbelt-service' + (view === 'wall' ? ' active' : '')}
          onClick={() => setView('wall')}
          title={t('toolbelt.svc.wallTitle')}
        >
          <LayoutGrid size={16} /> {t('toolbelt.svc.wall')}
          {catalog && <span className="toolbelt-service-count">{catalog.total}</span>}
        </button>
        <button
          className={'toolbelt-service' + (view === 'network' ? ' active' : '')}
          onClick={() => setView('network')}
          title={t('toolbelt.svc.networkTitle')}
        >
          <Globe size={16} /> {t('toolbelt.svc.network')}
        </button>
        <button
          className={'toolbelt-service' + (view === 'sysinfo' ? ' active' : '')}
          onClick={() => setView('sysinfo')}
          title={t('toolbelt.svc.sysinfoTitle')}
        >
          <Cpu size={16} /> {t('toolbelt.svc.sysinfo')}
        </button>
        <button
          className={'toolbelt-service' + (view === 'hardware' ? ' active' : '')}
          onClick={() => setView('hardware')}
          title={t('toolbelt.svc.hardwareTitle')}
        >
          <Activity size={16} /> {t('toolbelt.svc.hardware')}
        </button>
        <button
          className={'toolbelt-service' + (view === 'bench' ? ' active' : '')}
          onClick={() => setView('bench')}
          title={t('toolbelt.svc.benchTitle')}
        >
          <Gauge size={16} /> {t('toolbelt.svc.bench')}
        </button>
        <button
          className={'toolbelt-service' + (view === 'plugins' ? ' active' : '')}
          onClick={() => setView('plugins')}
          title={t('toolbelt.svc.marketTitle')}
        >
          <Package size={16} /> {t('toolbelt.svc.market')}
        </button>
        <div className="grow" />
        <button className="ghost small" onClick={onGoWorkspace} title={t('toolbelt.svc.workspaceTitle')}>
          <ArrowRight size={12} /> {t('toolbelt.svc.workspace')}
        </button>
      </div>

      {view === 'wall' ? (
        <CatalogWall catalog={catalog} onOpenSettings={onOpenSettings} />
      ) : view === 'network' ? (
        <NetworkPanel ctx={ctx} />
      ) : view === 'sysinfo' ? (
        <SysInfoPanel ctx={ctx} />
      ) : view === 'hardware' ? (
        <HardwarePanel ctx={ctx} />
      ) : view === 'bench' ? (
        <BenchPanel />
      ) : (
        <MarketPanel />
      )}
    </div>
  );
}
