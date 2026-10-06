import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useNavigate, useRouter } from '@tanstack/react-router';
import { useQueryClient } from '@tanstack/react-query';
import { ArrowLeft, Heart, Play, RefreshCw, Star, Tv } from 'lucide-react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import { Skeleton } from '@/components/ui/skeleton';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { FeedCardGrid } from '@/features/feed/components/feed-card-grid';
import {
  isRenderableCover,
  useAccount,
  useComments,
  useFeed,
  useInteractionState,
  useRank,
  useResolveSeries,
  useSeriesCollect,
  useSeriesEpisodes,
  useSeriesExtras,
  useSeriesProgress,
  useVideoDigg,
  useWatchHistory,
  useWebCover,
} from '@/lib/queries';
import { usePlayerStore } from '@/lib/stores/player';
import { t, tf } from '@/i18n';
import { cn } from '@/lib/utils';
import type { FeedItem, RankItem, RecommendItem } from '@/lib/schema';

/**
 * 剧集详情页（/detail?seriesId=…）。
 *
 * 播放器里点剧名进来：封面/统计/标签/简介 + 继续看 + 选集网格 + 剧评 +
 * 相关推荐。数据全部来自既有查询（分集档案 / extras / 观看历史 / 评论），
 * 不新增后端命令；「继续看第 N 集」的 N 从云端观看历史推。
 */
