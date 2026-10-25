/**
 * TanStack Query 的 key 工厂 + 无限滚动通用封装（各查询域共用）。
 *
 * 所有 key 必须从这里出，不要在组件里手写字符串——否则失效（invalidate）
 * 时容易漏掉某个 key，导致界面不刷新。
 */
import { useCallback, useMemo, useState } from 'react';
import type { BrowseFilters } from '../schema';

/** 整组失效用根 key 的域段（与下方同名工厂同源，杜绝双轨字面量）。 */
const RANK = ['rank'] as const;
const RESERVATIONS = ['reservations'] as const;

/** 抖动区间下限（毫秒）。 */
const FUZZY_STALE_MIN_MS = 10_000;
/** 抖动区间宽度（毫秒）：值域 [10s, 10s+20s)。 */
const FUZZY_STALE_SPAN_MS = 20_000;

/**
 * 白名单外查询的统一节流区间：10-30s、按 query key 哈希派生抖动。
 * 同批挂载的查询过期时刻彼此错开（避免集体过期→集体重拉的请求风暴），
 * 同一 key 跨渲染/重启稳定（不会因重渲染漂移）。白名单：账号（会话
 * 不失效一直用缓存）、媒体/封面、本地真值（设置/能力探测/播放进度）、
 * 以及刻意 staleTime 0 的社交态——它们不走本函数。
 */
export function fuzzyStaleTime(key: readonly unknown[]): number {
  let h = 7;
  for (const part of key) {
    const s = typeof part === 'string' ? part : JSON.stringify(part);
    for (const c of s) h = (h * 31 + c.charCodeAt(0)) | 0;
  }
  return FUZZY_STALE_MIN_MS + (Math.abs(h) % FUZZY_STALE_SPAN_MS);
}

export const keys = {
  settings: ['settings'] as const,
  /** 缓存目录留空时的默认位置（进程内不变，设置页只在空态展示） */
  defaultCacheDir: ['default-cache-dir'] as const,
  feed: (tab: string) => ['feed', tab] as const,
  /** 首页顶栏 tab 列表（服务端下发；登录态变化时壳侧过滤，不换 key） */
  feedTabs: ['feed-tabs'] as const,
  newDrama: (gender: number) => ['new-drama', gender] as const,
  /** 分集表（选集/详情/播放器）：在线现拉，失败回落快照。 */
  seriesEpisodes: (id: string) => ['series-episodes', id] as const,
  /** 本地快照（列表页封面/标题）：零网络读档，与分集表分 key。 */
  seriesSnapshot: (id: string) => ['series-snapshot', id] as const,
  seriesProgress: (id: string) => ['series-progress', id] as const,
  watchHistory: ['watch-history'] as const,
  localHistory: ['local-history'] as const,
  tasks: ['download-tasks'] as const,
  queueStatus: ['queue-status'] as const,
  mergeTasks: ['merge-tasks'] as const,
  mergeCandidates: ['merge-candidates'] as const,
  mergePreflight: (id: string) => ['merge-preflight', id] as const,
  storageUsage: ['storage-usage'] as const,
  storageSeries: ['storage-series'] as const,
  cacheUsage: ['cache-usage'] as const,
  capability: ['decode-capability'] as const,
  playbackCapability: ['playback-capability'] as const,
  browsePanel: ['browse-panel'] as const,
  browseFeed: (filters: BrowseFilters) => ['browse-feed', filters] as const,
  relatedSeries: (seriesId: string) => ['related-series', seriesId] as const,
  seriesMeta: (seriesId: string) => ['series-meta', seriesId] as const,
  seriesComments: (seriesId: string) => ['series-comments', seriesId] as const,
  /** 单集评论的回复列表（展开「N 条回复」时按需拉取） */
  commentReplies: (vid: string, commentId: string) => ['comment-replies', vid, commentId] as const,
  /** 剧评的回复列表 */
  reviewReplies: (seriesId: string, commentId: string) =>
    ['review-replies', seriesId, commentId] as const,
  danmaku: (vid: string) => ['danmaku', vid] as const,
  comments: (vid: string) => ['comments', vid] as const,
  /** 评论 @ 提及候选（登录态，含官方 AI bot 小果） */
  mentionList: (seriesId: string) => ['mention-list', seriesId] as const,
  /** 第三方表情库（GIF tab） */
  emoticonsThirdParty: ['emoticons', 'third-party'] as const,
  /** 我上传的自定义表情（♡ tab） */
  emoticonsOwn: ['emoticons', 'own'] as const,
  /** 表情关键词搜索结果 */
  emoticonSearch: (keyword: string) => ['emoticons', 'search', keyword] as const,
  /** 创作者主页（详情页点创作者进入：资料头 + 作品列表） */
  creatorProfile: (userId: string) => ['creator-profile', userId] as const,
  creatorWorks: (userId: string) => ['creator-works', userId] as const,
  interactState: ['interact-state'] as const,
  bookshelf: ['bookshelf'] as const,
  /** 追更整组失效用根 key（追更/取消后列表与「是否已追」一起翻面） */
  followRoot: ['follow-updates'] as const,
  followUpdates: (cursor: string) => ['follow-updates', cursor] as const,
  isFollowing: (seriesId: string) => ['is-following', seriesId] as const,
  followNotices: ['follow-notices'] as const,
  /** 榜单整组失效用根 key（如预约态就地翻新时 findAll 前缀匹配） */
  rankRoot: RANK,
  rank: (selected: string, sub: string, panel: string, background: string) =>
    [...RANK, selected, sub, panel, background] as const,
  newCalendar: (date: string) => ['new-calendar', date] as const,
  /** 预约整组失效用根 key（登录/预约变更后两个 tab 一起翻面） */
  reservationsRoot: RESERVATIONS,
  reservations: (isOnline: boolean) => [...RESERVATIONS, isOnline] as const,
  account: ['account'] as const,
  appSeriesSearch: (query: string) => ['app-series-search', query] as const,
  searchSuggest: (kw: string) => ['search-suggest', kw] as const,
} satisfies Record<string, unknown>;

