import { Flame, MessageSquare, Star, Tv } from 'lucide-react';
import { Badge } from '@/components/ui/badge';
import { t, tf } from '@/i18n';
import type { FeedItem } from '@/lib/schema';

interface Props {
  items: FeedItem[];
  /** 每部剧已下载的集数，用于卡片角标 */
  downloadedMap?: Record<string, number>;
  /** 热榜模式：显示名次与播放量 */
  ranked?: boolean;
  onSelect: (item: FeedItem) => void;
  trailing?: React.ReactNode;
}

/** 播放量人性化：1.2亿 / 3456万 / 8921。 */
function formatPlayCount(n: number): string {
  if (n >= 100_000_000) return `${(n / 100_000_000).toFixed(1)}亿`;
  if (n >= 10_000) return `${Math.round(n / 10_000)}万`;
  return String(n);
}

/** 信息流卡片网格（首页推荐 / 热榜共用）。 */
export function FeedCardGrid({ items, downloadedMap, ranked, onSelect, trailing }: Props) {
  return (
    <div className="grid grid-cols-2 gap-4 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 2xl:grid-cols-8">
      {items.map((item, i) => {
        const downloaded = downloadedMap?.[item.seriesId] ?? 0;
        return (
          <article
            key={item.seriesId}
            role="button"
            tabIndex={0}
            aria-label={item.title}
            onClick={() => onSelect(item)}
            onKeyDown={(e) => {
              if (e.key !== 'Enter' && e.key !== ' ') return;
              // 空格默认会滚动页面，按钮不该有滚动副作用
              e.preventDefault();
              onSelect(item);
            }}
            className="group bg-card hover:border-foreground/30 focus-visible:border-foreground/30 flex w-full cursor-pointer flex-col overflow-hidden rounded-xl border text-left transition-colors hover:shadow-md focus-visible:outline-none"
          >
            <div className="bg-muted relative aspect-[3/4] w-full overflow-hidden">
              {item.cover ? (
                <img
                  src={item.cover}
                  alt={item.title}
                  loading="lazy"
                  className="size-full object-cover"
                />
              ) : (
                <div className="text-muted-foreground grid size-full place-items-center">
                  <Tv className="size-8" />
                </div>
              )}

              {downloaded > 0 && (
                <Badge
                  variant="success"
                  className="absolute top-2 right-2 shadow-sm"
                  title={tf('browse.downloadedBadge', { count: downloaded })}
                >
                  {downloaded}
                </Badge>
              )}

              {ranked && (
                <span
                  className="absolute top-2 left-2 grid size-6 place-items-center rounded bg-black/70 text-xs font-bold text-amber-400"
                  aria-hidden
                >
                  {i + 1}
                </span>
              )}

              {item.episodeCnt > 0 && (
                <span className="absolute bottom-2 left-2 rounded bg-black/70 px-1.5 py-0.5 text-xs text-white">
                  {tf('common.episodeCount', { count: item.episodeCnt })}
                </span>
              )}
              {item.playCnt > 0 && (
                <span className="absolute right-2 bottom-2 flex items-center gap-0.5 rounded bg-black/70 px-1.5 py-0.5 text-xs text-white">
                  <Flame className="size-3 text-orange-400" aria-hidden />
                  {formatPlayCount(item.playCnt)}
                </span>
              )}
            </div>

            <div className="flex flex-col gap-1.5 p-3">
              <p className="truncate text-sm font-semibold" title={item.title}>
                {item.title}
              </p>
              <div className="text-muted-foreground flex items-center gap-2 text-xs">
                {item.score > 0 && (
                  <span className="flex items-center gap-0.5" title={t('feed.scoreTitle')}>
                    <Star className="size-3 text-amber-400" aria-hidden />
                    {item.score.toFixed(1)}
                  </span>
                )}
                {item.commentCount > 0 && (
                  <span className="flex items-center gap-0.5">
                    <MessageSquare className="size-3" aria-hidden />
                    {formatPlayCount(item.commentCount)}
                  </span>
                )}
              </div>
              {item.tags.length > 0 && (
                <div className="flex flex-nowrap gap-1 overflow-hidden">
                  {item.tags.slice(0, 3).map((tag) => (
                    <Badge key={tag} variant="secondary" className="shrink-0 text-[10px]">
                      {tag}
                    </Badge>
                  ))}
                </div>
              )}
            </div>
          </article>
        );
      })}
      {trailing}
    </div>
  );
}
