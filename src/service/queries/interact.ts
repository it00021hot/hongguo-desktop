import { useCallback } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';

import { interact as interactCmd, login } from '../commands';
import type { InteractionItem, InteractionState } from '../schema';
import { fuzzyStaleTime, keys } from './common';

/** 最近互动状态（登录后才拉，匿名接口静默拒）；社交态数据每次挂载重拉，不缓存。 */
export function useInteractionState() {
  const { data: account } = useAccount();
  return useQuery({
    queryKey: keys.interactState,
    queryFn: interactCmd.state,
    enabled: !!account,
    staleTime: 0,
  });
}

/** 书架（我的收藏）列表；登录后才拉。收藏/取消收藏后要失效。 */
export function useBookshelf() {
  const { data: account } = useAccount();
  return useQuery({
    queryKey: keys.bookshelf,
    queryFn: interactCmd.bookshelf,
    enabled: !!account,
    staleTime: fuzzyStaleTime(keys.bookshelf),
  });
}

/** 登录/退出后的统一缓存刷新：账号态、互动回显、书架、预约一起失效；任何登录成功/退出入口都该调，否则要等 staleTime 过期才翻面。 */
export function useAuthRefresh() {
  const qc = useQueryClient();
  return useCallback(() => {
    void qc.invalidateQueries({ queryKey: keys.account });
    void qc.invalidateQueries({ queryKey: keys.interactState });
    void qc.invalidateQueries({ queryKey: keys.bookshelf });
    void qc.invalidateQueries({ queryKey: keys.reservationsRoot });
  }, [qc]);
}

export function useVideoDigg() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: { vid: string; seriesId: string; digg: boolean }) =>
      interactCmd.videoDigg(input.vid, input.seriesId, input.digg),
    // 乐观更新：互动回显接口是「最近互动列表」，剧不在列表时状态恒 false——不乐观写按钮永不点亮
    onMutate: async (input) => {
      await queryClient.cancelQueries({ queryKey: keys.interactState });
      const prev = queryClient.getQueryData<InteractionState>(keys.interactState);
      queryClient.setQueryData<InteractionState>(keys.interactState, (old) => {
        const items = old?.items ?? [];
        const idx = items.findIndex((i) => i.vid === input.vid);
        const patched = { ...items[idx], userDigg: input.digg } as InteractionItem;
        return {
          items:
            idx >= 0
              ? items.toSpliced(idx, 1, patched)
              : [
                  ...items,
                  {
                    vid: input.vid,
                    seriesId: input.seriesId,
                    userDigg: input.digg,
                    diggedCount: 0,
                    followed: false,
                    followedCnt: 0,
                    seriesTitle: '',
                  },
                ],
        };
      });
      return { prev };
    },
    onError: (_e, _input, ctx) => {
      if (ctx?.prev) queryClient.setQueryData(keys.interactState, ctx.prev);
    },
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: keys.interactState }),
  });
}

export function useSeriesCollect() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: { seriesId: string; collect: boolean }) =>
      interactCmd.seriesCollect(input.seriesId, input.collect),
    // 乐观写 followed（详情页/互动栏收藏态从 interactState 匹配），权威源是书架列表
    onMutate: async (input) => {
      await queryClient.cancelQueries({ queryKey: keys.interactState });
      const prev = queryClient.getQueryData<InteractionState>(keys.interactState);
      queryClient.setQueryData<InteractionState>(keys.interactState, (old) => {
        const items = old?.items ?? [];
        const idx = items.findIndex((i) => i.seriesId === input.seriesId);
        const patched = { ...items[idx], followed: input.collect } as InteractionItem;
        return {
          items:
            idx >= 0
              ? items.toSpliced(idx, 1, patched)
              : [
                  ...items,
                  {
                    vid: '',
                    seriesId: input.seriesId,
                    userDigg: false,
                    diggedCount: 0,
                    followed: input.collect,
                    followedCnt: 0,
                    seriesTitle: '',
                  },
                ],
        };
      });
      return { prev };
    },
    onError: (_e, _input, ctx) => {
      if (ctx?.prev) queryClient.setQueryData(keys.interactState, ctx.prev);
    },
    onSuccess: () => {
      // 收藏态权威回显走书架列表；interactState 回显是 best-effort，不失效——服务端延迟回带旧值会把乐观态顶回去
      void queryClient.invalidateQueries({ queryKey: keys.bookshelf });
    },
  });
}

/** 批量取消收藏（批量管理用；权威回显走书架列表失效重拉）。 */
export function useSeriesCollectBatch() {
  const queryClient = useQueryClient();
  return useMutation({
    // 部分失败时组件层有带明细的定制 toast，全局错误 toast 静默
    meta: { silentOnError: true },
    mutationFn: (entries: { seriesId: string; collect: boolean }[]) =>
      interactCmd.seriesCollectBatch(entries),
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: keys.bookshelf }),
  });
}

/**
 * 当前登录态（null = 未登录）。登录信息属缓存白名单：staleTime Infinity 长期缓存，
 * 仅登录/退出的 `useAuthRefresh` 显式失效，不进 10-30s 抖动节流区间。
 */
export function useAccount() {
  return useQuery({
    queryKey: keys.account,
    queryFn: login.status,
    staleTime: Infinity,
  });
}
