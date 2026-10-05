import { useCallback, useEffect, useState } from 'react';
import { Loader2 } from 'lucide-react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Skeleton } from '@/components/ui/skeleton';
import { PlayerView } from '@/features/player/components/player-page';
import { useFeed, usePrefetchSeriesEpisodes, useResolveSeries } from '@/lib/queries';
import { usePlayerStore } from '@/lib/stores/player';
import { t, tf } from '@/i18n';

/**
 * 首页：沉浸式播放器流（hgplayer 同款形态）。
 *
 * 打开即播官方推荐流，视频**铺满整页**——剧名/集数/简介叠加在画面左下，
 * 鼠标静止 3 秒连同互动栏一起淡出；**滚轮 / ↑↓ 直接切上一部/下一部剧**，
 * 不再要按钮（解析中的提示浮在顶部，不打断画面）。
 *
 * 切换要快：当前剧进入时就预取下一部的分集档案（本地缺失自动回落解析），
 * 真正切过去时只剩取流时间。
 */
export function HomePage() {
  const feed = useFeed();
  const [feedIndex, setFeedIndex] = useState(0);
  const current = feed.items[feedIndex];
  const currentId = current?.seriesId;
  const { mutate: resolve, isPending: resolving } = useResolveSeries();
  const prefetchEpisodes = usePrefetchSeriesEpisodes();
  const setTarget = usePlayerStore((s) => s.setTarget);

  // 当前剧变化（含首进）→ 解析登记并设为播放目标（从第 1 集开始，
  // 看过的剧由后端 resumeAt 自动接续进度）
  useEffect(() => {
    if (!currentId) return;
    resolve(currentId, {
      onSuccess: (series) => setTarget(series.seriesId, 1),
      onError: (e) =>
        toast.error(t('common.resolveFailed'), { description: String(e.message ?? e) }),
    });
    // resolve/mutate 引用稳定，只需要跟当前剧走
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [currentId]);

  // 预取下一部剧（以及上一步回退目标）的分集档案：换剧时 resolve 秒回
  const nextItem = feed.items[feedIndex + 1];
  useEffect(() => {
    if (nextItem) prefetchEpisodes(nextItem.seriesId);
  }, [nextItem, prefetchEpisodes]);

  // 快滑到信息流尾部时预取下一页
  useEffect(() => {
    if (feed.hasMore && !feed.isFetchingMore && feed.items.length - feedIndex <= 3) {
      feed.loadMore();
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [feedIndex, feed.items.length, feed.hasMore, feed.isFetchingMore]);

  const next = useCallback(() => {
    setFeedIndex((i) => Math.min(i + 1, feed.items.length - 1));
  }, [feed.items.length]);
  const prev = useCallback(() => setFeedIndex((i) => Math.max(i - 1, 0)), []);
  const step = useCallback(
    (dir: 1 | -1) => {
      if (dir === 1) next();
      else prev();
    },
    [next, prev],
  );

  if (feed.isLoading && feed.items.length === 0) {
    return (
      <div className="flex h-full flex-col gap-3 p-4">
        <Skeleton className="min-h-0 flex-1 rounded-xl" />
        <Skeleton className="h-4 w-1/3" />
        <Skeleton className="h-3 w-2/3" />
      </div>
    );
  }
  if (feed.error && feed.items.length === 0) {
    return (
      <div className="text-muted-foreground flex flex-col items-center gap-3 py-16">
        <p>{t('feed.loadFailed')}</p>
        <p className="text-destructive text-xs">{feed.error}</p>
        <Button variant="outline" size="sm" onClick={() => void feed.refresh()}>
          <Loader2 className="mr-1 size-4 animate-spin" aria-hidden />
          {t('feed.retry')}
        </Button>
      </div>
    );
  }
  if (!current) {
    return (
      <div className="text-muted-foreground flex flex-col items-center gap-2 py-16">
        <p className="text-sm">{t('home.empty')}</p>
        <Button variant="outline" size="sm" onClick={() => void feed.refresh()}>
          {t('feed.retry')}
        </Button>
      </div>
    );
  }

  return (
    <div className="relative h-full min-h-0">
      {/* 播放器铺满整页；滚轮/↑↓ 在沉浸流里切上一部/下一部剧 */}
      <PlayerView seriesPanelMode="overlay" onWheelStep={step} />
      {resolving && (
        <div className="pointer-events-none absolute inset-x-0 top-0 z-40 flex justify-center pt-3">
          <p className="flex items-center gap-2 rounded-full bg-black/60 px-4 py-1.5 text-xs text-white/90 shadow-lg backdrop-blur-sm">
            <Loader2 className="size-3.5 animate-spin" aria-hidden />
            {tf('home.preparing', { title: current.title })}
          </p>
        </div>
      )}
    </div>
  );
}
