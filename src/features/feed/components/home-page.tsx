import { useEffect, useMemo, useRef, useState } from 'react';
import { Flame, Loader2, RefreshCw } from 'lucide-react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Skeleton } from '@/components/ui/skeleton';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';
import {
  SeriesDetailSheet,
  type SeriesRef,
} from '@/features/series/components/series-detail-sheet';
import { FeedCardGrid } from './feed-card-grid';
import { useDownloadTasks, useFeed, useResolveSeries } from '@/lib/queries';
import { t } from '@/i18n';
import type { FeedItem } from '@/lib/schema';

/**
 * 首页：官方推荐信息流。
 *
 * 两个视图共用同一批已拉取数据：推荐 = 拉取序（算法个性化排布），
 * 热榜 = 播放量重排（拉得越多池子越大、榜越实）。
 * 预约（新剧日历）接口参数未破解，登录体系落地后一并补上。
 */
export function HomePage() {
  const feed = useFeed();
  const { data: tasks } = useDownloadTasks();
  const { mutate: resolve, isPending: resolving } = useResolveSeries();
  const [detail, setDetail] = useState<{ card: SeriesRef; selected: number[] } | null>(null);
  const sentinelRef = useRef<HTMLDivElement | null>(null);

  // 已下载集数角标
  const downloadedMap = useMemo(() => {
    const map: Record<string, number> = {};
    for (const task of tasks ?? []) {
      if (task.status !== 'completed') continue;
      map[task.seriesId] = (map[task.seriesId] ?? 0) + 1;
    }
    return map;
  }, [tasks]);

  // 热榜：同一池子按播放量降序（拉过的都参与，榜随加载变实）
  const hot = useMemo(() => [...feed.items].sort((a, b) => b.playCnt - a.playCnt), [feed.items]);

  // 首次进入自动加载
  useEffect(() => {
    void feed.refresh();
    // refresh 是闭包，依赖它只会反复触发；这里只需要挂载时拉一次
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // 滚动到底自动翻页（推荐 tab 下才有意义；热榜也受益——池子变大）。
  // 观察器只建一次：feed 是每次渲染的新对象，进依赖的话每次渲染都会
  // disconnect + 重新 observe——哨兵在视口内时重新 observe 会立刻回调，
  // 形成「加载→渲染→重建→再加载」的级联，页面连续跳变（闪屏的来源）。
  // 最新状态走 ref 镜像。
  const feedRef = useRef(feed);
  useEffect(() => {
    feedRef.current = feed;
  });
  useEffect(() => {
    const el = sentinelRef.current;
    if (!el) return;
    const io = new IntersectionObserver(
      (entries) => {
        const f = feedRef.current;
        if (entries[0]?.isIntersecting && f.hasMore && !f.isLoading && !f.isFetchingMore) {
          void f.loadMore();
        }
      },
      { rootMargin: '400px' },
    );
    io.observe(el);
    return () => io.disconnect();
  }, []);

  const handleSelect = (item: FeedItem) => {
    // 信息流的剧多半没解析过档案：先 resolve（拉分集 + 登记）再开抽屉。
    // 失败要出提示而不是无声吞掉——点了没反应是最差的体验。
    resolve(item.seriesId, {
      onSuccess: (series) =>
        setDetail({
          card: {
            seriesId: series.seriesId,
            seriesTitle: series.title,
            cover: series.cover || item.cover,
            episodeCount: series.episodeCount,
            tags: series.tags.length > 0 ? series.tags : item.tags,
          },
          selected: [],
        }),
      onError: (e) =>
        toast.error(t('common.resolveFailed'), { description: String(e.message ?? e) }),
    });
  };

  const gridProps = { downloadedMap, onSelect: handleSelect };

  return (
    <div className="flex flex-col gap-4">
      <Tabs defaultValue="recommend">
        <TabsList>
          <TabsTrigger value="recommend">{t('feed.tabs.recommend')}</TabsTrigger>
          <TabsTrigger value="hot">
            <Flame className="mr-1 size-4" aria-hidden />
            {t('feed.tabs.hot')}
          </TabsTrigger>
        </TabsList>

        <TabsContent value="recommend" className="mt-3">
          {feed.isLoading ? (
            <SkeletonGrid />
          ) : feed.error ? (
            <div className="text-muted-foreground flex flex-col items-center gap-3 py-16">
              <p>{t('feed.loadFailed')}</p>
              <p className="text-destructive text-xs">{feed.error}</p>
              <Button variant="outline" size="sm" onClick={() => void feed.refresh()}>
                <RefreshCw className="mr-1 size-4" aria-hidden />
                {t('feed.retry')}
              </Button>
            </div>
          ) : (
            <FeedCardGrid items={feed.items} {...gridProps} />
          )}
        </TabsContent>

        <TabsContent value="hot" className="mt-3">
          {/* 热榜与推荐共用加载状态：池子来自同一个流 */}
          {feed.isLoading ? <SkeletonGrid /> : <FeedCardGrid items={hot} ranked {...gridProps} />}
        </TabsContent>
      </Tabs>

      {/* 滚动哨兵 + 状态行 */}
      <div ref={sentinelRef} className="h-px" aria-hidden />
      {feed.isFetchingMore && (
        <p className="text-muted-foreground flex items-center justify-center gap-2 py-2 text-sm">
          <Loader2 className="size-4 animate-spin" aria-hidden />
          {t('feed.loadingMore')}
        </p>
      )}
      {!feed.isLoading && !feed.error && !feed.hasMore && feed.items.length > 0 && (
        <p className="text-muted-foreground py-2 text-center text-sm">{t('feed.end')}</p>
      )}
      {resolving && (
        <p className="text-muted-foreground bg-card fixed bottom-4 left-1/2 flex -translate-x-1/2 items-center gap-2 rounded-full border px-4 py-2 text-sm shadow-lg">
          <Loader2 className="size-4 animate-spin" aria-hidden />
          {t('feed.resolving')}
        </p>
      )}

      <SeriesDetailSheet
        card={detail?.card ?? null}
        selected={detail?.selected ?? []}
        onSelectedChange={(next) => setDetail((d) => (d ? { ...d, selected: next } : d))}
        onOpenChange={(open) => !open && setDetail(null)}
      />
    </div>
  );
}

function SkeletonGrid() {
  return (
    <div className="grid grid-cols-2 gap-4 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 2xl:grid-cols-8">
      {Array.from({ length: 10 }, (_, i) => (
        <div key={i} className="flex flex-col gap-2">
          <Skeleton className="aspect-[3/4] w-full rounded-xl" />
          <Skeleton className="h-4 w-3/4" />
          <Skeleton className="h-3 w-1/2" />
        </div>
      ))}
    </div>
  );
}
