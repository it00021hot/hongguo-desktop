import { useMutation } from '@tanstack/react-query';

import { rank } from '../commands/rank';
import type { RankItem, RankPage } from '../schema';
import { keepPreviousData, useInfiniteQuery, useQuery, useQueryClient } from '@tanstack/react-query';
import { useMemo } from 'react';
import { keys, useInfiniteStream } from './common';

// ---------------------------------------------------------------- 排行榜 / 新剧 / 上新日历 / 预约

/** 我的预约查询的根 key（登录/预约变更后整组失效用）。 */
export const RESERVATIONS_KEY_ROOT = ['reservations'] as const;

/**
 * 一个榜单（内容tab × 子榜 × 筛选 组合缓存；榜单一天更新几次，10 分钟内
 * 不重打。切筛选时用 keepPreviousData 保住旧列表，避免整页闪 loading）。
 *
 * 无限滚动（**2026-10-09 抓 hgplayer 滚动榜单实锤的协议**）：每页固定
 * 20 条（limit 参数恒 "0" 不参与），首页 offset=0 不带 session_id；翻页
 * offset=响应的 next_offset（步进 10）并回传响应的 session_id。相邻页
 * 重叠 10 条——按 seriesId 去重。tabs 取首页（选项表每页随行下发）。
 */
export function useRank(selected: string, sub: string, panel: string) {
  const query = useInfiniteQuery({
    queryKey: keys.rank(selected, sub, panel),
    queryFn: ({ pageParam }) =>
      rank.list(selected, sub, panel, pageParam.offset, pageParam.sessionId),
    initialPageParam: { offset: 0, sessionId: '' },
    getNextPageParam: (last) =>
      last.hasMore && last.nextOffset > 0
        ? { offset: last.nextOffset, sessionId: last.sessionId }
        : undefined,
    staleTime: 10 * 60_000,
    placeholderData: keepPreviousData,
  });

  const items = useMemo(() => {
    const seen = new Set<string>();
    const out: RankItem[] = [];
    for (const page of query.data?.pages ?? []) {
      for (const item of page.items) {
        if (seen.has(item.seriesId)) continue;
        seen.add(item.seriesId);
        out.push(item);
      }
    }
    return out;
  }, [query.data]);
  const tabs = query.data?.pages[0]?.tabs ?? [];

  return {
    items,
    tabs,
    isLoading: query.isPending,
    isFetching: query.isFetching,
    error: query.error,
    refetch: () => query.refetch(),
    hasMore: query.hasNextPage,
    isFetchingMore: query.isFetchingNextPage,
    loadMore: () => void query.fetchNextPage().catch(() => undefined),
  };
}

/** pages → 顺序条目（新剧推荐无推荐位轮换，直接平铺）。 */
function newDramaItems(pages: RankPage[]) {
  return pages.flatMap((p) => p.items);
}

/**
 * 新剧推荐（无限滚动，按 gender 分缓存）。
 *
 * 每页固定 18 条，下一页 offset 按已拉条数累计；响应不带 has_more，
 * 以空页为终点（旧实现会向空页无限续拉，这里顺手修正）。频道各存一份
 * 缓存，切回看过的频道秒出。
 */
export function useNewDrama(gender: number) {
  const query = useInfiniteQuery({
    queryKey: keys.newDrama(gender),
    queryFn: ({ pageParam }) => rank.newDrama(gender, pageParam),
    initialPageParam: 0,
    getNextPageParam: (last, allPages) =>
      last.items.length > 0 ? allPages.reduce((n, p) => n + p.items.length, 0) : undefined,
    staleTime: 10 * 60_000,
  });
  return useInfiniteStream(query, newDramaItems);
}

/** 上新日历（date 为空串取默认日；切日期保旧列表平滑过渡）。 */
export function useNewCalendar(date: string) {
  return useQuery({
    queryKey: keys.newCalendar(date),
    queryFn: () => rank.calendar(date === '' ? undefined : date),
    staleTime: 10 * 60_000,
    placeholderData: keepPreviousData,
  });
}

/** 我的预约（isOnline：已上线 / 待上线 tab）。 */
export function useReservations(isOnline: boolean) {
  return useQuery({
    queryKey: keys.reservations(isOnline),
    queryFn: () => rank.reservations(isOnline),
    staleTime: 60_000,
  });
}

/**
 * 预约 / 取消预约一部剧（复用 2026-10-04 抓包的 uncover_subscribe 端点）。
 *
 * 成功后两件事：刷新预约列表（权威源）；**就地翻新榜单缓存里该条的
 * reserved 位**——cell 接口的 online_subscribed 是响应时刻快照，不翻新的
 * 话切个子榜回来（10 分钟 staleTime 内命中缓存）预约态就「丢」了。
 */
export function useReserveSeries() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: { seriesId: string; reserve: boolean }) =>
      rank.reserve(input.seriesId, input.reserve),
    onSuccess: (_data, input) => {
      void queryClient.invalidateQueries({ queryKey: RESERVATIONS_KEY_ROOT });
      for (const query of queryClient.getQueryCache().findAll({ queryKey: ['rank'] })) {
        queryClient.setQueryData<RankPage>(query.queryKey, (page) =>
          page
            ? {
                ...page,
                items: page.items.map((i) =>
                  i.seriesId === input.seriesId ? { ...i, reserved: input.reserve } : i,
                ),
              }
            : page,
        );
      }
    },
  });
}
