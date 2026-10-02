import { useMemo } from 'react';
import { useNavigate } from '@tanstack/react-router';
import { History, Play } from 'lucide-react';
import { Badge } from '@/components/ui/badge';
import { usePlaybackHistory, useSeriesList } from '@/lib/queries';
import { usePlayerStore } from '@/lib/stores/player';
import { formatDuration } from '@/lib/format';
import { t, tf } from '@/i18n';

/**
 * 「继续观看」——播放历史。
 *
 * 没有在播任何一集时占据主区域。放在播放器侧栏是不够的：
 * 侧栏只在已经选中剧集时才渲染，而用户找历史恰恰是在「还没在播」的时候，
 * 那样入口等于不存在。
 */
export function ContinueWatching() {
  const navigate = useNavigate();
  const setTarget = usePlayerStore((s) => s.setTarget);
  const { data: history } = usePlaybackHistory();
  const { data: seriesList } = useSeriesList();

  /** 把历史与剧集档案对起来：档案给封面/标题，历史给看到第几集。 */
  const items = useMemo(() => {
    const byId = new Map((seriesList ?? []).map((s) => [s.seriesId, s]));
    return (history ?? [])
      .map((h) => {
        const series = byId.get(h.seriesId);
        if (!series) return null;
        return { ...h, series };
      })
      .filter((x): x is NonNullable<typeof x> => x !== null);
  }, [history, seriesList]);

  if (items.length === 0) {
    return (
      <div className="grid h-full place-items-center p-6">
        <div className="text-center">
          <History className="text-muted-foreground mx-auto size-8" />
          <p className="text-muted-foreground mt-3 text-sm">{t('player.historyEmpty')}</p>
          <p className="text-muted-foreground mt-1 text-xs">{t('player.historyHint')}</p>
        </div>
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-4 p-6">
      <h2 className="flex items-center gap-2 text-sm font-semibold">
        <History className="size-4" />
        {t('player.continueWatching')}
      </h2>

      <div className="grid grid-cols-2 gap-4 md:grid-cols-3 lg:grid-cols-5">
        {items.map((item) => (
          <button
            key={item.seriesId}
            type="button"
            className="group bg-card overflow-hidden rounded-lg border text-left transition-colors hover:border-foreground/30"
            onClick={() => {
              setTarget(item.seriesId, item.vidIndex);
              void navigate({ to: '/player' });
            }}
          >
            <div className="bg-muted relative aspect-3/4 w-full overflow-hidden">
              {item.series.cover ? (
                <img
                  src={item.series.cover}
                  alt=""
                  loading="lazy"
                  className="size-full object-cover"
                />
              ) : null}
              <span className="bg-background/80 absolute right-2 bottom-2 grid size-8 place-items-center rounded-full opacity-0 transition-opacity group-hover:opacity-100">
                <Play className="size-4" />
              </span>
            </div>
            <div className="grid gap-1 p-2">
              <p className="truncate text-sm font-medium">{item.series.title}</p>
              <div className="flex items-center gap-1.5">
                <Badge variant="secondary" className="text-[10px]">
                  {tf('player.epShort', { index: item.vidIndex })}
                </Badge>
                <span className="text-muted-foreground text-xs tabular-nums">
                  {formatDuration(item.currentTime)}
                </span>
              </div>
            </div>
          </button>
        ))}
      </div>
    </div>
  );
}
