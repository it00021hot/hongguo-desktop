import { useMemo, useState } from 'react';
import { useNavigate, useRouter } from '@tanstack/react-router';
import { useQueryClient } from '@tanstack/react-query';
import { ArrowLeft, Bell, Heart, Play, Star } from 'lucide-react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import { Skeleton } from '@/components/ui/skeleton';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { SeriesCover } from '@/components/series-cover';
import { formatPlayCount } from '@/lib/format';
import {
  useAccount,
  useBookshelf,
  useInteractionState,
  useRelatedSeries,
  useReserveSeries,
  useResolveSeries,
  useSeriesCollect,
  useSeriesComments,
  useSeriesEpisodes,
  useSeriesExtras,
  useSeriesDetailMeta,
  useSeriesProgress,
  useVideoDigg,
  useWatchHistory,
} from '@/lib/queries';
import { usePlayerStore } from '@/lib/stores/player';
import { t, tf } from '@/i18n';
import { cn } from '@/lib/utils';
import type { RelatedItem } from '@/lib/schema';

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
  // 相关推荐总数（相关作品 + 猜你喜欢，plan 接口两格一并计）——tab 上的
  // 计数用；RelatedWorks 里还有一份同 key 的调用，react-query 共享缓存
  const { data: relatedData } = useRelatedSeries(seriesId || '');
  const relatedTotal = (relatedData?.works.length ?? 0) + (relatedData?.guess.length ?? 0);
  const relatedGuess = relatedData?.guess ?? [];
  // 头部元信息（追剧/播放/季徽/标签/备案号，video_detail 接口），
  // 失败为 undefined：头部相应行不渲染，不打断页面
  const { data: meta } = useSeriesDetailMeta(seriesId);
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
    const ratio = historyItem.durationMs > 0 ? historyItem.positionMs / historyItem.durationMs : 0;
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

  const [introExpanded, setIntroExpanded] = useState(false);
  const intro = extras?.intro ?? '';

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
                {reviewScore && (
                  <span className="flex items-baseline gap-1.5">
                    <span className="text-lg leading-none font-bold text-amber-400">
                      {Number(reviewScore).toFixed(1)}
                      <span className="ml-0.5 text-xs font-semibold">分</span>
                    </span>
                    {reviewScoreCnt > 0 && (
                      <span>
                        {tf('detail.ratingCount', { count: formatPlayCount(reviewScoreCnt) })}
                      </span>
                    )}
                  </span>
                )}
                {series.episodeCount > 0 && (
                  <span>
                    {reviewScore && <span className="mr-2">·</span>}
                    {tf('detail.episodesCount', { count: series.episodeCount })}
                  </span>
                )}
                {meta && meta.followedCnt > 0 && (
                  <span>
                    {series.episodeCount > 0 && <span className="mr-2">·</span>}
                    {tf('detail.followCount', { count: meta.followedCnt })}
                  </span>
                )}
                {meta && meta.playCnt > 0 && (
                  <span>
                    {(meta.followedCnt > 0 || series.episodeCount > 0) && (
                      <span className="mr-2">·</span>
                    )}
                    {formatPlayCount(meta.playCnt)}
                    {t('detail.plays')}
                  </span>
                )}
              </div>

              {/* 季徽（高亮）+ 题材标签（video_detail secondary_infos，
                  官方详情页同款行）；档案自带的 tags 是解析兜底，meta 优先 */}
              {(meta ? !!meta.season || meta.tags.length > 0 : series.tags.length > 0) ? (
                <div className="mt-3 flex flex-wrap gap-1.5">
                  {meta?.season && (
                    <Badge variant="default" className="text-primary-foreground bg-red-500">
                      {meta.season}
                    </Badge>
                  )}
                  {(meta?.tags ?? series.tags).map((tag) => (
                    <Badge key={tag} variant="secondary">
                      {tag}
                    </Badge>
                  ))}
                </div>
              ) : null}

              {meta?.recordNumber && (
                <p className="text-muted-foreground/70 mt-3 text-xs">{meta.recordNumber}</p>
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
                    className="text-muted-foreground hover:text-foreground shrink-0 cursor-pointer pt-0.5 text-xs"
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
              </div>
            </div>
          </div>

          {/* 选集 / 剧评 / 相关推荐。
              推荐 tab 每次被打开都换一批（第三方同款）：feed 游标前进一页，
              第一次打开除外——第一批本来就是新的。 */}
          <Tabs defaultValue="episodes" className="mt-8">
            <TabsList>
              <TabsTrigger value="episodes">
                {t('detail.tabEpisodes')}
                {series.episodes.length > 0 && ` ${series.episodes.length}`}
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
                      <span className="truncate text-sm">
                        {ep.title || tf('player.epShort', { index: ep.vidIndex })}
                      </span>
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
                <ul className="divide-border divide-y">
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
                          {c.replyCount > 0 && (
                            <span>{tf('detail.replyCount', { count: c.replyCount })}</span>
                          )}
                        </div>
                      </div>
                    </li>
                  ))}
                </ul>
              )}
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

/**
 * 相关作品·系列（官方 plan 接口第一格）：同系列各季（第1季/第2季…）
 * 与同 IP 作品横排卡片。没有相关作品就整块不渲染——它是增强项，
 * 不值得占一个错误位。
 */
