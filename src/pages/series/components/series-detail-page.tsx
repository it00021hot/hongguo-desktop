import { useMemo, useState } from 'react';
import { useNavigate, useRouter } from '@tanstack/react-router';
import { ArrowLeft, Flame, Heart, Hourglass, Play, Star, UserRound } from 'lucide-react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import { ErrorState } from '@/components/common/error-state';
import { Empty, EmptyContent, EmptyHeader, EmptyTitle } from '@/components/ui/empty';
import { Skeleton } from '@/components/ui/skeleton';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { SeriesCover } from '@/components/common/series-cover';
import { GuessYouLike } from './guess-you-like';
import { RelatedWorks } from './related-works';
import { ReserveButton } from './reserve-button';
import { FollowButton } from '@/components/common/follow-button';
import { ResolveButton } from './resolve-button';
import { ReviewList } from './review-list';
import { formatCountPrecise, formatDuration } from '@/utils/format';
import { resolveResumeTarget } from '@/features/player/resume-target';
import { usePlayerStore } from '@/stores/player';
import {
  useAccount,
  useBookshelf,
  useInteractionState,
  useRelatedSeries,
  useSeriesCollect,
  useSeriesComments,
  useSeriesEpisodes,
  useSeriesDetailMeta,
  useSeriesProgress,
  useVideoDigg,
  useWatchHistory,
} from '@/service/queries';
import { usePlaySeries } from '@/hooks/use-play-series';
import { t, tf } from '@/locales';
import { cn } from '@/lib/utils';
import type { Series } from '@/service/schema';

/**
 * 剧集详情页：封面/统计/标签/简介 + 继续看 + 选集网格 + 剧评 + 相关推荐，数据全走官方接口查询。
 * prefill（来源页档案快照）给未上线剧渲染「即将上线」降级视图（合成 0 集档案走正常渲染流）。
 */
