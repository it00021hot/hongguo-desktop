/** 新剧推荐视图：封面网格 + 滚动翻页（频道筛选留在页面层）。 */
import { useEffect, useMemo, useRef } from 'react';
import { Loader2 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { RefreshShade } from '@/components/refresh-shade';
import { SkeletonCardGrid } from '@/components/skeletons';
import { useDownloadTasks, useNewDrama } from '@/service/queries';
import { t } from '@/locales';
import type { RankItem } from '@/service/schema';
import { NewDramaCard } from './new-drama-card';

/** 新剧推荐：封面网格 + 滚动翻页（频道筛选在页面标题行）。 */
export function NewDramaRecommends({
  gender,
  onSelect,
}: {
  gender: number;
  onSelect: (item: RankItem) => void;
}) {
  const feed = useNewDrama(gender);
  const { data: tasks } = useDownloadTasks();

  const downloadedMap = useMemo(() => {
    const map: Record<string, number> = {};
    for (const task of tasks ?? []) {
      if (task.status !== 'completed') continue;
      map[task.seriesId] = (map[task.seriesId] ?? 0) + 1;
    }
    return map;
  }, [tasks]);

  // 挂载与换频道的首拉由 useInfiniteQuery 随 queryKey 自动驱动；
  // 这里只留哨兵回调用的最新状态镜像（feed 是每次渲染的新对象）
  const feedRef = useRef(feed);
  useEffect(() => {
    feedRef.current = feed;
  });

  const sentinelRef = useRef<HTMLDivElement | null>(null);
  useEffect(() => {
    const el = sentinelRef.current;
    if (!el) return;
    const io = new IntersectionObserver(
      (entries) => {
        const f = feedRef.current;
        if (entries[0]?.isIntersecting && f.items.length > 0 && !f.isLoading && !f.isFetchingMore) {
          void f.loadMore();
        }
      },
      { rootMargin: '400px' },
    );
    io.observe(el);
    return () => io.disconnect();
  }, []);

  return (
    // 组件根即滚动区（页面唯一会滚的地方）；哨兵在滚动区内，
    // IntersectionObserver 对 viewport 的判定会穿过滚动容器，翻页照常触发
    <div className="flex min-h-0 flex-1 scrollbar-thin flex-col gap-4 overflow-y-auto">
      <RefreshShade refreshing={feed.isRefreshing}>
        {feed.isLoading ? (
          <SkeletonCardGrid count={9} />
        ) : feed.error ? (
          <div className="text-muted-foreground flex flex-col items-center gap-3 py-16">
            <p>{t('newDrama.loadFailed')}</p>
            <p className="text-destructive text-xs">{feed.error}</p>
            <Button variant="outline" size="sm" onClick={() => void feed.refresh()}>
              <Loader2 className="mr-1 size-4 animate-spin" aria-hidden />
              {t('feed.retry')}
            </Button>
          </div>
        ) : (
          <div className="grid grid-cols-2 gap-4 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 2xl:grid-cols-8">
            {feed.items.map((item) => (
              <NewDramaCard
                key={item.seriesId}
                item={item}
                downloaded={downloadedMap[item.seriesId] ?? 0}
                onSelect={onSelect}
              />
            ))}
          </div>
        )}
      </RefreshShade>

      <div ref={sentinelRef} className="h-px" aria-hidden />
      {feed.isFetchingMore && (
        <p className="text-muted-foreground flex items-center justify-center gap-2 py-2 text-sm">
          <Loader2 className="size-4 animate-spin" aria-hidden />
          {t('feed.loadingMore')}
        </p>
      )}
      {!feed.isLoading &&
        !feed.error &&
        feed.items.length > 0 &&
        !feed.isFetchingMore &&
        !feed.hasMore && (
          <p className="text-muted-foreground py-2 text-center text-sm">{t('feed.end')}</p>
        )}
    </div>
  );
}
