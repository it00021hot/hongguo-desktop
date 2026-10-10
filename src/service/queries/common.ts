/**
 * TanStack Query 的 key 工厂 + 无限滚动通用封装（各查询域共用）。
 *
 * 所有 key 必须从这里出，不要在组件里手写字符串——否则失效（invalidate）
 * 时容易漏掉某个 key，导致界面不刷新。
 */
import { useCallback, useMemo, useState } from 'react';
import type { BrowseFilters } from '../schema';

export const keys = {
  settings: ['settings'] as const,
  feed: (tab: string) => ['feed', tab] as const,
  newDrama: (gender: number) => ['new-drama', gender] as const,
  seriesEpisodes: (id: string) => ['series-episodes', id] as const,
  seriesProgress: (id: string) => ['series-progress', id] as const,
  watchHistory: ['watch-history'] as const,
  tasks: ['download-tasks'] as const,
  queueStatus: ['queue-status'] as const,
  mergeTasks: ['merge-tasks'] as const,
  mergeCandidates: ['merge-candidates'] as const,
  mergePreflight: (id: string) => ['merge-preflight', id] as const,
  storageUsage: ['storage-usage'] as const,
  storageSeries: ['storage-series'] as const,
  capability: ['decode-capability'] as const,
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
  interactState: ['interact-state'] as const,
  bookshelf: ['bookshelf'] as const,
  rank: (selected: string, sub: string, panel: string) => ['rank', selected, sub, panel] as const,
  newCalendar: (date: string) => ['new-calendar', date] as const,
  reservations: (isOnline: boolean) => ['reservations', isOnline] as const,
  account: ['account'] as const,
  appSeriesSearch: (query: string) => ['app-series-search', query] as const,
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
