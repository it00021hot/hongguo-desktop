import { useCallback } from 'react';
import { useInfiniteQuery, useQuery, useQueryClient } from '@tanstack/react-query';

import { discover } from '../commands';
import type { FeedItem, FeedPage } from '../schema';
import { dedupBy, fuzzyStaleTime, keys, useInfiniteStream } from './common';

/** pages → 按拉取顺序、seriesId 去重的条目（推荐位轮换会跨页重复）。 */
export function feedItems(pages: FeedPage[]): FeedItem[] {
  return dedupBy(
    pages.flatMap((p) => p.items),
    (i) => i.seriesId,
  );
}

/**
 * 首页推荐流（书城换一换，无限滚动）：翻页三件套回传——首页会话 sessionId + offset（上页 nextOffset）
 * + filterIds（已下发过的 series_id，服务端排除已见）；pages 存缓存，切路由回来秒出不闪骨架屏。
 */
export function useFeed(tab: string) {
  const queryClient = useQueryClient();
  const query = useInfiniteQuery({
    queryKey: keys.feed(tab),
    queryFn: ({ pageParam }) =>
      discover.recommendFeed(tab, pageParam.offset, pageParam.sessionId, pageParam.filterIds),
    initialPageParam: { offset: 0, sessionId: '', filterIds: [] as string[] },
    getNextPageParam: (last, allPages) => {
      if (!last.hasMore) return undefined;
      // filterIds 累积全部已下发条目（跨页去重的服务端形态）
      const seen = Array.from(new Set(allPages.flatMap((p) => p.items.map((i) => i.seriesId))));
      return { offset: last.nextOffset, sessionId: last.sessionId, filterIds: seen };
    },
    // 失败 home-page 就地给 ErrorState + 重试，全局不再弹
    meta: { silentOnError: true },
    staleTime: fuzzyStaleTime(keys.feed(tab)),
  });
  const stream = useInfiniteStream(query, feedItems);
  // 换一批：reset 清掉已翻页从首页游标重拉；原地 refetch 只会重放旧会话，看起来「没反应」
  const restart = useCallback(() => {
    void queryClient.resetQueries({ queryKey: keys.feed(tab) });
  }, [queryClient, tab]);
  return { ...stream, restart };
}

/** 首页顶栏 tab 列表（服务端下发）；拉取前/失败由壳侧回落渲染不阻塞首屏，属配置数据抖动节流内不重打。 */
export function useFeedTabs() {
  return useQuery({
    queryKey: keys.feedTabs,
    queryFn: () => discover.recommendTabs(),
    staleTime: fuzzyStaleTime(keys.feedTabs),
    meta: { silentOnError: true },
  });
}
