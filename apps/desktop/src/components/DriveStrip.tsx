import { useEffect, useState } from 'react';
import { Check, HardDrive, RefreshCw, ScanSearch } from 'lucide-react';
import { useStore } from '../store';
import { formatBytes } from '../format';
import { useT } from '../i18n';

interface DriveInfo {
  path: string;
  total_bytes: number;
  used_bytes: number;
  free_bytes: number;
}

function normKey(p: string): string {
  return p.replace(/[\\/]+$/, '').toUpperCase();
}
function driveLetter(p: string): string {
  return p.replace(/\\$/, '').replace(/:$/, '');
}

type Props = {
  drives: DriveInfo[];
  scanning: boolean;
  onScanAll: () => void;
  onScanDrive: (path: string) => void;
  onRefresh: (path: string) => void;
  /** 受控选中（总览页用它同步右侧详情卡片）；不传则内部自管高亮。 */
  selPath?: string | null;
  onSelect?: (path: string) => void;
};

// 顶部硬盘条：一键扫描全部 + 盘卡片横向滚动。总览页与工作台共用，
// 点盘符 = 扫描/秒开该盘，卡片右上角刷新角标 = 强制重扫。
export function DriveStrip({ drives, scanning, onScanAll, onScanDrive, onRefresh, selPath, onSelect }: Props) {
  const t = useT();
  const scanCache = useStore((s) => s.scanCache);
  const [localSel, setLocalSel] = useState<string | null>(null);
  useEffect(() => {
    if (!localSel && drives.length > 0) setLocalSel(drives[0].path);
  }, [drives, localSel]);
  const sel = selPath !== undefined ? selPath : localSel;

  const pick = (p: string) => {
    setLocalSel(p);
    if (onSelect) onSelect(p);
    // 点盘符始终触发扫描/秒开；总览页额外同步右侧详情卡片的选中态。
    onScanDrive(p);
  };

  return (
    <section className="ao-strip">
      <button
        className="ao-strip-all"
        onClick={onScanAll}
        disabled={scanning}
        title={drives.every((d) => !!scanCache[normKey(d.path)]) ? t('shell.drivestrip.allScanned') : t('shell.drivestrip.scanAllTitle')}
      >
        <span className="ao-strip-all-top"><ScanSearch size={16} /> {t('shell.drivestrip.scanAll')}</span>
        <small>{t('shell.drivestrip.scanAllSub')}</small>
      </button>
      {drives.map((d) => {
        const usedPct = d.total_bytes > 0 ? Math.round((d.used_bytes / d.total_bytes) * 100) : 0;
        const scanned = !!scanCache[normKey(d.path)];
        const isSel = sel === d.path;
        return (
          <button
            key={d.path}
            className={'ao-strip-card' + (isSel ? ' sel' : '') + (scanned ? ' scanned' : '')}
            onClick={() => pick(d.path)}
            disabled={scanning}
            title={scanned ? t('shell.drivestrip.openLast', { path: d.path }) : t('shell.drivestrip.scanNow', { path: d.path, free: formatBytes(d.free_bytes) })}
          >
            <span
              className="ao-strip-refresh"
              role="button"
              tabIndex={-1}
              title={t('shell.drivestrip.rescan', { path: d.path })}
              onClick={(e) => { e.stopPropagation(); onRefresh(d.path); }}
            >
              <RefreshCw size={12} />
            </span>
            <span className="ao-strip-top">
              <span className="ao-strip-ic"><HardDrive size={15} /></span>
              <b>{driveLetter(d.path)}:</b>
              {usedPct >= 95 && <span className="ao-strip-danger">{t('shell.drivestrip.lowSpace')}</span>}
              {scanned && <span className="ao-strip-done"><Check size={10} /> {t('shell.drivestrip.scanned')}</span>}
            </span>
            <span className={'ao-strip-bar ' + (usedPct >= 85 ? 'r' : usedPct >= 60 ? 'o' : 'g')}>
              <i style={{ width: `${Math.max(3, Math.min(100, usedPct))}%` }} />
            </span>
            <span className="ao-strip-meta">
              {t('shell.drivestrip.freeUsed', { free: formatBytes(d.free_bytes), used: formatBytes(d.used_bytes) })}
            </span>
          </button>
        );
      })}
    </section>
  );
}