export function SeriesDetailPage({
  seriesId,
  prefill,
}: {
  seriesId: string;
  prefill?: { title: string; cover: string; tags: string; desc: string };
}) {
  const navigate = useNavigate();
  // routeTree.gen 的 as any 污染 useRouter 返回值；这里只用 history.back
  const router = useRouter() as { history: { back: () => void } };
  const playSeries = usePlaySeries();
  const setResumeHint = usePlayerStore((state) => state.setResumeHint);
  const [introExpanded, setIntroExpanded] = useState(false);
  const [detailTab, setDetailTab] = useState('episodes');

  const { data: series, isPending, isError, error } = useSeriesEpisodes(seriesId || null);
  const { data: localProgress } = useSeriesProgress(seriesId ?? '');
  const { data: history } = useWatchHistory();
  // 相关推荐总数（相关作品 + 猜你喜欢，plan 接口两格一并计）——tab 上的
  // 计数用；RelatedWorks 里还有一份同 key 的调用，react-query 共享缓存
  const { data: relatedData } = useRelatedSeries(seriesId || '');
  const relatedTotal = (relatedData?.works.length ?? 0) + (relatedData?.guess.length ?? 0);
  const relatedGuess = relatedData?.guess ?? [];
  // 头部元信息（追剧/播放/季徽/标签/备案号/简介，video_detail 接口），
  // 失败为 undefined：头部相应行不渲染，不打断页面
  const { data: meta } = useSeriesDetailMeta(seriesId);
  const historyItem = history?.items.find((i) => i.seriesId === seriesId);

  // 未上线剧（来源页带档案 prefill）：解析分集必然失败（平台无分集可给），
  // 合成一个 0 集档案走**正常渲染流**（详情页不分支，只是
  // 动作行换「即将上线 + 预约」、选集 tab 自然落在「暂无分集信息」）
  const upcomingSeries: Series | undefined =
    !series && isError && prefill
      ? {
          seriesId,
          title: prefill.title,
          cover: prefill.cover,
          episodeCount: 0,
          followedCnt: 0,
          tags: prefill.tags.split(',').filter(Boolean),
          episodes: [],
          dismissed: false,
        }
      : undefined;
  const activeSeries = series ?? upcomingSeries;
  const upcoming = upcomingSeries != null;

  const continueTarget = useMemo(
    () => (series ? resolveResumeTarget(series, localProgress, historyItem) : null),
    [series, localProgress, historyItem],
  );
  const continueIndex = continueTarget?.vidIndex ?? 0;

  const playEpisode = (idx: number) => {
    if (!seriesId) return;
    setResumeHint(null);
    playSeries(seriesId, idx);
  };

  const continuePlayback = () => {
    if (!continueTarget) {
      setDetailTab('episodes');
      toast.error(t('history.contentUnavailable'));
      return;
    }
    setResumeHint({ seriesId, ...continueTarget });
    playSeries(seriesId, continueTarget.vidIndex);
  };

  // ---- 互动（收藏/点赞）：登录后才可用，与播放器互动栏同一套 mutation ----
  const { data: account } = useAccount();
  const loggedIn = !!account;
  const { data: state } = useInteractionState();
  const { data: bookshelf } = useBookshelf();
  // 收藏态：互动回显列表是 best-effort（最近互动过的才有条目），
  // 书架列表才是权威——两处任一命中即已收藏
  const collected =
    (state?.items.find((i) => i.seriesId === seriesId)?.followed ?? false) ||
    (bookshelf?.some((b) => b.seriesId === seriesId) ?? false);
  const continueEp = series?.episodes.find((e) => e.vidIndex === continueIndex);
  const digged = state?.items.find((i) => i.vid === continueEp?.vid)?.userDigg ?? false;
  const digg = useVideoDigg();
  const collect = useSeriesCollect();

  const requireLogin = () => {
    toast.info(t('player.interact.loginRequired'));
    void navigate({ to: '/settings' });
  };

  const onCollect = () => {
    if (!loggedIn || !seriesId) return requireLogin();
    collect.mutate(
      { seriesId, collect: !collected },
      {
        onSuccess: () =>
          toast.success(t(collected ? 'player.interact.uncollected' : 'player.interact.collected')),
      },
    );
  };

  const onDigg = () => {
    if (!loggedIn) return requireLogin();
    if (!continueEp) return;
    digg.mutate(
      { vid: continueEp.vid, seriesId, digg: !digged },
      {
        onSuccess: () =>
          toast.success(t(digged ? 'player.interact.undone' : 'player.interact.liked')),
      },
    );
  };

  // ---- 剧评：**整部剧一条线**（group_type=1 形态，官方客户端同款）——
  //      单集评论在播放器的 💬 面板里，两套评论分属不同 group_id ----
  const { data: commentPages } = useSeriesComments(seriesId);
  const comments = commentPages?.pages.flatMap((p) => p.items) ?? [];
  const commentTotal = commentPages?.pages[0]?.total || comments.length;
  // 头部评分（第一页 extra 携带；空 = 暂无评分，整行不渲染）
  const reviewScore = commentPages?.pages[0]?.score || '';
  const reviewScoreCnt = commentPages?.pages[0]?.scoreCnt ?? 0;

  // 简介：meta 的 series_intro 优先；未上线合成档案回落 prefill 的描述
  const intro = meta?.intro ?? (upcoming ? (prefill?.desc ?? '') : '');

  // 未带 seriesId（直接敲路由）：只指路，不去解析
  if (!seriesId) {
    return (
      <div className="text-muted-foreground grid h-full place-items-center p-6 text-sm">
        {t('detail.noSeries')}
      </div>
    );
  }

  return (
    <div className="mx-auto w-full max-w-6xl px-4 py-4 pb-36 md:px-8 md:py-6 md:pb-6">
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

      {isPending && (
        <div className="mt-6 flex gap-4 md:gap-8">
          <Skeleton className="aspect-[3/4] w-28 shrink-0 rounded-xl md:w-44" />
          <div className="flex min-w-0 flex-1 flex-col gap-4">
            <Skeleton className="h-7 w-2/3" />
            <Skeleton className="h-4 w-1/3" />
            <Skeleton className="h-20 w-full" />
          </div>
        </div>
      )}

      {isError && !upcomingSeries && (
        <ErrorState
          message={t('series.loadFailed')}
          detail={error instanceof Error ? error.message : undefined}
        >
          <ResolveButton seriesId={seriesId} />
        </ErrorState>
      )}

      {activeSeries && (
        <>
          {/* 头部：海报+标题同行顶对齐；简介/出品方/开播钮全宽在下（官方同款，
              避免简介塞进右栏导致海报与标题视觉错位） */}
          <div className="mt-4 flex items-start gap-3 md:gap-8">
            <div className="bg-muted relative aspect-[3/4] w-28 shrink-0 overflow-hidden rounded-xl md:w-44">
              <SeriesCover cover={activeSeries.cover} alt={activeSeries.title} />
            </div>

            <div className="min-w-0 flex-1 pt-0.5">
              <h1 className="text-lg leading-snug font-bold md:text-2xl">{activeSeries.title}</h1>
              {(meta?.updateTag ||
                activeSeries.episodeCount > 0 ||
                (meta && meta.hotScore > 0)) && (
                <p className="text-muted-foreground mt-1.5 text-sm md:hidden">
                  {meta?.updateTag ||
                    tf('detail.allEpisodesBadge', { count: activeSeries.episodeCount })}
                  {meta && meta.hotScore > 0 && (
                    <>
                      {' · '}
                      {tf('detail.heatValue', { count: formatCountPrecise(meta.hotScore) })}
                    </>
                  )}
                </p>
              )}
              <div className="text-muted-foreground mt-2 hidden flex-wrap items-center gap-x-3 gap-y-1 text-sm md:flex">
                {reviewScore && (
                  <span className="flex items-baseline gap-1.5">
                    <span className="text-star text-xl leading-none font-bold">
                      {Number(reviewScore).toFixed(1)}
                    </span>
                    <span>
                      {t('detail.scoreUnit')}{' '}
                      {tf('detail.ratingCount', { count: formatCountPrecise(reviewScoreCnt) })}
                    </span>
                  </span>
                )}
                {meta && meta.hotScore > 0 && (
                  <span className="flex items-center gap-1">
                    <Flame className="text-hot size-4" aria-hidden />
                    <span>
                      {tf('detail.heatValue', { count: formatCountPrecise(meta.hotScore) })}
                    </span>
                  </span>
                )}
                {meta && meta.followedCnt > 0 && (
                  <span>
                    {tf('detail.followCount', { count: formatCountPrecise(meta.followedCnt) })}
                  </span>
                )}
                {meta && meta.playCnt > 0 && (
                  <span>
                    {tf('detail.playsCount', { count: formatCountPrecise(meta.playCnt) })}
                  </span>
                )}
              </div>

              {meta?.updateFrequency && (
                <div className="text-muted-foreground mt-1 text-sm">{meta.updateFrequency}</div>
              )}

              {(meta ? !!meta.season || meta.tags.length > 0 : activeSeries.tags.length > 0) ||
              activeSeries.episodes.length > 0 ||
              upcoming ? (
                <div className="mt-3 flex flex-wrap gap-1.5">
                  {meta?.season && (
                    <Badge variant="default" className="bg-hot text-primary-foreground">
                      {meta.season}
                    </Badge>
                  )}
                  {(upcoming || activeSeries.episodes.length > 0) && (
                    <Badge
                      variant={upcoming ? 'default' : 'secondary'}
                      className={upcoming ? 'bg-hot text-primary-foreground' : ''}
                    >
                      {meta?.updateTag ||
                        tf('detail.allEpisodesBadge', { count: activeSeries.episodes.length })}
                    </Badge>
                  )}
                  {(meta?.tags ?? activeSeries.tags).map((tag) => (
                    <Badge key={tag} variant="secondary">
                      {tag}
                    </Badge>
                  ))}
                </div>
              ) : null}
            </div>
          </div>

          {intro && (
            <div className="mt-5">
              <h2 className="mb-1.5 text-sm font-semibold">{t('detail.plotIntro')}</h2>
              <p
                className={cn(
                  'text-muted-foreground text-sm leading-relaxed',
                  !introExpanded && 'line-clamp-2',
                )}
              >
                {intro}
              </p>
              {!introExpanded && intro.length > 48 && (
                <button
                  type="button"
                  onClick={() => setIntroExpanded(true)}
                  className="text-primary mt-1 cursor-pointer text-sm"
                >
                  {t('player.introExpand')}
                </button>
              )}
            </div>
          )}

          {meta && meta.creators.length > 0 && (
            <div className="mt-4 flex flex-wrap items-center gap-3">
              {meta.creators.map((c) => (
                <button
                  key={c.userId + c.name}
                  type="button"
                  onClick={
                    c.userId
                      ? () =>
                          void navigate({
                            to: '/creator',
                            search: { userId: c.userId },
                          })
                      : undefined
                  }
                  disabled={!c.userId}
                  className={cn(
                    'flex items-center gap-2 rounded-full border py-1 pr-3 pl-1 transition-colors',
                    c.userId && 'hover:bg-sidebar-accent cursor-pointer',
                  )}
                  title={tf('detail.creatorHome', { name: c.name })}
                >
                  {c.avatar ? (
                    <img
                      src={c.avatar}
                      alt=""
                      className="size-7 rounded-full object-cover"
                      loading="lazy"
                    />
                  ) : (
                    <span className="bg-muted grid size-7 place-items-center rounded-full">
                      <UserRound className="text-muted-foreground size-4" aria-hidden />
                    </span>
                  )}
                  <span className="text-left leading-tight">
                    <span className="block text-xs font-medium">{c.name}</span>
                    {c.role && (
                      <span className="text-muted-foreground block text-[10px]">{c.role}</span>
                    )}
                  </span>
                </button>
              ))}
            </div>
          )}

          <div className="mt-5 flex flex-wrap items-center gap-2">
            {upcoming ? (
              <>
                <span className="text-hot flex items-center gap-1.5 text-sm font-medium">
                  <Hourglass className="size-4" aria-hidden />
                  {t('player.comingSoon')}
                </span>
                <ReserveButton seriesId={seriesId} />
              </>
            ) : (
              <>
                <Button size="sm" className="w-full md:w-auto" onClick={continuePlayback}>
                  <Play className="size-4" aria-hidden />
                  {!continueTarget
                    ? t('player.episodes')
                    : continueIndex > 1
                      ? tf('detail.continueEpisode', { index: continueIndex })
                      : t('detail.playFirst')}
                </Button>
                <Button
                  size="sm"
                  variant="outline"
                  onClick={onCollect}
                  className={cn('hidden md:inline-flex', collected && 'text-star')}
                >
                  <Star className={cn('size-4', collected && 'fill-star text-star')} aria-hidden />
                  {t(collected ? 'detail.collected' : 'detail.collect')}
                </Button>
                <Button
                  size="sm"
                  variant="outline"
                  onClick={onDigg}
                  className={cn('hidden md:inline-flex', digged && 'text-hot')}
                >
                  <Heart className={cn('size-4', digged && 'fill-hot text-hot')} aria-hidden />
                  {t('player.interact.like')}
                </Button>
                {meta?.seriesStatus === 0 && (
                  <span className="hidden md:inline-flex">
                    <FollowButton seriesId={seriesId} />
                  </span>
                )}
              </>
            )}
          </div>

          {/* 备案号：放在头部块之下、tab 之上（独立一行小字） */}
          {meta?.recordNumber && (
            <p className="text-muted-foreground/70 mt-4 text-xs">{meta.recordNumber}</p>
          )}

          {/* 选集 / 剧评 / 相关推荐。
              推荐 tab 每次被打开都换一批（官方客户端同款）：feed 游标前进一页，
              第一次打开除外——第一批本来就是新的。 */}
          <Tabs value={detailTab} onValueChange={setDetailTab} className="mt-8">
            <TabsList>
              <TabsTrigger value="episodes">
                {t('detail.tabEpisodes')}
                {(upcoming || activeSeries.episodes.length > 0) &&
                  ` ${activeSeries.episodes.length}`}
              </TabsTrigger>
              <TabsTrigger value="comments">
                {t('detail.tabComments')}
                {commentTotal > 0 && ` ${commentTotal}`}
              </TabsTrigger>
              <TabsTrigger value="recommend">
                {t('detail.tabRecommend')}
                {relatedTotal > 0 && ` ${relatedTotal}`}
              </TabsTrigger>
            </TabsList>

            <TabsContent value="episodes">
              {activeSeries.episodes.length === 0 ? (
                <Empty className="py-10">
                  <EmptyHeader>
                    <EmptyTitle className="text-muted-foreground text-sm font-normal">
                      {t('series.noEpisodes')}
                    </EmptyTitle>
                  </EmptyHeader>
                  {/* 未上线剧没有可解析的分集，重试无意义，只给文案 */}
                  {!upcoming && (
                    <EmptyContent>
                      <ResolveButton seriesId={seriesId} />
                    </EmptyContent>
                  )}
                </Empty>
              ) : (
                <>
                  {/* 窄屏：官方数字方格；宽屏：序号+第N集+时长卡片 */}
                  <div className="grid grid-cols-6 gap-2 md:hidden">
                    {activeSeries.episodes.map((ep) => (
                      <button
                        key={ep.vidIndex}
                        type="button"
                        onClick={() => playEpisode(ep.vidIndex)}
                        className={cn(
                          'relative grid aspect-square place-items-center rounded-lg text-sm tabular-nums transition-colors',
                          ep.vidIndex === continueIndex
                            ? 'bg-primary text-primary-foreground font-semibold'
                            : 'bg-muted text-foreground hover:bg-muted/80',
                        )}
                      >
                        {ep.vidIndex}
                      </button>
                    ))}
                  </div>
                  <div className="hidden grid-cols-2 gap-3 sm:grid-cols-3 md:grid lg:grid-cols-4 2xl:grid-cols-5">
                    {activeSeries.episodes.map((ep) => (
                      <button
                        key={ep.vidIndex}
                        type="button"
                        onClick={() => playEpisode(ep.vidIndex)}
                        className={cn(
                          'bg-card hover:border-foreground/30 flex cursor-pointer items-center gap-3 rounded-lg border px-4 py-2.5 text-left transition-colors',
                          ep.vidIndex === continueIndex && 'border-primary text-primary',
                        )}
                      >
                        <span className="w-8 shrink-0 text-center font-mono text-sm font-semibold">
                          {ep.vidIndex}
                        </span>
                        <span className="truncate text-sm">
                          {tf('player.epShort', { index: ep.vidIndex })}
                        </span>
                        {ep.duration > 0 && (
                          <span className="text-muted-foreground ml-auto shrink-0 text-xs tabular-nums">
                            {formatDuration(ep.duration)}
                          </span>
                        )}
                      </button>
                    ))}
                  </div>
                </>
              )}
            </TabsContent>

            <TabsContent value="comments">
              {/* 剧均评分块 + 发评框（评分/表情）+ 滚动懒加载列表，
                  整体在 ReviewList */}
              <ReviewList seriesId={seriesId} />
            </TabsContent>

            <TabsContent value="recommend">
              {/* 相关推荐 tab = plan 接口的两格（官方客户端同款）：
                  相关作品·系列 置顶，下面是 猜你喜欢；tab 计数也是两格之和。
                  猜你喜欢空了就整块不渲染，不塞别的内容。 */}
              <RelatedWorks works={relatedData?.works ?? []} />
              {relatedGuess.length > 0 && <GuessYouLike items={relatedGuess} />}
            </TabsContent>
          </Tabs>

          {/* 窄屏 sticky（BottomNav 上方）：收藏 | 点赞；剧评在 tab 里发 */}
          {!upcoming && (
            <div className="bg-background/95 supports-backdrop-filter:bg-background/80 fixed inset-x-0 bottom-14 z-30 flex gap-2 border-t px-4 py-2.5 backdrop-blur-sm md:hidden">
              <Button
                variant="outline"
                className={cn('h-10 flex-1', collected && 'text-star')}
                onClick={onCollect}
              >
                <Star className={cn('size-4', collected && 'fill-star text-star')} aria-hidden />
                {t(collected ? 'detail.collected' : 'detail.collect')}
              </Button>
              <Button
                variant="outline"
                className={cn('h-10 flex-1', digged && 'text-hot')}
                onClick={onDigg}
              >
                <Heart className={cn('size-4', digged && 'fill-hot text-hot')} aria-hidden />
                {t('player.interact.like')}
              </Button>
            </div>
          )}
        </>
      )}
    </div>
  );
}
