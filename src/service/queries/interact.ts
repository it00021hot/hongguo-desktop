import { useCallback } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';

import { interact as interactCmd, login } from '../commands';
import type { InteractionItem, InteractionState } from '../schema';
import { RESERVATIONS_KEY_ROOT } from './rank';
import { keys } from './common';

// ---------------------------------------------------------------- 互动（点赞 / 收藏 / 书架）

/** 最近互动状态（登录后才拉；匿名接口静默拒）。 */
export function useInteractionState() {
  const { data: account } = useAccount();
  return useQuery({
    queryKey: keys.interactState,
    queryFn: interactCmd.state,
    enabled: !!account,
    staleTime: 60_000,
  });
}

/** 书架（我的收藏）列表；登录后才拉。收藏/取消收藏后要失效。 */
export function useBookshelf() {
  const { data: account } = useAccount();
  return useQuery({
    queryKey: keys.bookshelf,
    queryFn: interactCmd.bookshelf,
    enabled: !!account,
    staleTime: 60_000,
  });
}

/**
 * 登录/退出后的统一缓存刷新：账号态、互动回显、书架、预约一起失效。
 * 任何登录成功/退出入口都该调（LoginDialog / 侧边栏账户区），否则
 * 互动栏与列表页要等 staleTime 过期才翻面。
 */
export function useAuthRefresh() {
  const qc = useQueryClient();
  return useCallback(() => {
    void qc.invalidateQueries({ queryKey: keys.account });
    void qc.invalidateQueries({ queryKey: keys.interactState });
    void qc.invalidateQueries({ queryKey: keys.bookshelf });
    void qc.invalidateQueries({ queryKey: RESERVATIONS_KEY_ROOT });
  }, [qc]);
}

/** 点赞 / 取消点赞一集。 */
export function useVideoDigg() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: { vid: string; seriesId: string; digg: boolean }) =>
      interactCmd.videoDigg(input.vid, input.seriesId, input.digg),
    // 乐观更新：互动回显接口（ugc/action/mget）是「最近互动列表」，
    // 剧集不在列表里时状态永远是 false——不乐观写的话按钮永不点亮、
    // 二次点击也不会走取消分支（详情页「二次点击还是提示已收藏」事故）
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

/** 收藏 / 取消收藏一部剧。 */
export function useSeriesCollect() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: { seriesId: string; collect: boolean }) =>
      interactCmd.seriesCollect(input.seriesId, input.collect),
    // 乐观写 followed（详情页/互动栏的收藏态都从 interactState 匹配），
    // 权威数据源是书架列表（onSuccess 里失效重拉）
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
      // 收藏态的权威回显走书架列表；interactState 的回显是 best-effort，
      // 不失效它——服务端列表延迟回带旧值会把乐观态顶回去（同一事故）
      void queryClient.invalidateQueries({ queryKey: keys.bookshelf });
    },
  });
}

/** 当前登录态（null = 未登录）；登录/退出后要主动失效。 */
export function useAccount() {
  return useQuery({
    queryKey: keys.account,
    queryFn: login.status,
    staleTime: 30_000,
  });
}
