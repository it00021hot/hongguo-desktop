import { useMutation } from '@tanstack/react-query';

import { rank } from '../commands';
import type { RankPage } from '../schema';
import { keepPreviousData, useInfiniteQuery, useQueryClient } from '@tanstack/react-query';
import { useEffect, useMemo } from 'react';
import { dedupBy, fuzzyStaleTime, keys, useInfiniteStream } from './common';

/**
 * 一个榜单（内容tab × 子榜 × 筛选 × 月榜组合缓存，社交态不缓存每次重拉；切筛选 keepPreviousData 保旧列表）。
 * 对齐官方滚动榜单协议：每页 10 条，翻页回传响应的 next_offset / session_id / rank_version；
 * 页间可能重叠，按 seriesId 去重兜底；tabs 取首页。
 */
export function useRank(selected: string, sub: string, panel: string, background: string) {
  const query = useInfiniteQuery({
    queryKey: keys.rank(selected, sub, panel, background),
    queryFn: ({ pageParam }) =>
      rank.list(
        selected,
        sub,
        panel,
        background,
        pageParam.offset,
        pageParam.sessionId,
        pageParam.rankVersion,
      ),
    initialPageParam: { offset: 0, sessionId: '', rankVersion: '' },
    getNextPageParam: (last) =>
      last.hasMore && last.nextOffset > 0
        ? {
            offset: last.nextOffset,
            sessionId: last.sessionId,
            rankVersion: last.rankVersion,
          }
        : undefined,
    staleTime: 0,
    placeholderData: keepPreviousData,
  });

  const items = useMemo(
    () =>
      dedupBy(query.data?.pages.flatMap((p) => p.items) ?? [], (i) => i.seriesId).map((it, idx) =>
        // recommend_info.rank 是页内位次（每页 1~10、可重复），连续名次按拼接序号重算；0 = 无名次榜（预约榜）保持 0
        it.rank > 0 ? { ...it, rank: idx + 1 } : it,
      ),
    [query.data],
  );
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

/** 新剧推荐（按 gender 分缓存）：每页固定 18 条，下一页 offset 按已拉条数累计；响应不带 has_more，以空页为终点。 */
export function useNewDrama(gender: number) {
  const query = useInfiniteQuery({
    queryKey: keys.newDrama(gender),
    // pageParam 结构化：offset + 上一页回传的 session_id（翻页必须回传，首页不传）
    queryFn: ({ pageParam }) => rank.newDrama(gender, pageParam.offset, pageParam.sessionId),
    initialPageParam: { offset: 0, sessionId: '' },
    getNextPageParam: (last, allPages) => {
      if (last.items.length === 0) return undefined;
      const offset = allPages.reduce((n, p) => n + p.items.length, 0);
      return { offset, sessionId: last.sessionId };
    },
    staleTime: 0,
  });
  return useInfiniteStream(query, newDramaItems);
}

/** 上新日历（date 空串取默认日，切日期 keepPreviousData 平滑过渡）；服务端多天混排 + 游标翻页，Rust 按展示日过滤后单页返回，这里懒翻页。 */
export function useNewCalendar(date: string) {
  const query = useInfiniteQuery({
    queryKey: keys.newCalendar(date),
    queryFn: ({ pageParam }) => rank.calendar(date, pageParam.offset, pageParam.sessionId),
    initialPageParam: { offset: 0, sessionId: '' },
    getNextPageParam: (last) =>
      last.hasMore && last.nextOffset > 0
        ? { offset: last.nextOffset, sessionId: last.sessionId }
        : undefined,
    staleTime: 0,
    placeholderData: keepPreviousData,
  });

  return {
    /** 日期条（每页响应都带 calendar_schema，取首页的即可） */
    dates: query.data?.pages[0]?.dates ?? [],
    defaultDate: query.data?.pages[0]?.defaultDate ?? '',
    items: useMemo(
      () => dedupBy(query.data?.pages.flatMap((p) => p.items) ?? [], (i) => i.seriesId),
      [query.data],
    ),
    isLoading: query.isPending,
    isFetching: query.isFetching,
    error: query.error,
    refetch: () => query.refetch(),
    hasMore: query.hasNextPage,
    isFetchingMore: query.isFetchingNextPage,
    loadMore: () => void query.fetchNextPage().catch(() => undefined),
  };
}

/**
 * 我的预约（isOnline：已上线 / 待上线 tab），单页懒翻页，首屏一页即回；
 * 角标计数直接用响应下发的两个 total。需要全集做预约态成员判定的消费方
 * （ReserveButton / 选集面板）挂 [`useReservationsFillAll`] 后台补齐，共享同一份缓存。
 */
export function useReservations(isOnline: boolean) {
  const query = useInfiniteQuery({
    queryKey: keys.reservations(isOnline),
    queryFn: ({ pageParam }) => rank.reservations(isOnline, pageParam.offset, pageParam.sessionId),
    initialPageParam: { offset: 0, sessionId: '' },
    getNextPageParam: (last) =>
      last.hasMore && last.nextOffset > 0
        ? { offset: last.nextOffset, sessionId: last.sessionId }
        : undefined,
    staleTime: fuzzyStaleTime(keys.reservations(isOnline)),
  });

  const items = useMemo(
    () => dedupBy(query.data?.pages.flatMap((p) => p.items) ?? [], (i) => i.seriesId),
    [query.data],
  );
  const first = query.data?.pages[0];

  return {
    items,
    /**
     * 角标计数（首页响应下发，条数兜底只在响应缺 total 时生效）；
     * null = 首页未到，让 tab 显示骨架点而不是冒充「没有预约」。
     */
    count: first ? Math.max(isOnline ? first.onlineTotal : first.offlineTotal, items.length) : null,
    isLoading: query.isPending,
    isFetchingMore: query.isFetchingNextPage,
    error: query.error,
    hasMore: query.hasNextPage,
    loadMore: () => void query.fetchNextPage().catch(() => undefined),
    refetch: () => query.refetch(),
  };
}

/** 预约全集后台补齐（预约态成员判定用）：挂上后持续翻页直到拉完，与预约页共享同一份缓存；失败即停，重拉后自然重试。 */
export function useReservationsFillAll(isOnline: boolean) {
  const list = useReservations(isOnline);
  useEffect(() => {
    if (list.error || list.isLoading || !list.hasMore || list.isFetchingMore) return;
    list.loadMore();
  });
  return list;
}

/** 批量删除预约（subscribe_delete；两个 tab 的列表一起失效）。 */
export function useReservationsDelete() {
  const queryClient = useQueryClient();
  return useMutation({
    // 部分失败时组件层有带明细的定制 toast，全局错误 toast 静默
    meta: { silentOnError: true },
    mutationFn: (input: {
      itemIds: string[];
      allSelect?: boolean;
      notDelItemIds?: string[];
      isOnline: boolean;
    }) =>
      rank.deleteReservations(
        input.itemIds,
        input.allSelect ?? false,
        input.notDelItemIds ?? [],
        input.isOnline,
      ),
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: keys.reservationsRoot }),
  });
}

/**
 * 预约 / 取消预约一部剧（uncover_subscribe 端点）：刷新预约列表（权威源），
 * 并就地翻新榜单缓存里该条的 reserved 位——cell 接口的 online_subscribed 是响应时刻快照，不翻新则预约态到下次重拉前不翻面。
 */
export function useReserveSeries() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: { seriesId: string; reserve: boolean }) =>
      rank.reserve(input.seriesId, input.reserve),
    onSuccess: (_data, input) => {
      void queryClient.invalidateQueries({ queryKey: keys.reservationsRoot });
      for (const query of queryClient.getQueryCache().findAll({ queryKey: keys.rankRoot })) {
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
