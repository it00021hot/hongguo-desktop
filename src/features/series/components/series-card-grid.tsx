import { Play, Tv } from 'lucide-react';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Skeleton } from '@/components/ui/skeleton';
import { cn } from '@/lib/utils';
import { t, tf } from '@/i18n';
import type { SeriesCard } from '@/lib/schema';

interface Props {
  cards: SeriesCard[];
  /** 每部剧已下载的集数，用于卡片角标 */
  downloadedMap?: Record<string, number>;
  loading?: boolean;
  onPlay: (card: SeriesCard) => void;
}

/** 剧集卡片网格。浏览页与搜索页共用。 */
export function SeriesCardGrid({ cards, downloadedMap, loading, onPlay }: Props) {
  if (loading) {
    return (
      <div className="grid grid-cols-2 gap-4 md:grid-cols-4 lg:grid-cols-5">
        {Array.from({ length: 10 }, (_, i) => (
          <Skeleton key={i} className="h-56" />
        ))}
      </div>
    );
  }

  return (
    <div className="grid grid-cols-2 gap-4 md:grid-cols-4 lg:grid-cols-5">
      {cards.map((card) => {
        const downloaded = downloadedMap?.[card.seriesId] ?? 0;
        return (
          <article
            key={card.seriesId}
            className="group bg-card relative flex flex-col overflow-hidden rounded-xl border transition-shadow hover:shadow-md"
          >
            <div className="bg-muted relative aspect-[3/4] w-full overflow-hidden">
              {card.cover ? (
                <img
                  src={card.cover}
                  alt={card.seriesTitle}
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

              {card.episodeCount > 0 && (
                <span className="absolute bottom-2 left-2 rounded bg-black/70 px-1.5 py-0.5 text-xs text-white">
                  {card.episodeCount} 集
                </span>
              )}

              <Button
                size="sm"
                className={cn(
                  'absolute inset-x-2 bottom-8 opacity-0 transition-opacity',
                  'group-hover:opacity-100 focus-visible:opacity-100',
                )}
                onClick={() => onPlay(card)}
              >
                <Play className="size-4" />
                {t('download.playNow')}
              </Button>
            </div>

            <div className="flex flex-col gap-1.5 p-3">
              {/* 固定两行高度：剧名长短不一会让同排卡片的标签高低错落，看着更碎 */}
              <h3
                className="line-clamp-2 min-h-9 text-sm leading-snug font-semibold"
                title={card.seriesTitle}
              >
                {card.seriesTitle}
              </h3>
              {card.tags.length > 0 && (
                <div className="flex flex-wrap gap-1">
                  {/* 题材用实心 chip：outline 透明底贴在白卡片上像三个浮着的空框 */}
                  {card.tags.slice(0, 3).map((tag) => (
                    <Badge key={tag} variant="secondary" className="text-[10px]">
                      {tag}
                    </Badge>
                  ))}
                </div>
              )}
            </div>
          </article>
        );
      })}
    </div>
  );
}
