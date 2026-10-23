export function formatBytes(n: number): string {
  if (Math.abs(n) < 1024) return `${n} B`;
  const units = ['KB', 'MB', 'GB', 'TB', 'PB'];
  const sign = n < 0 ? '-' : '';
  let v = Math.abs(n) / 1024;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${sign}${v.toFixed(v >= 100 ? 0 : v >= 10 ? 1 : 2)} ${units[i]}`;
}

// 容量三元组：总容量/已用/可用 用同一单位同一精度，且总量显示值 = 已用 + 可用，
// 保证三行明细相加永远自洽（避免出现 299 ≠ 295 + 3.73 的质疑）
export function formatBytesTriple(total: number, used: number, free: number): [string, string, string] {
  if (total < 1024) return [`${total} B`, `${used} B`, `${free} B`];
  const units = ['KB', 'MB', 'GB', 'TB', 'PB'];
  let div = 1024;
  let i = 0;
  while (total / div >= 1024 && i < units.length - 1) {
    div *= 1024;
    i++;
  }
  const v = total / div;
  const digits = v >= 10 ? 1 : 2;
  const usedS = (used / div).toFixed(digits);
  const freeS = (free / div).toFixed(digits);
  const totalS = (Number(usedS) + Number(freeS)).toFixed(digits);
  return [`${totalS} ${units[i]}`, `${usedS} ${units[i]}`, `${freeS} ${units[i]}`];
}

export function formatCount(n: number): string {
  if (n < 1000) return String(n);
  if (n < 1_000_000) return `${(n / 1000).toFixed(1)}k`;
  return `${(n / 1_000_000).toFixed(1)}M`;
}

/** 路径中间截断：保留盘符/开头 + 末尾文件名，中间用 … 折叠，maxLen 内放下。 */
export function ellipsizePath(p: string, maxLen = 48): string {
  if (p.length <= maxLen) return p;
  const headLen = Math.floor(maxLen * 0.4);
  const tailLen = maxLen - headLen - 1; // 1 个 … 占位
  return `${p.slice(0, headLen)}…${p.slice(-tailLen)}`;
}

/** 路径归一化：去掉尾部分隔符、统一大写，用作按盘符/路径做 Map key。 */
export function normKey(p: string): string {
  return p.replace(/[\\/]+$/, '').toUpperCase();
}

/** 从盘符路径取盘号字母，如 `C:\` → `C`、`C:\Users` → `C`。 */
export function driveLetter(p: string): string {
  return p.replace(/\\$/, '').replace(/:$/, '');
}

/** R248：从任意路径取「盘根」（第一个分隔符之后）：`C:\Users\AppData` → `C:\`；
 *  `C:\` 本身不变；UNC（`\\server\share`）取到服务器根 `\\server\`。 */
export function driveRootOf(p: string): string {
  if (p.startsWith('\\\\')) {
    // UNC：开头就是分隔符，取到第二个分隔符之后（服务器名后）
    const second = p.indexOf('\\', 2);
    return second >= 0 ? p.slice(0, second + 1) : p;
  }
  const sep = p.indexOf('\\');
  if (sep >= 0) return p.slice(0, sep + 1);
  return p;
}

/** Date → `YYYY-MM-DD`（地区中立）。用于按天分组/展示日期，与 `toLocaleDateString` 时区语义一致。 */
export function formatDateKey(d: Date): string {
  if (Number.isNaN(d.getTime())) return '';
  const pad = (n: number) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}

/**
 * Date → `YYYY-MM-DD HH:MM`（地区中立，可排序）。用于报表/审计等要落文本的场合。
 * R324：此前 Steam 报表里写死带地区参数的 toLocaleString 调用——英文界面也会
 * 按中文地区格式渲染，且依赖运行时地区。收口到这里，口径与 formatDateKey 一致。
 */
export function formatDateTime(d: Date, withSeconds = false): string {
  if (Number.isNaN(d.getTime())) return '';
  const pad = (n: number) => String(n).padStart(2, '0');
  const date = `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
  const time = `${pad(d.getHours())}:${pad(d.getMinutes())}` + (withSeconds ? `:${pad(d.getSeconds())}` : '');
  return `${date} ${time}`;
}
/** Date → `HH:MM` 或 `HH:MM:SS`（地区中立）。R329：此前多处用 toLocaleTimeString，
 *  输出跟随系统地区（英文界面在中文系统上会渲染成中文时段），收口到与 formatDateTime 同口径。 */
export function formatTime(d: Date, withSeconds = false): string {
  if (Number.isNaN(d.getTime())) return '';
  const pad = (n: number) => String(n).padStart(2, '0');
  return `${pad(d.getHours())}:${pad(d.getMinutes())}` + (withSeconds ? `:${pad(d.getSeconds())}` : '');
}

/** Date → `MM-DD`（地区中立）。用于图表横轴这类只放得下月日的刻度。 */
export function formatMonthDay(d: Date): string {
  if (Number.isNaN(d.getTime())) return '';
  const pad = (n: number) => String(n).padStart(2, '0');
  return `${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}

/**
 * 取文件名扩展名（小写，不含点）。无扩展名返回 `''`。
 * FileView/TreeView 各有一份私有实现，收敛到此单源；FileView 的
 * `'(none)'` 分组哨兵是展示层语义，由调用方自行映射。
 */
export function extOf(name: string): string {
  const i = name.lastIndexOf('.');
  return i > 0 ? name.slice(i + 1).toLowerCase() : '';
}

