/** 体积格式化：字节 → 可读字符串。 */
export function formatBytes(bytes: number, decimals = 1): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  const i = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
  const value = bytes / 1024 ** i;
  return `${value.toFixed(i === 0 ? 0 : decimals)} ${units[i]}`;
}

/** 秒 → 时分秒。 */
export function formatDuration(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds < 0) return '00:00';
  const total = Math.floor(seconds);
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  const pad = (n: number) => String(n).padStart(2, '0');
  return h > 0 ? `${h}:${pad(m)}:${pad(s)}` : `${pad(m)}:${pad(s)}`;
}

/** 播放量人性化：1.2亿 / 3456万 / 8921。 */
export function formatPlayCount(n: number): string {
  if (n >= 100_000_000) return `${(n / 100_000_000).toFixed(1)}亿`;
  if (n >= 10_000) return `${Math.round(n / 10_000)}万`;
  return String(n);
}

/**
 * 计数人性化（详情页头部精度）：万/亿带一位小数，整数省略小数点——
 * 27.05万 → 「27.1万」、40.26万 → 「40.3万」、3786.01万 → 「3786万」。
 * 与 hgplayer 头部口径一致（「27.1万人追剧 / 40.3万次播放 / 3786万」）；
 * 列表卡片用的 formatPlayCount 保持整数万不变。
 */
export function formatCountPrecise(n: number): string {
  if (n >= 100_000_000) return `${trimTrailingZero((n / 100_000_000).toFixed(1))}亿`;
  if (n >= 10_000) return `${trimTrailingZero((n / 10_000).toFixed(1))}万`;
  return String(n);
}

/** 去掉 toFixed(1) 尾部的 「.0」。 */
function trimTrailingZero(s: string): string {
  return s.endsWith('.0') ? s.slice(0, -2) : s;
}
