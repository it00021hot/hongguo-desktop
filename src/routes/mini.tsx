import { createFileRoute } from '@tanstack/react-router';
import { MiniPlayerPage } from '@/features/player/components/mini-player';
import { readLastTarget } from '@/lib/playback-prefs';
import { app as appApi } from '@/lib/ipc/commands';
import { t } from '@/i18n';

/**
 * 小窗播放（独立置顶窗口，由 `open_mini_window` 以
 * `/mini?series=…&index=…` 打开）。它不是主窗口里的页面——不进应用壳
 * （侧栏/顶栏都没有），整个窗口就是播放器。
 */
export const Route = createFileRoute('/mini')({
  validateSearch: (search: Record<string, unknown>) => ({
    series: typeof search.series === 'string' ? search.series : '',
    index: typeof search.index === 'number' && search.index > 0 ? search.index : 1,
  }),
  component: MiniRoute,
});

function MiniRoute() {
  const { series, index } = Route.useSearch();
  // 异常直开（没带目标）：兜底读「上次在看」——两窗共享同一份 localStorage
  const last = readLastTarget();
  const targetSeries = series || last?.seriesId || '';
  const targetIndex = series ? index : (last?.vidIndex ?? 1);

  if (!targetSeries) {
    return (
      <div className="flex h-screen w-screen flex-col items-center justify-center gap-3 bg-black text-sm text-white/70">
        <span>{t('player.miniNoTarget')}</span>
        <button
          type="button"
          onClick={() => void appApi.closeMiniWindow().catch(() => undefined)}
          className="rounded-full border border-white/30 px-4 py-1.5 text-xs text-white hover:bg-white/10"
        >
          {t('player.backToMain')}
        </button>
      </div>
    );
  }
  return <MiniPlayerPage initialSeries={targetSeries} initialIndex={targetIndex} />;
}
