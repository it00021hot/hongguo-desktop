import { useCallback } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';

import { play, series } from '../commands';
import { useEvent } from '../tauri/events';
import { EVENTS } from '../tauri/types';
import { keys } from './common';

// ---------------------------------------------------------------- 剧集档案

export function useSeriesEpisodes(seriesId: string | null) {
  const queryClient = useQueryClient();
  // 旧格式档案在 Rust 侧后台补计数，补完发事件——这里失效自己的缓存，
  // 让计数无感浮现（档案本身早已秒回，不等这次刷新）
  useEvent<string>(EVENTS.seriesArchiveUpdated, (id) => {
    if (id && id === seriesId) {
      void queryClient.invalidateQueries({ queryKey: keys.seriesEpisodes(id) });
    }
  });
  return useQuery({
    queryKey: keys.seriesEpisodes(seriesId ?? ''),
    // 挂起兜底：Rust 热重载重启会丢掉在途 invoke 的应答（promise 永不
    // settle），30s 强制超时转成错误，让上层给出重试入口而不是永远空态
    queryFn: () =>
      Promise.race([
        series.episodes(seriesId!),
        new Promise<never>((_, reject) => {
          setTimeout(() => reject(new Error('分集档案解析超时')), 30_000);
        }),
      ]),
    enabled: seriesId !== null,
  });
}

/**
 * 一部剧最近看到的那一集（本地 playback 表，5 秒一写的真值）。
 *
 * 故意不给 staleTime：详情页每次挂载都要现读——「继续看第 N 集」停在旧集
 * 的根源就是云端历史既滞后又有缓存，这里必须是本地最新值。
 */
export function useSeriesProgress(seriesId: string) {
  return useQuery({
    queryKey: keys.seriesProgress(seriesId),
    queryFn: () => play.progress(seriesId),
    enabled: seriesId !== '',
    gcTime: 60_000,
  });
}

/**
 * 剧集元信息（收藏/点赞等列表页用）：本地档案命中秒回，未收录的
 * （如书架里从没看过的剧）回落 resolve_series 解析并进同一份缓存。
 */
export function useSeriesMeta(seriesId: string) {
  return useQuery({
    queryKey: keys.seriesEpisodes(seriesId),
    queryFn: async () => {
      try {
        return await series.episodes(seriesId);
      } catch {
        return series.resolve(seriesId);
      }
    },
    enabled: seriesId !== '',
    staleTime: 10 * 60_000,
  });
}

/** 详情页相关作品·系列（失败静默降级，不阻塞推荐 tab 的其他内容）。 */
export function useRelatedSeries(seriesId: string) {
  return useQuery({
    queryKey: keys.relatedSeries(seriesId),
    queryFn: () => series.related(seriesId),
    staleTime: 10 * 60_000,
    retry: false,
  });
}

/**
 * 详情页头部元信息（追剧数/播放量/季徽/题材标签/备案号）。
 * 失败静默降级：头部缺这几行不影响主功能（与后端同一口径）。
 * （与上面收藏/列表页的 useSeriesMeta 不同：那个回退 resolve 拿整档案。）
 */
export function useSeriesDetailMeta(seriesId: string) {
  return useQuery({
    queryKey: keys.seriesMeta(seriesId),
    queryFn: () => series.meta(seriesId),
    staleTime: 10 * 60_000,
    retry: false,
  });
}

/** 预取一部剧的分集档案（本地缺失会回落解析并落库）——沉浸流切下一部剧时
 * resolve 链路提前走完，切换只剩取流时间。 */
export function usePrefetchSeriesEpisodes() {
  const qc = useQueryClient();
  return useCallback(
    (seriesId: string) => {
      if (!seriesId) return;
      void qc.prefetchQuery({
        queryKey: keys.seriesEpisodes(seriesId),
        queryFn: () => series.episodes(seriesId),
        staleTime: 5 * 60_000,
      });
    },
    [qc],
  );
}

export function useResolveSeries() {
  return useMutation({ mutationFn: (input: string) => series.resolve(input) });
}
