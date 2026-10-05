import { useCallback, useEffect, useMemo, useState } from 'react';
import { ListVideo, Loader2, Play, SkipBack, SkipForward } from 'lucide-react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Skeleton } from '@/components/ui/skeleton';
import { PlayerView } from '@/features/player/components/player-page';
import {
  useFeed,
  useResolveSeries,
} from '@/lib/queries';
import { usePlayerStore } from '@/lib/stores/player';
import { t, tf } from '@/i18n';

/**
 * 首页：沉浸式播放器流（对齐第三方形态）。
 *
 * 打开即播官方推荐流的第一部剧，底部「上一个/下一个」在信息流里切换；
 * 剧集信息在视频下方，右侧选集面板**默认隐藏**（按钮呼出，滑出盖在画面上）。
 *
 * 播放器直接复用播放页的 `PlayerView`——弹幕/弹幕设置/音量/清晰度/倍速/
 * 兼容转码全套能力同源，不会出现「沉浸流的播放器是简化版」。
 * 每部剧进入时先 `resolve`（登记档案 + 拿分集）再设为播放目标，
 * 续播进度由本地播放档案接上。
 */
export function HomePage() {
  const feed = useFeed();
  const [feedIndex, setFeedIndex] = useState(0);
  const current = feed.items[feedIndex];
  const currentId = current?.seriesId;
  const { mutate: resolve, isPending: resolving } = useResolveSeries();
  const setTarget = usePlayerStore((s) => s.setTarget);
  const setSeriesPanelOpen = usePlayerStore((s) => s.setSeriesPanelOpen);
  const [detailOpen, setDetailOpen] = useState(false);

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

  const tagsLine = useMemo(
    () => (current ? current.tags.filter(Boolean).slice(0, 4).join('·') : ''),
    [current],
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
    <div className="flex h-full min-h-0 flex-col">
      {/* 顶条：剧名 + 信息流切换 */}
      <div className="flex shrink-0 items-center justify-between gap-3 px-4 py-2">
        <p className="truncate text-sm font-semibold">{current.title}</p>
        <div className="flex shrink-0 items-center gap-2">
          <Button
            size="sm"
            variant="outline"
            disabled={feedIndex === 0}
            onClick={prev}
          >
            <SkipBack className="size-4" />
            {t('home.prev')}
          </Button>
          <Button
            size="sm"
            variant="outline"
            disabled={feedIndex >= feed.items.length - 1 && !feed.hasMore}
            onClick={next}
          >
            {t('home.next')}
            <SkipForward className="size-4" />
          </Button>
        </div>
      </div>

      {/* 播放器：完整能力（弹幕/设置/音量/清晰度/倍速/兼容转码） */}
      <div className="relative min-h-0 flex-1">
        <PlayerView seriesPanelMode="overlay" />
        {/* 右侧选集：默认隐藏，按钮呼出滑出面板 */}
        <Button
          size="sm"
          variant="outline"
          className="absolute top-3 right-3 z-40 shadow-md"
          onClick={() => setSeriesPanelOpen(true)}
        >
          <ListVideo className="size-4" aria-hidden />
          {t('home.episodes')}
        </Button>
        {resolving && (
          <div className="absolute inset-x-0 top-0 z-40 flex justify-center pt-3">
            <p className="text-muted-foreground bg-card flex items-center gap-2 rounded-full border px-4 py-1.5 text-xs shadow-lg">
              {tf('home.preparing', { title: current.title })}
            </p>
          </div>
        )}
      </div>

      {/* 剧信息 */}
      <div className="shrink-0 px-4 py-3">
        <p className="truncate text-sm font-semibold">{current.title}</p>
        <div className="text-muted-foreground mt-1 flex items-center gap-2 text-xs">
          {tagsLine !== '' && <span className="truncate">{tagsLine}</span>}
          {current.episodeCnt > 0 && (
            <span className="shrink-0">{tf('common.episodeCount', { count: current.episodeCnt })}</span>
          )}
          {current.playCnt > 0 && (
            <span className="shrink-0">{tf('home.playCount', { count: current.playCnt })}</span>
          )}
        </div>
      </div>

      {/* 选集/详情抽屉（默认隐藏） */}
      {detailOpen && (
        <div
          className="fixed inset-0 z-40 bg-black/40"
          onClick={() => setDetailOpen(false)}
          aria-hidden
        />
      )}
      {detailOpen && current && (
        <div className="bg-card fixed inset-y-0 right-0 z-50 w-[420px] max-w-full overflow-y-auto border-l p-4 shadow-2xl">
          <div className="mb-3 flex items-center justify-between">
            <p className="text-sm font-semibold">{t('home.detail')}</p>
            <Button size="sm" variant="ghost" onClick={() => setDetailOpen(false)}>
              {t('common.close')}
            </Button>
          </div>
          <p className="text-muted-foreground text-xs leading-relaxed">
            {current.tags.length > 0 && (
              <span className="mr-2">{current.tags.join('·')}</span>
            )}
            {tf('common.episodeCount', { count: current.episodeCnt })}
          </p>
          <Button
            size="sm"
            className="mt-3 w-full"
            onClick={() => {
              setDetailOpen(false);
              setSeriesPanelOpen(true);
            }}
          >
            <Play className="mr-1 size-4" aria-hidden />
            {t('home.openEpisodes')}
          </Button>
        </div>
      )}
    </div>
  );
}
