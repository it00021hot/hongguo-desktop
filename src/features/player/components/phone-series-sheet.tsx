/** 窄屏半屏剧集 Sheet：海报+标题进详情、简介|选集、底收藏。 */
import { useState } from 'react';
import { useNavigate } from '@tanstack/react-router';
import { ChevronRight, Star } from 'lucide-react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Sheet, SheetContent, SheetTitle } from '@/components/ui/sheet';
import { SeriesCover } from '@/components/common/series-cover';
import { useAccount, useBookshelf, useInteractionState, useSeriesCollect } from '@/service/queries';
import { t, tf } from '@/locales';
import { cn } from '@/lib/utils';
import { EpisodePicker } from './episode-picker';

/** 深色底 Sheet 关闭钮：白 X、无 ring 叠影 */
export const DARK_SHEET_CLOSE =
  'text-white opacity-80 hover:opacity-100 focus:ring-0 focus:ring-offset-0';

export function PhoneSeriesSheet({
  open,
  onOpenChange,
  seriesId,
  title,
  cover,
  intro,
  episodeCount,
  currentIndex,
  onPickEpisode,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  seriesId: string;
  title?: string;
  cover?: string;
  intro?: string;
  episodeCount: number;
  currentIndex: number;
  onPickEpisode: (vidIndex: number) => void;
}) {
  const navigate = useNavigate();
  const [tab, setTab] = useState<'intro' | 'episodes'>('episodes');
  const { data: account } = useAccount();
  const loggedIn = !!account;
  const { data: state } = useInteractionState();
  const { data: bookshelf } = useBookshelf();
  const collect = useSeriesCollect();
  const collected =
    (state?.items.find((i) => i.seriesId === seriesId)?.followed ?? false) ||
    (bookshelf?.some((b) => b.seriesId === seriesId) ?? false);

  const goDetail = () => {
    onOpenChange(false);
    void navigate({ to: '/detail', search: { seriesId } });
  };

  const onCollect = () => {
    if (!loggedIn) {
      toast.info(t('player.interact.loginRequired'));
      void navigate({ to: '/settings' });
      return;
    }
    collect.mutate(
      { seriesId, collect: !collected },
      {
        onSuccess: () =>
          toast.success(t(collected ? 'player.interact.uncollected' : 'player.interact.collected')),
      },
    );
  };

  return (
    <Sheet open={open} onOpenChange={onOpenChange}>
      <SheetContent
        side="bottom"
        closeClassName={DARK_SHEET_CLOSE}
        className="flex max-h-[70vh] flex-col gap-0 overflow-hidden rounded-t-2xl border-white/10 bg-neutral-950 p-0 text-white"
        onClick={(e) => e.stopPropagation()}
      >
        <SheetTitle className="sr-only">{t('player.episodes')}</SheetTitle>

        {/* 头：海报 + 标题 > + 全N集 */}
        <div className="flex shrink-0 gap-3 border-b border-white/10 px-4 pt-3 pr-12 pb-3">
          <div className="bg-muted relative aspect-[3/4] w-14 shrink-0 overflow-hidden rounded-lg">
            {cover ? <SeriesCover cover={cover} alt={title ?? ''} /> : null}
          </div>
          <div className="min-w-0 flex-1">
            <button
              type="button"
              onClick={goDetail}
              className="flex max-w-full items-center gap-0.5 text-left text-base font-semibold text-white"
            >
              <span className="truncate">{title || t('player.miniTitle')}</span>
              <ChevronRight className="size-4 shrink-0 text-white/50" />
            </button>
            {episodeCount > 0 && (
              <p className="mt-1 text-xs text-white/55">
                {tf('player.totalEpisodes', { count: episodeCount })}
              </p>
            )}
          </div>
        </div>

        {/* 简介 | 选集 */}
        <div className="flex shrink-0 gap-5 border-b border-white/10 px-4">
          {(
            [
              ['intro', t('player.intro')],
              ['episodes', t('player.episodes')],
            ] as const
          ).map(([key, label]) => (
            <button
              key={key}
              type="button"
              onClick={() => setTab(key)}
              className={cn(
                'relative cursor-pointer py-2.5 text-sm font-semibold transition-colors',
                tab === key ? 'text-white' : 'text-white/45 hover:text-white/75',
              )}
            >
              {label}
              {tab === key && (
                <span className="absolute inset-x-0 bottom-0 h-0.5 rounded-full bg-white" />
              )}
            </button>
          ))}
        </div>

        <div className="min-h-0 flex-1 scrollbar-thin overflow-y-auto px-4 py-3">
          {tab === 'intro' ? (
            <p className="text-sm leading-relaxed whitespace-pre-wrap text-white/75">
              {intro?.trim() ? intro : t('player.introEmpty')}
            </p>
          ) : (
            <EpisodePicker
              embedded
              seriesId={seriesId}
              currentIndex={currentIndex}
              onSelect={onPickEpisode}
              onClose={() => onOpenChange(false)}
            />
          )}
        </div>

        <div className="shrink-0 border-t border-white/10 px-4 py-3">
          <Button
            type="button"
            variant={collected ? 'secondary' : 'default'}
            className="h-10 w-full"
            onClick={onCollect}
          >
            <Star className={cn('size-4', collected && 'fill-star text-star')} />
            {t(collected ? 'detail.collected' : 'detail.collect')}
          </Button>
        </div>
      </SheetContent>
    </Sheet>
  );
}
