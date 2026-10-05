import { useCallback, useEffect, useRef, useState } from 'react';
import { Loader2 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Skeleton } from '@/components/ui/skeleton';
import { PlayerView } from '@/features/player/components/player-page';
import { play } from '@/lib/ipc/commands';
import {
  useFeed,
  usePrefetchSeriesEpisodes,
  useSeriesEpisodes,
} from '@/lib/queries';
import { usePlayerStore } from '@/lib/stores/player';
import { t } from '@/i18n';

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
  const prefetchEpisodes = usePrefetchSeriesEpisodes();
  const setTarget = usePlayerStore((s) => s.setTarget);

  /**
   * 当前剧的档案：走 `get_series_episodes`（本地命中秒回，缺失才回落解析），
   * **不走 resolve_series**——那是每次都打网络的解析，预取的缓存它吃不到，
   * 滚轮切剧会在「正在准备」上白等一拍。预取 effect 已经把下一部剧的
   * 分集档案灌进同一份缓存，这里直接命中。
   */
  const { data: currentSeries } = useSeriesEpisodes(currentId ?? '');

  // 档案就位 → 设为播放目标（从第 1 集开始，看过的剧由 resumeAt 接进度）
  useEffect(() => {
    if (!currentSeries) return;
    setTarget(currentSeries.seriesId, 1);
  }, [currentSeries, setTarget]);

  // 预取下一部剧（以及上一步回退目标）的分集档案：换剧时 resolve 秒回
  const nextItem = feed.items[feedIndex + 1];
  useEffect(() => {
    if (nextItem) prefetchEpisodes(nextItem.seriesId);
  }, [nextItem, prefetchEpisodes]);

  // 预取下一部剧第 1 集的**流**（取流表 + 渐进填充）：滚过去时缓存已就绪，
  // 首帧只等头部数据——hgplayer「一切就下一部」的同款做法。
  // 延迟 1.5 秒让当前剧先把首帧吃下来，别抢带宽；同一部剧只取一次。
  const streamPrefetched = useRef(new Set<string>());
  useEffect(() => {
    if (!nextItem) return;
    const id = nextItem.seriesId;
    if (streamPrefetched.current.has(id)) return;
    streamPrefetched.current.add(id);
    const timer = setTimeout(() => {
      void play.prefetch(id).catch(() => {
        // 失败不算数：下次这个剧再成为「下一部」时允许重试
        streamPrefetched.current.delete(id);
      });
    }, 3_000);
    return () => clearTimeout(timer);
  }, [nextItem]);

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
    // overflow-hidden + 绝对定位铺满：AppShell 的 main 是 overflow-y-auto
    // （别的页面靠它滚），沉浸流内部任何 1px 超高都会冒出一条页面滚动条，
    // 还会把滚轮切剧吃掉——这里整个锁死在视口内。
    <div className="relative h-full min-h-0 overflow-hidden">
      <div className="absolute inset-0">
        {/* 播放器铺满整页；滚轮/↑↓ 在沉浸流里切上一部/下一部剧 */}
        <PlayerView seriesPanelMode="overlay" onWheelStep={step} />
      </div>
    </div>
  );
}
