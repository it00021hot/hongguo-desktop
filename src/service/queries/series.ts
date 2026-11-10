import { useCallback } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';

import { play, series } from '../commands';
import { fuzzyStaleTime, keys } from './common';

/** Rust 热重载重启会丢在途 invoke 的应答（promise 永不 settle）——dev 下 30s 强制超时转错误，让上层给重试入口。 */
function withDevTimeout<T>(p: Promise<T>): Promise<T> {
  if (!import.meta.env.DEV) return p;
  return Promise.race([
    p,
    new Promise<never>((_, reject) => {
      setTimeout(() => reject(new Error('分集档案解析超时')), 30_000);
    }),
  ]);
}

/**
 * 分集表查询（选集/详情页/播放器）：在线现拉（连载剧随时更新，本地快照集数一过夜就是错的），
 * 现拉失败后端回落快照；追更通知到达时会显式失效（app-shell 的监听），过期挂载即现拉。
 */
export function useSeriesEpisodes(seriesId: string | null) {
  return useQuery({
    queryKey: keys.seriesEpisodes(seriesId ?? ''),
    queryFn: () => withDevTimeout(series.episodes(seriesId!)),
    // 空串也要挡：恢复目标就位前的启动窗口期传 '' 会打到 resolver 的「请输入剧集 ID」上
    enabled: seriesId !== null && seriesId !== '',
    // 失败 home-page/选集面板就地给 ErrorState + 重试，全局不再弹
    meta: { silentOnError: true },
    staleTime: fuzzyStaleTime(keys.seriesEpisodes(seriesId ?? '')),
  });
}

/**
 * 本地快照查询（点赞/收藏列表行的封面/标题）：快照命中零网络，未收录的剧后端解析一次并归档。
 * 与分集表分 key：列表页一页几十行逐行挂本 hook，绝不能跟着分集表的现拉语义打成几十个网络请求。
 */
export function useSeriesMeta(seriesId: string) {
  return useQuery({
    queryKey: keys.seriesSnapshot(seriesId),
    queryFn: () => withDevTimeout(series.snapshot(seriesId)),
    enabled: seriesId !== '',
    meta: { silentOnError: true },
    staleTime: fuzzyStaleTime(keys.seriesSnapshot(seriesId)),
  });
}

/**
 * 一部剧最近看到的那一集（本地 playback 表，5 秒一写的真值）。
 * 切剧返回和详情页挂载都读本地最新值。
 */
export function useSeriesProgress(seriesId: string) {
  return useQuery({
    queryKey: keys.seriesProgress(seriesId),
    queryFn: () => play.progress(seriesId),
    enabled: seriesId !== '',
    // 本地 playback 表读取，失败退化成从头播即可，不值得打扰
    meta: { silentOnError: true },
    staleTime: 0,
    gcTime: 60_000,
  });
}

/** 详情页相关作品·系列（失败静默降级，不阻塞推荐 tab 的其他内容）。 */
export function useRelatedSeries(seriesId: string) {
  return useQuery({
    queryKey: keys.relatedSeries(seriesId),
    queryFn: () => series.related(seriesId),
    // 空串也要挡：启动窗口期传 '' 会拿空 book_id 打满重试再报误导性 Media 错误
    enabled: seriesId !== '',
    // 文档口径即静默降级：失败只是切季面板/接力不可用，不弹全局
    meta: { silentOnError: true },
    staleTime: fuzzyStaleTime(keys.relatedSeries(seriesId)),
    retry: false,
  });
}

/** 详情页头部元信息（追剧数/播放量/季徽/题材标签/备案号）；失败静默降级，与后端同一口径。 */
export function useSeriesDetailMeta(seriesId: string) {
  return useQuery({
    queryKey: keys.seriesMeta(seriesId),
    queryFn: () => series.meta(seriesId),
    // 头部装饰性信息，失败静默降级（doc 口径），不弹全局
    meta: { silentOnError: true },
    staleTime: fuzzyStaleTime(keys.seriesMeta(seriesId)),
    retry: false,
  });
}

/** 预取一部剧的分集档案（本地缺失会回落解析并落库）——沉浸流切下一部剧时 resolve 链路提前走完，切换只剩取流时间。 */
export function usePrefetchSeriesEpisodes() {
  const qc = useQueryClient();
  return useCallback(
    (seriesId: string) => {
      if (!seriesId) return;
      const key = keys.seriesEpisodes(seriesId);
      void qc.prefetchQuery({
        queryKey: key,
        queryFn: () => series.episodes(seriesId),
        staleTime: fuzzyStaleTime(key),
      });
    },
    [qc],
  );
}

export function useResolveSeries() {
  return useMutation({ mutationFn: (input: string) => series.resolve(input) });
}