export function SeriesDetailPage({ seriesId }: { seriesId: string }) {
  const navigate = useNavigate();
  const router = useRouter();
  const setTarget = usePlayerStore((s) => s.setTarget);

  const { data: series, isPending, isError, error } = useSeriesEpisodes(seriesId || null);
  const { data: extras } = useSeriesExtras(seriesId ?? '');
  const { data: localProgress } = useSeriesProgress(seriesId ?? '');
  const { data: history } = useWatchHistory();
  const historyItem = history?.items.find((i) => i.seriesId === seriesId);

  /**
   * 「继续看」的集号。本地 playback 表是第一真值（播放期间 5 秒一写，
   * 切集立即更新）；云端观看历史既滞后（约 1 分钟一报）又有查询缓存，
   * 只做本地没记录时的兜底——否则就会出现「都看到第二集了还停在第一集」。
   * 看完（≥95%，与后端 is_near_end 同口径）指到下一集，没有下一集回第 1 集。
   */
  const continueIndex = useMemo(() => {
    if (localProgress) {
      const ratio =
        localProgress.duration > 0 ? localProgress.currentTime / localProgress.duration : 0;
      if (ratio >= 0.95) {
        const hasNext = series?.episodes.some((e) => e.vidIndex === localProgress.vidIndex + 1);
        return hasNext ? localProgress.vidIndex + 1 : 1;
      }
      return localProgress.vidIndex;
    }
    if (!historyItem || historyItem.positionMs <= 0) return 1;
    const ratio =
      historyItem.durationMs > 0 ? historyItem.positionMs / historyItem.durationMs : 0;
    return ratio >= 0.95 ? 1 : historyItem.vidIndex;
  }, [localProgress, historyItem, series]);

  const playEpisode = (idx: number) => {
    if (!seriesId) return;
    setTarget(seriesId, idx);
    void navigate({ to: '/player' });
  };

  // ---- 互动（收藏/点赞）：登录后才可用，与播放器互动栏同一套 mutation ----
  const { data: account } = useAccount();
  const loggedIn = !!account;
  const { data: state } = useInteractionState();
  const collected = state?.items.find((i) => i.seriesId === seriesId)?.followed ?? false;
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
        onSuccess: () => toast.success(t(digged ? 'player.interact.undone' : 'player.interact.liked')),
        onError: (e) => toast.error(String(e)),
      },
    );
  };

  // ---- 剧评：跟着「继续看」那一集走（评论按集组织，与播放器评论区同源） ----
  const commentVid = continueEp ? `${continueEp.vid}:${seriesId}` : '';
  const { data: commentPages } = useComments(commentVid);
  const comments = commentPages?.pages.flatMap((p) => p.items) ?? [];
  const commentTotal = commentPages?.pages[0]?.total || comments.length;

  const [introExpanded, setIntroExpanded] = useState(false);
  const intro = extras?.intro ?? '';

  // ---- 推荐 tab：每次打开换一批（feed 游标前进），换一换同一动作 ----
  /** feed 窗口游标（与首页沉浸流共享同一份推荐流缓存） */
  const [feedCursor, setFeedCursor] = useState(0);
  const recEverOpened = useRef(false);
  const advanceFeed = useCallback(() => setFeedCursor((c) => c + REC_PAGE), []);

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

      {isError && (
        <div className="grid gap-3 py-16 text-center">
          <p className="text-destructive text-sm">
            {t('series.loadFailed')}
            {error instanceof Error && `: ${error.message}`}
          </p>
          <ResolveButton seriesId={seriesId} />
        </div>
      )}

      {series && (
        <>
          {/* 头部：封面 + 档案。对齐官网详情页的构图，数据全走本地档案/extras */}
          <div className="mt-4 flex gap-8">
            <div className="bg-muted relative aspect-[3/4] w-44 shrink-0 overflow-hidden rounded-xl">
              <DetailCover seriesId={seriesId} cover={series.cover} name={series.title} />
            </div>

            <div className="min-w-0 flex-1">
              <h1 className="text-2xl font-bold">{series.title}</h1>
              <div className="text-muted-foreground mt-2 flex flex-wrap items-center gap-2 text-sm">
                {series.followedCnt > 0 && (
                  <span>{tf('detail.followCount', { count: series.followedCnt })}</span>
                )}
                {series.followedCnt > 0 && series.episodeCount > 0 && <span>·</span>}
                {series.episodeCount > 0 && (
                  <span>{tf('detail.episodesCount', { count: series.episodeCount })}</span>
                )}
              </div>

              {series.tags.length > 0 && (
                <div className="mt-3 flex flex-wrap gap-1.5">
                  {series.tags.map((tag) => (
                    <Badge key={tag} variant="secondary">
                      {tag}
                    </Badge>
                  ))}
                </div>
              )}

              {intro && (
                <div className="mt-4 flex items-start gap-3">
                  <p
                    role="button"
                    tabIndex={0}
                    onClick={() => setIntroExpanded((v) => !v)}
                    onKeyDown={(e) => {
                      if (e.key === 'Enter' || e.key === ' ') setIntroExpanded((v) => !v);
                    }}
                    className={cn(
                      'text-muted-foreground min-w-0 flex-1 cursor-pointer text-sm leading-relaxed',
                      !introExpanded && 'line-clamp-3',
                    )}
                  >
                    {intro}
                  </p>
                  <button
                    type="button"
                    onClick={() => setIntroExpanded((v) => !v)}
                    className="text-muted-foreground shrink-0 cursor-pointer pt-0.5 text-xs hover:text-foreground"
                  >
                    {introExpanded ? t('player.introCollapse') : t('player.introExpand')}
                  </button>
                </div>
              )}

              <div className="mt-5 flex flex-wrap items-center gap-2">
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
                  <Star className={cn('size-4', collected && 'fill-amber-400 text-amber-400')} aria-hidden />
                  {t(collected ? 'detail.collected' : 'detail.collect')}
                </Button>
                <Button
                  size="sm"
                  variant="outline"
                  onClick={onDigg}
                  className={cn(digged && 'text-red-500')}
                >
                  <Heart className={cn('size-4', digged && 'fill-red-500 text-red-500')} aria-hidden />
                  {t('player.interact.like')}
                </Button>
              </div>
            </div>
          </div>

          {/* 选集 / 剧评 / 相关推荐。
              推荐 tab 每次被打开都换一批（第三方同款）：feed 游标前进一页，
              第一次打开除外——第一批本来就是新的。 */}
          <Tabs
            defaultValue="episodes"
            onValueChange={(v) => {
              if (v !== 'recommend') return;
              if (recEverOpened.current) advanceFeed();
              else recEverOpened.current = true;
            }}
            className="mt-8"
          >
            <TabsList>
              <TabsTrigger value="episodes">
                {t('detail.tabEpisodes')}
                {series.episodes.length > 0 && ` ${series.episodes.length}`}
              </TabsTrigger>
              <TabsTrigger value="comments">
                {t('detail.tabComments')}
                {commentTotal > 0 && ` ${commentTotal}`}
              </TabsTrigger>
              <TabsTrigger value="recommend">{t('detail.tabRecommend')}</TabsTrigger>
            </TabsList>

            <TabsContent value="episodes">
              {series.episodes.length === 0 ? (
                <div className="grid gap-3 py-10 text-center">
                  <p className="text-muted-foreground text-sm">{t('series.noEpisodes')}</p>
                  <ResolveButton seriesId={seriesId} />
                </div>
              ) : (
                <div className="grid grid-cols-2 gap-3 sm:grid-cols-3 lg:grid-cols-4 2xl:grid-cols-5">
                  {series.episodes.map((ep) => (
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
                      <span className="truncate text-sm">{ep.title || tf('player.epShort', { index: ep.vidIndex })}</span>
                    </button>
                  ))}
                </div>
              )}
            </TabsContent>

            <TabsContent value="comments">
              {comments.length === 0 ? (
                <p className="text-muted-foreground py-10 text-center text-sm">
                  {t('detail.commentsEmpty')}
                </p>
              ) : (
                <ul className="divide-y divide-border">
                  {comments.map((c) => (
                    <li key={c.commentId} className="flex gap-3 py-4">
                      <div className="bg-muted grid size-9 shrink-0 place-items-center overflow-hidden rounded-full text-xs">
                        {c.avatar ? (
                          <img src={c.avatar} alt="" className="size-full object-cover" />
                        ) : (
                          (c.userName[0] ?? '?')
                        )}
                      </div>
                      <div className="min-w-0 flex-1">
                        <div className="flex items-baseline gap-2">
                          <span className="truncate text-sm font-medium">{c.userName}</span>
                          <span className="text-muted-foreground shrink-0 text-xs">
                            {new Date(c.createTime * 1000).toLocaleDateString()}
                          </span>
                        </div>
                        <p className="mt-1 text-sm leading-relaxed break-words whitespace-pre-wrap">
                          {c.text}
                        </p>
                        <div className="text-muted-foreground mt-1 flex gap-4 text-xs">
                          <span>♥ {c.diggCount}</span>
                          {c.replyCount > 0 && <span>{tf('detail.replyCount', { count: c.replyCount })}</span>}
                        </div>
                      </div>
                    </li>
                  ))}
                </ul>
              )}
            </TabsContent>

            <TabsContent value="recommend">
              <RecommendSection
                siteItems={extras?.recommendations ?? []}
                feedCursor={feedCursor}
                onShuffle={advanceFeed}
              />
            </TabsContent>
          </Tabs>
        </>
      )}
    </div>
  );
}

