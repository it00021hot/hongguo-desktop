import { t, tf } from '@/locales';

/** 评论区相对时间（刚刚 / N 分钟前 / N 小时前 / 昨天 /
 *  N 天前（30 天内）/ M月D日）。评论面板与剧评回复共用。
 *
 *  入参是 UTC 毫秒时间戳（后端 createTime 口径）。 */
export function relativeTime(unixMs: number): string {
  const diff = Date.now() - unixMs;
  if (unixMs <= 0) return '';
  if (diff < 60_000) return t('player.comments.justNow');
  if (diff < 3_600_000) return tf('player.comments.minutesAgo', { n: Math.floor(diff / 60_000) });
  if (diff < 86_400_000) return tf('player.comments.hoursAgo', { n: Math.floor(diff / 3_600_000) });
  if (diff < 172_800_000) return t('player.comments.yesterday');
  if (diff < 2_592_000_000)
    return tf('player.comments.daysAgo', { n: Math.floor(diff / 86_400_000) });
  const d = new Date(unixMs);
  return `${d.getMonth() + 1}/${d.getDate()}`;
}
