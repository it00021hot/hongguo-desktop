import { useCallback } from 'react';
import { useInfiniteQuery, useQueryClient } from '@tanstack/react-query';

import { discover } from '../commands';
import type { FeedItem, FeedPage } from '../schema';
import { keys, useInfiniteStream } from './common';

// ---------------------------------------------------------------- 首页推荐流

/** pages → 按拉取顺序、seriesId 去重的条目（推荐位轮换会跨页重复）。 */
export function feedItems(pages: FeedPage[]): FeedItem[] {
  const seen = new Set<string>();
  const items: FeedItem[] = [];
  for (const page of pages) {
    for (const item of page.items) {
      if (seen.has(item.seriesId)) continue;
      seen.add(item.seriesId);
      items.push(item);
    }
  }
  return items;
}

/**
 * 首页推荐流（书城换一换，无限滚动）。
 *
 * 翻页三件套回传（hgplayer 同款，2026-10-08 抓包实锤）：首页会话
 * `sessionId`（bookmall/tab cr=4 下发）+ `offset`（上一页 nextOffset，
 * 0→6→12 递进）+ `filterIds`（已下发过的 series_id，服务端排除已见）。
 *
 * pages 存在 Query 缓存里：切到其它路由再回来秒出已拉内容，不闪骨架屏；
 * staleTime 内完全不重打，超时只后台刷新（配合 RefreshShade 无感过渡）。
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
    staleTime: 5 * 60_000,
  });
  const stream = useInfiniteStream(query, feedItems);
  // 换一批：重开会话回第 1 批（hgplayer V() 同款）。reset 会清掉已翻的页，
  // 重新从首页游标拉——原地 refetch 只会重放旧会话，看起来「没反应」。
  const restart = useCallback(() => {
    void queryClient.resetQueries({ queryKey: keys.feed(tab) });
  }, [queryClient, tab]);
  return { ...stream, restart };
}
