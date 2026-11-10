import { ChevronRight } from 'lucide-react';
import { ChromeSurface } from './chrome-surface';
import { t, tf } from '@/locales';
import { cn } from '@/lib/utils';

/** 沉浸流底部「观看完整短剧 · 全N集」横条：进入选中剧连播态的唯一手动入口。 */
export function BingeEntryBar({
  chromeShown,
  narrow,
  portrait,
  episodeCount,
  onEnterBinge,
}: {
  chromeShown: boolean;
  narrow: boolean;
  portrait: boolean;
  episodeCount: number;
  onEnterBinge: () => void;
}) {
  return (
    <ChromeSurface
      shown={chromeShown}
      data-wheel-block
      className={cn('absolute inset-x-3 z-10', narrow && !portrait ? 'bottom-10' : 'bottom-14')}
    >
      <button
        type="button"
        onClick={(e) => {
          e.stopPropagation();
          onEnterBinge();
        }}
        className="flex w-full cursor-pointer items-center justify-center gap-2 rounded-lg bg-black/60 px-3 py-2.5 text-sm text-white backdrop-blur-sm transition-colors hover:bg-black/75"
      >
        <span className="truncate">
          {t('player.watchFullSeries')}
          {episodeCount > 0 && (
            <span className="text-white/70">
              {' '}
              · {tf('player.totalEpisodes', { count: episodeCount })}
            </span>
          )}
        </span>
        <ChevronRight className="size-4 shrink-0 text-white/60" aria-hidden />
      </button>
    </ChromeSurface>
  );
}
