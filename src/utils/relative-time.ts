import { t, tf } from '@/locales';

/** 评论区相对时间（hgplayer 口径：刚刚 / N 分钟前 / N 小时前 / 昨天 /
 *  N 天前（30 天内）/ M月D日）。评论面板与剧评回复共用。 */
export function relativeTime(unixSec: number): string {
  const diff = Date.now() / 1000 - unixSec;
  if (unixSec <= 0) return '';
  if (diff < 60) return t('player.comments.justNow');
  if (diff < 3600) return tf('player.comments.minutesAgo', { n: Math.floor(diff / 60) });
  if (diff < 86400) return tf('player.comments.hoursAgo', { n: Math.floor(diff / 3600) });
  if (diff < 172800) return t('player.comments.yesterday');
  if (diff < 2592_000) return tf('player.comments.daysAgo', { n: Math.floor(diff / 86400) });
  const d = new Date(unixSec * 1000);
  return `${d.getMonth() + 1}/${d.getDate()}`;
}
