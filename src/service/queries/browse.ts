import { useInfiniteQuery, useQuery } from '@tanstack/react-query';

import { discover } from '../commands';
import type { BrowseFilters } from '../schema';
import { feedItems } from './feed';
import { keys, useInfiniteStream } from './common';

// ---------------------------------------------------------------- 浏览与搜索

/** 找剧筛选面板：八行维度选项（选项表随服务端运营变化，拉一次长期用）。 */
export function useBrowsePanel() {
  return useQuery({
    queryKey: keys.browsePanel,
    queryFn: discover.browsePanel,
    staleTime: Infinity,
  });
}

/** 找剧筛选流（无限滚动）。
 *
 * 翻页必须回传首页发放的 `sessionId`（服务端按它记住筛选上下文）+
 * 上一页的 `nextOffset` 游标——2026-10-07 抓 hgplayer 1.1.6 实证：
 * limit=18、offset 0→18→36…、session_id 从第二页起同值回传。
 * 旧的 (page-1)*18 算术 offset + 空串 session_id 会让结果集换源，
 * 表现就是「条数和第三方对不上」。
 */
export function useBrowseFeed(filters: BrowseFilters) {
  const query = useInfiniteQuery({
    queryKey: keys.browseFeed(filters),
    queryFn: ({ pageParam }) =>
      pageParam.offset === 0
        ? discover.browsePage(filters, 0)
        : discover.browsePage(filters, pageParam.offset, pageParam.sessionId),
    initialPageParam: { offset: 0, sessionId: '' },
    getNextPageParam: (last) =>
      last.hasMore ? { offset: last.nextOffset, sessionId: last.sessionId } : undefined,
    staleTime: 30_000,
  });
  return useInfiniteStream(query, feedItems);
}