/** 无限滚动查询的最小结构面（只取页面消费的字段，避免深泛型签名）。 */
interface InfiniteStream<TPage> {
  data: { pages: TPage[] } | undefined;
  error: Error | null;
  hasNextPage: boolean;
  isPending: boolean;
  isFetching: boolean;
  isFetchingNextPage: boolean;
  /** 最近一次数据落定时间（毫秒；过期判断用） */
  dataUpdatedAt: number;
  fetchNextPage: () => Promise<unknown>;
  refetch: () => Promise<unknown>;
}

/**
 * 翻页流按业务 id 去重：服务端相邻分页会重叠条目（榜单实测相邻页重叠
 * 10 条，评论/剧评分页同样有），不去重 React 直接报 duplicate key 且
 * 同一条目重复渲染。flatMap 之后、进列表之前必过这一道。
 */
export function dedupBy<T>(items: T[], keyOf: (item: T) => string): T[] {
  const seen = new Set<string>();
  const out: T[] = [];
  for (const item of items) {
    const k = keyOf(item);
    if (seen.has(k)) continue;
    seen.add(k);
    out.push(item);
  }
  return out;
}

/**
 * 无限滚动流的通用出口：把 useInfiniteQuery 的结果包装成旧手动累积器
 * 的形状（items/hasMore/isLoading/isFetchingMore/error/loadMore/refresh），
 * 页面侧无感迁移。翻页失败不炸整页——旧内容还在，错误就地展示。
 */
export function useInfiniteStream<TPage, TItem>(
  query: InfiniteStream<TPage>,
  flatten: (pages: TPage[]) => TItem[],
) {
  const [loadError, setLoadError] = useState<string | null>(null);
  const pages = query.data?.pages;
  const items = useMemo(() => flatten(pages ?? []), [pages, flatten]);

  const loadMore = useCallback(() => {
    if (!query.hasNextPage || query.isFetchingNextPage) return;
    setLoadError(null);
    query.fetchNextPage().catch((e: unknown) => {
      setLoadError(e instanceof Error ? e.message : String(e));
    });
  }, [query]);

  const refresh = useCallback(() => {
    setLoadError(null);
    return query.refetch();
  }, [query]);

  return {
    items,
    hasMore: query.hasNextPage,
    isLoading: query.isPending,
    isRefreshing: query.isFetching && !query.isPending,
    isFetchingMore: query.isFetchingNextPage,
    /** 最近一次数据落定时间（过期判断用，如回首页换一批） */
    dataUpdatedAt: query.dataUpdatedAt,
    error: query.error
      ? query.error instanceof Error
        ? query.error.message
        : String(query.error)
      : loadError,
    loadMore,
    refresh,
  };
}
