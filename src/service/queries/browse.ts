import { useInfiniteQuery, useQuery } from '@tanstack/react-query';

import { discover } from '../commands';
import type { BrowseFilters } from '../schema';
import { feedItems } from './feed';
import { keys, useInfiniteStream } from './common';

/** 找剧筛选面板：八行维度选项（选项表随服务端运营变化，每次打开重拉，运营数据不缓存）。 */
export function useBrowsePanel() {
  return useQuery({
    queryKey: keys.browsePanel,
    queryFn: discover.browsePanel,
    staleTime: 0,
  });
}

/** 找剧筛选流（无限滚动）：翻页必须回传首页发放的 sessionId（服务端按它记筛选上下文）+ 上页 nextOffset。
 *  旧的 (page-1)*18 算术 offset + 空串 session_id 会让结果集换源，表现为「条数和官方 App 对不上」。 */
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
    staleTime: 0,
  });
  return useInfiniteStream(query, feedItems);
}
