import { useMemo } from 'react';
import { useNavigate, useRouter } from '@tanstack/react-router';
import { ArrowLeft, Flame, Heart, Hourglass, Play, Star } from 'lucide-react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import { Skeleton } from '@/components/ui/skeleton';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { SeriesCover } from '@/components/series-cover';
import { GuessYouLike } from './guess-you-like';
import { RelatedWorks } from './related-works';
import { ReserveButton } from './reserve-button';
import { ResolveButton } from './resolve-button';
import { ReviewList } from './review-list';
import { formatCountPrecise, formatDuration } from '@/utils/format';
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
import { usePlayerStore } from '@/stores/player';
import { t, tf } from '@/locales';
import { cn } from '@/lib/utils';
import type { Series } from '@/service/schema';

/**
 * 剧集详情页（/detail?seriesId=…）。
 *
 * 播放器里点剧名进来：封面/统计/标签/简介 + 继续看 + 选集网格 + 剧评 +
 * 相关推荐。数据全部来自官方 App 接口查询（分集档案 / meta / 观看历史 /
 * 评论 / plan 相关推荐）；「继续看第 N 集」的 N 从云端观看历史推。
 *
 * `prefill`：来源页（榜单行）的档案快照。未上线剧解析分集必然失败，
 * 有 prefill 时错误分支渲染「即将上线」降级视图（档案 + 预约按钮）。
 */
