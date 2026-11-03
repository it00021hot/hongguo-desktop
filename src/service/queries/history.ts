import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';

import { watchHistory } from '../commands';
import { fuzzyStaleTime, keys } from './common';

/** 云端观看历史（官方 App「历史」同源；登录后可用，匿名回空表）。
 *  `enabled` 给历史页双模式用（未登录切本地记录，云端不发起）。 */
export function useWatchHistory(enabled = true) {
  return useQuery({
    queryKey: keys.watchHistory,
    queryFn: () => watchHistory.list(),
    enabled,
    staleTime: fuzzyStaleTime(keys.watchHistory),
  });
}

/** 批量删除云端历史；成功后失效历史列表。 */
export function useWatchHistoryDelete() {
  const queryClient = useQueryClient();
  return useMutation({
    // 部分失败时组件层有带明细的定制 toast，全局错误 toast 静默
    meta: { silentOnError: true },
    mutationFn: (items: { seriesId: string; vid: string; vidIndex: number }[]) =>
      watchHistory.delete(items),
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: keys.watchHistory }),
  });
}

// 本地观看历史

/** 本地观看历史（未登录模式的历史页数据源；字段与云端条目同形）。 */
export function useLocalHistory(enabled: boolean) {
  return useQuery({
    queryKey: keys.localHistory,
    queryFn: () => watchHistory.localList(),
    enabled,
  });
}

/** 批量删除本地历史；成功后失效本地列表。 */
export function useLocalHistoryDelete() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (seriesIds: string[]) => watchHistory.localDelete(seriesIds),
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: keys.localHistory }),
  });
}
