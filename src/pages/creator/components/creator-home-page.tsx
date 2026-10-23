import { useEffect, useRef } from 'react';
import { useNavigate, useRouter } from '@tanstack/react-router';
import { ArrowLeft, Flame } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { ErrorState } from '@/components/common/error-state';
import { Skeleton } from '@/components/ui/skeleton';
import { Spinner } from '@/components/ui/spinner';
import { usePlaySeries } from '@/hooks/use-play-series';
import { useCreatorProfile, useCreatorWorks, usePrefetchSeriesEpisodes } from '@/service/queries';
import { formatCountPrecise } from '@/utils/format';
import { SENTINEL_ROOT_MARGIN_PX } from '@/utils/infinite-scroll';
import type { CreatorWork } from '@/service/schema';
import { t, tf } from '@/locales';

/**
 * 创作者主页（主演/出品方点进来的个人主页）。
 *
 * 2026-10-15 官方实包对齐：头部 = reading/user/profile/get（昵称/头像/
 * 简介），作品列表 = commentapi/user/profile/{uid}（剧级条目，video_id
 * 语义 = series_id，可直接当播放目标）。点击作品 = 起播（统一首页沉浸
 * 流，与追更/历史点卡片同一套 usePlaySeries）。
 */
export function CreatorHomePage({ userId }: { userId: string }) {
  const navigate = useNavigate();
  // routeTree.gen 的 as any 污染 useRouter 返回值；这里只用 history.back
  const router = useRouter() as { history: { back: () => void } };
  const profile = useCreatorProfile(userId);
  const works = useCreatorWorks(userId);

  if (!userId) {
    return (
      <div className="text-muted-foreground grid h-full place-items-center p-6 text-sm">
        {t('detail.noSeries')}
      </div>
    );
  }

  return (
    <div className="mx-auto w-full max-w-6xl px-8 py-6">
      <Button
        variant="ghost"
        size="sm"
        className="text-muted-foreground -ml-2 gap-1.5"
        onClick={() =>
          window.history.length > 1 ? router.history.back() : void navigate({ to: '/' })
        }
      >
        <ArrowLeft className="size-4" aria-hidden />
        {t('detail.back')}
      </Button>

      {/* 头部：头像 + 昵称 + 简介。资料失败不打断作品列表（官方头部同样可缺） */}
      {profile.isPending ? (
        <div className="mt-4 flex items-center gap-4">
          <Skeleton className="size-16 rounded-full" />
          <div className="flex flex-col gap-2">
            <Skeleton className="h-6 w-36" />
            <Skeleton className="h-4 w-64" />
          </div>
        </div>
      ) : profile.data ? (
        <div className="mt-4 flex items-center gap-4">
          {profile.data.avatar && (
            <img
              src={profile.data.avatar}
              alt=""
              className="size-16 shrink-0 rounded-full object-cover"
            />
          )}
          <div className="min-w-0">
            <h1 className="truncate text-xl font-bold">{profile.data.userName}</h1>
            {profile.data.description && (
              <p className="text-muted-foreground mt-1 line-clamp-2 text-sm">
                {profile.data.description}
              </p>
            )}
          </div>
        </div>
      ) : null}

      {/* 作品标题行（官方主页「作品 N」） */}
      {works.data && (
        <h2 className="text-foreground mt-6 text-base font-semibold">
          {tf('creator.works', { count: works.data.pages[0]?.total ?? 0 })}
        </h2>
      )}

      {works.isPending ? (
        <div className="mt-3 grid grid-cols-2 gap-3 sm:grid-cols-3 lg:grid-cols-4 xl:grid-cols-5">
          {Array.from({ length: 10 }, (_, i) => (
            <Skeleton key={i} className="aspect-[3/4] rounded-xl" />
          ))}
        </div>
      ) : works.error ? (
        <ErrorState
          message={t('creator.loadFailed')}
          detail={works.error.message}
          onRetry={() => void works.refetch()}
        />
      ) : (
        <WorksGrid
          works={works.data?.pages.flatMap((p) => p.works) ?? []}
          hasMore={works.hasNextPage}
          onLoadMore={() => void works.fetchNextPage()}
          loadingMore={works.isFetchingNextPage}
        />
      )}
    </div>
  );
}

/** 作品网格（3:4 封面卡：标题 + 集数/热度/播放行；点击起播）。 */
function WorksGrid({
  works,
  hasMore,
  onLoadMore,
  loadingMore,
}: {
  works: CreatorWork[];
  hasMore: boolean;
  onLoadMore: () => void;
  loadingMore: boolean;
}) {
  const playSeries = usePlaySeries();
  const prefetch = usePrefetchSeriesEpisodes();
  const sentinel = useRef<HTMLDivElement | null>(null);

  // 滚动到底续拉（滚动容器是 app-shell 的 #content，哨兵 root 对准它）
  useEffect(() => {
    const el = sentinel.current;
    if (!el || !hasMore) return;
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((e) => e.isIntersecting)) onLoadMore();
      },
      { rootMargin: `${SENTINEL_ROOT_MARGIN_PX}px`, root: document.getElementById('content') },
    );
    observer.observe(el);
    return () => observer.disconnect();
  }, [hasMore, onLoadMore]);

  if (works.length === 0) {
    return <p className="text-muted-foreground mt-6 text-sm">{t('creator.empty')}</p>;
  }

  return (
    <>
      <div className="mt-3 grid grid-cols-2 gap-3 sm:grid-cols-3 lg:grid-cols-4 xl:grid-cols-5">
        {works.map((w) => (
          <button
            key={w.seriesId}
            type="button"
            onMouseEnter={() => prefetch(w.seriesId)}
            onClick={() => playSeries(w.seriesId)}
            className="group relative cursor-pointer overflow-hidden rounded-xl bg-neutral-900 text-left"
            title={w.title}
          >
            {w.cover ? (
              <img
                src={w.cover}
                alt=""
                loading="lazy"
                className="aspect-[3/4] w-full object-cover transition-opacity group-hover:opacity-80"
              />
            ) : (
              <div className="bg-muted aspect-[3/4] w-full" />
            )}
            <div className="pointer-events-none absolute inset-x-0 bottom-0 bg-gradient-to-t from-black/85 to-transparent p-2.5 pt-8">
              <p className="line-clamp-2 text-sm font-medium text-white">{w.title}</p>
              <div className="mt-1 flex items-center gap-2 text-xs text-white/70">
                {w.episodeCnt > 0 && (
                  <span>{tf('detail.allEpisodesBadge', { count: w.episodeCnt })}</span>
                )}
                {w.hot > 0 && (
                  <span className="flex items-center gap-0.5">
                    <Flame className="size-3" aria-hidden />
                    {formatCountPrecise(w.hot)}
                  </span>
                )}
                {w.playCnt > 0 && (
                  <span>{tf('detail.playsCount', { count: formatCountPrecise(w.playCnt) })}</span>
                )}
              </div>
            </div>
          </button>
        ))}
      </div>
      <div ref={sentinel} />
      {loadingMore && (
        <div className="mt-3 flex justify-center py-2">
          <Spinner className="text-muted-foreground size-5" />
        </div>
      )}
    </>
  );
}
