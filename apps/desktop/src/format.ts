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
