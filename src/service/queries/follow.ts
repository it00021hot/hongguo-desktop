import { useCallback } from 'react';
import { useInfiniteQuery, useMutation, useQuery, useQueryClient } from '@tanstack/react-query';

import { follow as followCmd } from '../commands';
import type { FollowNotice } from '../schema';
import { keys } from './common';
import { useAccount } from './interact';

// ---------------------------------------------------------------- 追更（列表 / 是否已追 / 更新提醒）

/** 追更列表（cursor 无限翻页）。社交态数据：staleTime 0 每次重拉
 *  （同预约/互动——服务端跟随列表没有「缓存有效期」可言）。 */
export function useFollowUpdates() {
  const { data: account } = useAccount();
  return useInfiniteQuery({
    queryKey: keys.followRoot,
    queryFn: ({ pageParam }) => followCmd.updates(pageParam),
    initialPageParam: '',
    getNextPageParam: (last) => (last.hasMore && last.cursor ? last.cursor : undefined),
    enabled: !!account,
    staleTime: 0,
  });
}

/** 是否已追某剧（列表第一页本地判成员）。详情页/播放器挂载时查。 */
export function useIsFollowing(seriesId: string | null) {
  const { data: account } = useAccount();
  return useQuery({
    queryKey: keys.isFollowing(seriesId ?? ''),
    queryFn: () => followCmd.isFollowing(seriesId ?? ''),
    enabled: !!account && !!seriesId,
    staleTime: 0,
  });
}

/** 追更 / 取消追更。成功后失效「是否已追」与追更列表。 */
export function useFollowToggle() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (input: { seriesId: string; follow: boolean }) =>
      followCmd.update(input.seriesId, input.follow),
    onSuccess: (_data, input) => {
      void qc.invalidateQueries({ queryKey: keys.isFollowing(input.seriesId) });
      void qc.invalidateQueries({ queryKey: keys.followRoot });
    },
  });
}

/** 更新提醒列表（本地库）。 */
export function useFollowNotices() {
  const { data: account } = useAccount();
  return useQuery({
    queryKey: keys.followNotices,
    queryFn: followCmd.notices,
    enabled: !!account,
    staleTime: 0,
  });
}

/** 标记提醒已读 / 清空，完成后失效提醒列表（角标与列表一起翻面）。 */
export function useFollowNoticesMutations() {
  const qc = useQueryClient();
  const invalidate = useCallback(() => {
    void qc.invalidateQueries({ queryKey: keys.followNotices });
  }, [qc]);
  const markRead = useMutation({
    mutationFn: (ids: number[]) => followCmd.noticesRead(ids),
    onSuccess: invalidate,
  });
  const clear = useMutation({
    mutationFn: () => followCmd.noticesClear(),
    onSuccess: invalidate,
  });
  return { markRead, clear };
}

/** 手动检查更新（「检查更新」按钮），返回新增提醒数。 */
export function useFollowCheck() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: () => followCmd.checkUpdates(),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: keys.followNotices });
      void qc.invalidateQueries({ queryKey: keys.followRoot });
    },
  });
}

/** 新通知到达后的统一失效（事件订阅方调）：追更列表/角标，加上出新集
 * 剧的分集缓存——后端虽在线现拉，弹内还有 10-30s 抖动节流，这里失效
 * 才能让正开着的选集立刻见新集（「追更已知 400 集、选集还是 380」）。 */
export function useFollowNoticesInvalidator() {
  const qc = useQueryClient();
  return useCallback(
    (notices: FollowNotice[]) => {
      void qc.invalidateQueries({ queryKey: keys.followNotices });
      void qc.invalidateQueries({ queryKey: keys.followRoot });
      for (const n of notices) {
        void qc.invalidateQueries({ queryKey: keys.seriesEpisodes(n.seriesId) });
      }
    },
    [qc],
  );
}

/** 通知列表里是否还有未读（角标用）。 */
export function unreadNotices(notices: FollowNotice[] | undefined): number {
  return notices?.filter((n) => !n.read).length ?? 0;
}