export function SeriesDetailPage({
  seriesId,
  prefill,
}: {
  seriesId: string;
  prefill?: { title: string; cover: string; tags: string; desc: string };
}) {
  const navigate = useNavigate();
  const router = useRouter();
  const setTarget = usePlayerStore((s) => s.setTarget);

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
  // 合成一个 0 集档案走**正常渲染流**（hgplayer 同构：详情页不分支，只是
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

  /**
   * 「继续看」的集号。本地 playback 表是第一真值（播放期间 5 秒一写，
   * 切集立即更新）；云端观看历史既滞后（约 1 分钟一报）又有查询缓存，
   * 只做本地没记录时的兜底——否则就会出现「都看到第二集了还停在第一集」。
   * 看过 >3 秒就续播那一集的那个位置，不看片尾（2026-10-09 用户口径，
   * 与后端 resumeAt 同口径：位置 ≤3 秒视为没看过，从头播）。
   */
  const continueIndex = useMemo(() => {
    if (localProgress && localProgress.currentTime > 3) return localProgress.vidIndex;
    if (historyItem && historyItem.positionMs > 3000) return Math.max(1, historyItem.vidIndex);
    if (localProgress) return localProgress.vidIndex;
    return 1;
  }, [localProgress, historyItem]);

  const playEpisode = (idx: number) => {
    if (!seriesId) return;
    setTarget(seriesId, idx);
    void navigate({ to: '/player' });
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
        onError: (e) => toast.error(String(e)),
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
        onError: (e) => toast.error(String(e)),
      },
    );
  };

  // ---- 剧评：**整部剧一条线**（group_type=1 形态，第三方同款）——
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

      {isPending && (
        <div className="mt-6 flex gap-8">
          <Skeleton className="aspect-[3/4] w-44 shrink-0 rounded-xl" />
          <div className="flex-1 space-y-4">
            <Skeleton className="h-7 w-2/3" />
            <Skeleton className="h-4 w-1/3" />
            <Skeleton className="h-20 w-full" />
          </div>
        </div>
      )}

      {isError && !upcomingSeries && (
        <div className="grid gap-3 py-16 text-center">
          <p className="text-destructive text-sm">
            {t('series.loadFailed')}
            {error instanceof Error && `: ${error.message}`}
          </p>
          <ResolveButton seriesId={seriesId} />
        </div>
      )}

      {activeSeries && (
        <>
          {/* 头部：封面 + 档案。对齐第三方详情页构图，数据全走 App 接口 */}
          <div className="mt-4 flex gap-8">
            <div className="bg-muted relative aspect-[3/4] w-44 shrink-0 overflow-hidden rounded-xl">
              <DetailCover cover={activeSeries.cover} name={activeSeries.title} />
            </div>

            <div className="min-w-0 flex-1">
              <h1 className="text-2xl font-bold">{activeSeries.title}</h1>
              {/* 统计行，排版对齐第三方：评分 评分人数 → 红果热度值 → 追剧 → 播放。
                  评分来自剧评接口 extra（credibility_score），热度来自 video_detail
                  （hot_score），两者缺失时该段不渲染不打断行 */}
              <div className="text-muted-foreground mt-2 flex flex-wrap items-center gap-x-3 gap-y-1 text-sm">
                {reviewScore && (
                  <span className="flex items-baseline gap-1.5">
                    <span className="text-xl leading-none font-bold text-amber-400">
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
                    <Flame className="size-4 text-red-500" aria-hidden />
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

              {/* 季徽（高亮）+「全 N 集」+ 题材标签——行构成与第三方一致；
                  题材来自 video_detail secondary_infos，档案自带 tags 是解析兜底。
                  未上线剧红底「全 0 集」（hgplayer 同款，0 集也要亮出来） */}
              {(meta ? !!meta.season || meta.tags.length > 0 : activeSeries.tags.length > 0) ||
              activeSeries.episodes.length > 0 ||
              upcoming ? (
                <div className="mt-3 flex flex-wrap gap-1.5">
                  {meta?.season && (
                    <Badge variant="default" className="text-primary-foreground bg-red-500">
                      {meta.season}
                    </Badge>
                  )}
                  {(upcoming || activeSeries.episodes.length > 0) && (
                    <Badge
                      variant={upcoming ? 'default' : 'secondary'}
                      className={upcoming ? 'text-primary-foreground bg-red-500' : ''}
                    >
                      {tf('detail.allEpisodesBadge', { count: activeSeries.episodes.length })}
                    </Badge>
                  )}
                  {(meta?.tags ?? activeSeries.tags).map((tag) => (
                    <Badge key={tag} variant="secondary">
                      {tag}
                    </Badge>
                  ))}
                </div>
              ) : null}

              {/* 简介常驻全文（无收起/展开） */}
              {intro && (
                <p className="text-muted-foreground mt-4 text-sm leading-relaxed">{intro}</p>
              )}

              <div className="mt-5 flex flex-wrap items-center gap-2">
                {upcoming ? (
                  /* 未上线：动作行换「⏳ 即将上线 + 预约」（hgplayer 同款），
                     播放/收藏/点赞无从谈起 */
                  <>
                    <span className="flex items-center gap-1.5 text-sm font-medium text-red-500">
                      <Hourglass className="size-4" aria-hidden />
                      {t('player.comingSoon')}
                    </span>
                    <ReserveButton seriesId={seriesId} />
                  </>
                ) : (
                  <>
                    <Button size="sm" onClick={() => playEpisode(continueIndex)}>
                      <Play className="size-4" aria-hidden />
                      {continueIndex > 1
                        ? tf('detail.continueEpisode', { index: continueIndex })
                        : t('detail.playFirst')}
                    </Button>
                    <Button
                      size="sm"
                      variant="outline"
                      onClick={onCollect}
                      className={cn(collected && 'text-amber-500')}
                    >
                      <Star
                        className={cn('size-4', collected && 'fill-amber-400 text-amber-400')}
                        aria-hidden
                      />
                      {t(collected ? 'detail.collected' : 'detail.collect')}
                    </Button>
                    <Button
                      size="sm"
                      variant="outline"
                      onClick={onDigg}
                      className={cn(digged && 'text-red-500')}
                    >
                      <Heart
                        className={cn('size-4', digged && 'fill-red-500 text-red-500')}
                        aria-hidden
                      />
                      {t('player.interact.like')}
                    </Button>
                  </>
                )}
              </div>
            </div>
          </div>

          {/* 备案号：第三方放在头部块之下、tab 之上（独立一行小字） */}
          {meta?.recordNumber && (
            <p className="text-muted-foreground/70 mt-4 text-xs">{meta.recordNumber}</p>
          )}

          {/* 选集 / 剧评 / 相关推荐。
              推荐 tab 每次被打开都换一批（第三方同款）：feed 游标前进一页，
              第一次打开除外——第一批本来就是新的。 */}
          <Tabs defaultValue="episodes" className="mt-8">
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
                <div className="grid gap-3 py-10 text-center">
                  <p className="text-muted-foreground text-sm">{t('series.noEpisodes')}</p>
                  {/* 未上线剧没有可解析的分集，重试无意义，只给文案 */}
                  {!upcoming && <ResolveButton seriesId={seriesId} />}
                </div>
              ) : (
                <div className="grid grid-cols-2 gap-3 sm:grid-cols-3 lg:grid-cols-4 2xl:grid-cols-5">
                  {/* 选集格对齐第三方：序号 + 第 N 集 + 右侧时长角标。
                      接口的 title 是整句剧情简介（不是集名），照排会挤成一团，
                      第三方同款做法是统一显示「第 N 集」 */}
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
              )}
            </TabsContent>

            <TabsContent value="comments">
              {/* 剧均评分块 + 发评框（评分/表情）+ 滚动懒加载列表，
                  整体在 ReviewList（2026-10-10 抓包对齐 hgplayer 剧评页） */}
              <ReviewList seriesId={seriesId} />
            </TabsContent>

            <TabsContent value="recommend">
              {/* 相关推荐 tab = plan 接口的两格（第三方同款）：
                  相关作品·系列 置顶，下面是 猜你喜欢；tab 计数也是两格之和。
                  猜你喜欢空了就整块不渲染（hgplayer 同款），不塞别的内容。 */}
              <RelatedWorks works={relatedData?.works ?? []} />
              {relatedGuess.length > 0 && <GuessYouLike items={relatedGuess} />}
            </TabsContent>
          </Tabs>
        </>
      )}
    </div>
  );
}

/** 详情封面：统一走 SeriesCover（本地转码代理 + 占位图）。 */
function DetailCover({ cover, name }: { cover: string; name: string }) {
  return <SeriesCover cover={cover} alt={name} />;
}
