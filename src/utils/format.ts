import { locale, tf } from '@/locales';

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

/** 播放量人性化：中文 1.2亿 / 3456万；英文 120M / 34.6K。 */
export function formatPlayCount(n: number): string {
  if (locale() === 'zh-CN') {
    if (n >= 100_000_000) return tf('number.yi', { value: (n / 100_000_000).toFixed(1) });
    if (n >= 10_000) return tf('number.wan', { value: Math.round(n / 10_000) });
    return String(n);
  }
  if (n >= 1_000_000)
    return tf('number.million', { value: trimTrailingZero((n / 1_000_000).toFixed(1)) });
  if (n >= 10_000)
    return tf('number.thousand', { value: trimTrailingZero((n / 1_000).toFixed(1)) });
  return String(n);
}

/** 计数人性化（详情页头部精度）：万/亿带一位小数；列表卡片用的 formatPlayCount 保持整数万不变。 */
export function formatCountPrecise(n: number): string {
  if (locale() === 'zh-CN') {
    if (n >= 100_000_000) {
      return tf('number.yi', { value: trimTrailingZero((n / 100_000_000).toFixed(1)) });
    }
    if (n >= 10_000) {
      return tf('number.wan', { value: trimTrailingZero((n / 10_000).toFixed(1)) });
    }
    return String(n);
  }
  return formatPlayCount(n);
}

/**
 * 互动栏计数：中文「1.4万 / 1.2百万」；英文走 {@link formatPlayCount}。
 * 百万档仅中文 UI 使用（官方抖音系展示）。
 */
export function formatCountCompact(n: number): string {
  if (locale() === 'zh-CN') {
    if (n >= 1_000_000) {
      return tf('number.baiwan', {
        value: trimTrailingZero((n / 1_000_000).toFixed(1)),
      });
    }
    if (n >= 10_000) {
      const decimals = n % 10_000 >= 1000 ? 1 : 0;
      return tf('number.wan', {
        value: trimTrailingZero((n / 10_000).toFixed(decimals)),
      });
    }
    return String(n);
  }
  return formatPlayCount(n);
}

/** 去掉 toFixed(1) 尾部的 「.0」。 */
function trimTrailingZero(s: string): string {
  return s.endsWith('.0') ? s.slice(0, -2) : s;
}
