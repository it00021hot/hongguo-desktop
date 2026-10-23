import { useMutation } from '@tanstack/react-query';

import { rank } from '../commands';
import type { RankPage } from '../schema';
import { keepPreviousData, useInfiniteQuery, useQueryClient } from '@tanstack/react-query';
import { useEffect, useMemo } from 'react';
import { dedupBy, fuzzyStaleTime, keys, useInfiniteStream } from './common';

// ---------------------------------------------------------------- 排行榜 / 新剧 / 上新日历 / 预约

/**
 * 一个榜单（内容tab × 子榜 × 筛选 × 月榜 组合缓存；社交态数据不缓存——
 * 2026-10-10 定规，每次挂载重拉。切筛选时用 keepPreviousData 保住
 * 旧列表，避免整页闪 loading）。
 *
 * 无限滚动（**2026-10-17 抓官方 App 滚动榜单实锤的协议**）：每页 10 条
 * （limit 参数恒 "0" 不参与），首页 offset=0 不带 session_id；翻页
 * offset=响应的 next_offset（步进 10）并回传响应的 session_id 与
 * rank_version。页间可能重叠——按 seriesId 去重（后端已按会话记账 +
 * filter_ids 让服务端排重，这里是兜底）。tabs 取首页（选项表每页随行
 * 下发）。
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
        // 服务端 recommend_info.rank 是页内位次（每页 1~10、可重复），
        // 连续名次按拼接序号重算；0 = 无名次榜（预约榜）保持 0
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
    // pageParam 结构化：offset + 上一页回传的 session_id（抓包实锤翻页
    // 必须回传；首页不传）
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

/**
 * 上新日历（date 为空串取默认日；切日期 keepPreviousData 保旧列表平滑过渡）。
 *
 * 服务端是多天混排 + 游标翻页，Rust 按展示日过滤后**单页返回**——这里
 * 用无限查询懒翻页（滚到底再拉下一页），不再一次性串行吃满所有页
 * （那曾是切换日期肉眼可见卡顿的根源）。
 */
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
 * 我的预约（isOnline：已上线 / 待上线 tab）——单页懒翻页。
 *
 * 此前 queryFn 在后端一次性串行拉全：预约上百部时首屏要等几十个串行
 * 请求（issue #11 的「已上线 5–8 秒才出来」）。现与上新日历同构：
 * 首屏一页即回，角标计数直接用响应下发的两个 total（每页响应都带，
 * 首页即真值），滚到底再翻下一页。需要全集做预约态成员判定的消费方
 * （ReserveButton / 选集面板）挂 [`useReservationsFillAll`] 后台补齐，
 * 与本查询共享同一份缓存。
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
     * 角标计数（首页响应下发；条数兜底只在响应缺 total 时生效）。
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

/**
 * 预约全集后台补齐（预约态成员判定用）：ReserveButton / 选集面板要拿
 * 完整列表对号「是否已预约」，挂上后持续翻页直到拉完——预约页先画
 * 一页，剩余页由这里在后台补上，两边共享同一份缓存不重复请求。
 * 失败即停（错误态挡住续拉），下次挂载或失效重拉后自然重试。
 */
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
 * 预约 / 取消预约一部剧（复用 2026-10-04 抓包的 uncover_subscribe 端点）。
 *
 * 成功后两件事：刷新预约列表（权威源）；**就地翻新榜单缓存里该条的
 * reserved 位**——cell 接口的 online_subscribed 是响应时刻快照，不翻新
 * 的话当前挂载页到下次重拉前预约态不翻面（榜单 staleTime 0，重挂即自愈）。
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