/** 推荐一页的条数：详情页网格一屏正好放下的量，换一换就是整屏换血。 */
const REC_PAGE = 12;

/** 榜单条目 → 信息流卡片形状（FeedCardGrid 只吃这个）。 */
function rankToFeedItem(r: RankItem): FeedItem {
  return {
    seriesId: r.seriesId,
    title: r.title,
    cover: r.cover,
    horizCover: '',
    vid: r.vid,
    episodeCnt: r.episodeCnt,
    playCnt: r.playCnt,
    commentCount: 0,
    score: r.score,
    tags: r.tags,
    contentType: 0,
  };
}

/** 官网静态推荐 → 信息流卡片形状。 */
function siteToFeedItem(r: RecommendItem): FeedItem {
  return {
    seriesId: r.seriesId,
    title: r.seriesName,
    cover: r.seriesCover,
    horizCover: '',
    vid: '',
    episodeCnt: r.episodeCount,
    playCnt: 0,
    commentCount: 0,
    score: 0,
    tags: [],
    contentType: 0,
  };
}

/**
 * 推荐 tab 的三源切换（第三方同款形态）：
 *
 * - **为你推荐**：官方推荐流（与首页沉浸流共享缓存），窗口游标翻页，
 *   「换一换」与「每次打开 tab」都前进一批——永远有没看过的剧；
 * - **热播榜**：rank 热榜子榜，带名次角标；
 * - **官网推荐**：详情页底部的静态相关推荐（extras），同剧固定。
 */