function RelatedWorks({ works }: { works: RelatedItem[] }) {
  const navigate = useNavigate();
  if (works.length === 0) return null;

  const open = (id: string) => {
    // 同一路由换 search 参数：整页数据随之换挡
    void navigate({ to: '/detail', search: { seriesId: id } });
  };

  return (
    <div className="grid gap-3 pb-4">
      <h3 className="text-sm font-semibold">{t('detail.relatedWorks')}</h3>
      {/* items-start：卡片高度随两行/一行剧名浮动，行内不许互相拉伸 */}
      <div className="flex items-start gap-3 overflow-x-auto pb-2">
        {works.map((w) => (
          <RelatedCard key={w.seriesId} item={w} onOpen={open} />
        ))}
      </div>
    </div>
  );
}

/**
 * 猜你喜欢（官方 plan 接口第二格，第三方详情页同款）：响应式封面网格，
 * 卡片与相关作品同一套（角标 + 评分 + 剧名 + 集数/播放量）。
 */
function GuessYouLike({ items }: { items: RelatedItem[] }) {
  const navigate = useNavigate();
  const open = (id: string) => {
    void navigate({ to: '/detail', search: { seriesId: id } });
  };
  return (
    <div className="grid gap-3">
      <h3 className="text-sm font-semibold">{t('detail.guessYouLike')}</h3>
      <div className="grid grid-cols-[repeat(auto-fill,minmax(128px,1fr))] gap-3">
        {items.map((w) => (
          <RelatedCard key={w.seriesId} item={w} onOpen={open} className="w-full" />
        ))}
      </div>
    </div>
  );
}

/** 相关作品卡片：封面（角标 + 评分）+ 两行剧名 + 集数/播放量。 */
function RelatedCard({
  item,
  onOpen,
  className,
}: {
  item: RelatedItem;
  onOpen: (id: string) => void;
  /** 覆盖默认定宽（猜你喜欢网格里让卡片随格子伸缩） */
  className?: string;
}) {
  const isUpcoming = item.episodeCnt === 0 || item.tag === '即将上线';
  return (
    <button
      type="button"
      onClick={() => onOpen(item.seriesId)}
      className={cn('shrink-0 cursor-pointer text-left', className ?? 'w-32')}
      title={item.videoDesc || item.title}
    >
      {/* 封面盒：宽高全部钉死（w-32 × 3:4），图 object-cover 裁切——
          封面原始比例五花八门，绝不能让它撑盒子（一上一下就是这么来的） */}
      <div className="bg-muted relative aspect-[3/4] w-full overflow-hidden rounded-lg">
        {/* plan 接口的封面现已是 fqnovelpic HEIC 签名 URL（旧注释里的
            byteimg JPEG 不会再出现），直挂必裂，统一走 SeriesCover */}
        <SeriesCover seriesId={item.seriesId} cover={item.cover} alt={item.title} />
        {item.tag && (
          <span className="absolute top-1 left-1 rounded bg-black/50 px-1 py-0.5 text-[10px] leading-none text-white/95 backdrop-blur-[2px]">
            {item.tag}
          </span>
        )}
        {item.score > 0 && (
          <span className="absolute right-1 bottom-1 rounded bg-black/60 px-1 py-0.5 text-[10px] leading-none text-amber-300">
            {item.score.toFixed(1)}分
          </span>
        )}
      </div>
      <p className="mt-1.5 line-clamp-2 text-xs leading-snug">{item.title}</p>
      <p className="text-muted-foreground mt-0.5 truncate text-[11px]">
        {item.episodeCnt > 0
          ? tf('detail.episodesCount', { count: item.episodeCnt })
          : item.tag === '即将上线'
            ? item.tag
            : ''}
        {item.playCnt > 0 && ` · ${formatPlayCount(item.playCnt)}${t('detail.plays')}`}
      </p>
      {isUpcoming && <ReserveButton seriesId={item.seriesId} />}
    </button>
  );
}

/** 未上线剧集的预约按钮（第三方同款粉胶囊；已预约变描边，再点取消）。 */
function ReserveButton({ seriesId }: { seriesId: string }) {
  const reserve = useReserveSeries();
  const [reserved, setReserved] = useState(false);
  return (
    <button
      type="button"
      onClick={(e) => {
        e.stopPropagation(); // 别触发整卡跳详情
        const next = !reserved;
        reserve.mutate(
          { seriesId, reserve: next },
          {
            onSuccess: () => {
              setReserved(next);
              toast.success(t(next ? 'player.interact.reserved' : 'player.interact.unreserved'));
            },
            onError: (err) => toast.error(String(err)),
          },
        );
      }}
      disabled={reserve.isPending}
      className={cn(
        'mt-1.5 flex w-full cursor-pointer items-center justify-center gap-1 rounded-full py-1 text-xs font-medium transition-colors',
        reserved
          ? 'border border-red-400/60 text-red-400'
          : 'bg-red-500 text-white hover:bg-red-500/90',
      )}
    >
      {!reserved && <Bell className="size-3" aria-hidden />}
      {t(reserved ? 'player.reserved' : 'player.reserve')}
    </button>
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

/** 详情封面：统一走 SeriesCover（webp 增强 + 本地转码兜底 + 占位图）。 */
function DetailCover({ seriesId, cover, name }: { seriesId: string; cover: string; name: string }) {
  return <SeriesCover seriesId={seriesId} cover={cover} alt={name} />;
}