function RecommendSection({
  siteItems,
  feedCursor,
  onShuffle,
}: {
  siteItems: RecommendItem[];
  feedCursor: number;
  onShuffle: () => void;
}) {
  const navigate = useNavigate();
  const feed = useFeed(undefined);
  const rank = useRank('all', 'ranklist_hot_sc', '');
  const [source, setSource] = useState<'feed' | 'rank' | 'site'>('feed');

  const feedItems = feed.items.slice(feedCursor, feedCursor + REC_PAGE);

  // 窗口接近尾部自动续拉推荐流（与首页沉浸流同一模式）
  useEffect(() => {
    if (source !== 'feed') return;
    if (feed.hasMore && !feed.isFetchingMore && feed.items.length - (feedCursor + REC_PAGE) <= 3) {
      feed.loadMore();
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [source, feedCursor, feed.items.length, feed.hasMore, feed.isFetchingMore]);

  const openDetail = (seriesId: string) => {
    // 同一路由换 search 参数：整页数据随之换挡
    void navigate({ to: '/detail', search: { seriesId } });
  };

  const sourcePill = (id: 'feed' | 'rank' | 'site', label: string) => (
    <button
      key={id}
      type="button"
      onClick={() => setSource(id)}
      className={cn(
        'cursor-pointer rounded-full border px-3 py-1 text-xs transition-colors',
        source === id
          ? 'bg-primary text-primary-foreground border-primary'
          : 'text-muted-foreground hover:bg-accent',
      )}
    >
      {label}
    </button>
  );

  return (
    <div className="grid gap-4">
      <div className="flex flex-wrap items-center gap-2">
        {sourcePill('feed', t('detail.recFeed'))}
        {sourcePill('rank', t('detail.recRank'))}
        {sourcePill('site', t('detail.recSite'))}
        {source === 'feed' && (
          <Button
            variant="outline"
            size="sm"
            className="ml-auto gap-1.5"
            onClick={onShuffle}
            disabled={feed.isFetchingMore}
          >
            <RefreshCw className={cn('size-3.5', feed.isFetchingMore && 'animate-spin')} aria-hidden />
            {t('detail.recShuffle')}
          </Button>
        )}
      </div>

      {source === 'feed' &&
        (feedItems.length > 0 ? (
          <FeedCardGrid items={feedItems} onSelect={(item) => openDetail(item.seriesId)} />
        ) : (
          <p className="text-muted-foreground py-10 text-center text-sm">
            {feed.isLoading ? t('common.loading') : t('detail.recommendEmpty')}
          </p>
        ))}

      {source === 'rank' && (
        <FeedCardGrid
          items={(rank.data?.items ?? []).map(rankToFeedItem)}
          ranked
          onSelect={(item) => openDetail(item.seriesId)}
        />
      )}

      {source === 'site' &&
        (siteItems.length > 0 ? (
          <FeedCardGrid
            items={siteItems.map(siteToFeedItem)}
            onSelect={(item) => openDetail(item.seriesId)}
          />
        ) : (
          <p className="text-muted-foreground py-10 text-center text-sm">
            {t('detail.recommendEmpty')}
          </p>
        ))}
    </div>
  );
}

/** 解析按钮（档案缺失/无分集时的出口）。解析完失效本页两份缓存，就地换挡。 */
function ResolveButton({ seriesId }: { seriesId: string }) {
  const qc = useQueryClient();
  const resolve = useResolveSeries();
  return (
    <Button
      variant="outline"
      size="sm"
      className="mx-auto"
      disabled={resolve.isPending}
      onClick={() =>
        resolve.mutate(seriesId, {
          onSuccess: () => {
            void qc.invalidateQueries({ queryKey: ['series-episodes', seriesId] });
            void qc.invalidateQueries({ queryKey: ['series-extras', seriesId] });
            toast.success(t('detail.resolved'));
          },
          onError: (e) => toast.error(e.message),
        })
      }
    >
      {t('series.resolveAgain')}
    </Button>
  );
}

/**
 * 详情封面：与列表卡片同一套渲染口径——源图渲染不了（HEIC）先换官网 webp，
 * 再不行落 hongguo-cover 本地转码；两条路都没有就先占位，不挂必然裂图的 img。
 */
function DetailCover({
  seriesId,
  cover,
  name,
}: {
  seriesId: string;
  cover: string;
  name: string;
}) {
  const { data: webCover } = useWebCover(seriesId, cover);
  const src = webCover ?? (isRenderableCover(cover) ? cover : '');
  return src ? (
    <img src={src} alt={name} className="size-full object-cover" />
  ) : (
    <div className="text-muted-foreground grid size-full place-items-center">
      <Tv className="size-8" aria-hidden />
    </div>
  );
}
