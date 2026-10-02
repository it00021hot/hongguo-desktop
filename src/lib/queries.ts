import { useCallback } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  browse,
  download,
  merge,
  play,
  search,
  series,
  settings,
  storage,
  transcode,
} from './ipc/commands';
import { useEvent } from './ipc/events';
import { EVENTS } from './ipc/types';
import type { DownloadProgress, DownloadTask, MergeMode, QueueStatus } from './schema';

/**
 * TanStack Query 的 key 工厂。
 *
 * 所有 key 必须从这里出，不要在组件里手写字符串——否则失效（invalidate）
 * 时容易漏掉某个 key，导致界面不刷新。
 */
const keys = {
  settings: ['settings'] as const,
  seriesList: ['series-list'] as const,
  seriesEpisodes: (id: string) => ['series-episodes', id] as const,
  seriesExtras: (id: string) => ['series-extras', id] as const,
  playbackHistory: ['playback-history'] as const,
  tasks: ['download-tasks'] as const,
  queueStatus: ['queue-status'] as const,
  mergeTasks: ['merge-tasks'] as const,
  mergePreflight: (id: string) => ['merge-preflight', id] as const,
  storageUsage: ['storage-usage'] as const,
  capability: ['decode-capability'] as const,
  browseCategories: ['browse-categories'] as const,
  browseList: (cat: string, genre: string, page: number) =>
    ['browse-list', cat, genre, page] as const,
  seriesSearch: (keyword: string) => ['series-search', keyword] as const,
} satisfies Record<string, unknown>;

// ---------------------------------------------------------------- 设置

export function useSettings() {
  return useQuery({ queryKey: keys.settings, queryFn: settings.get });
}

export function useSaveSettings() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: settings.save,
    onSuccess: (saved) => {
      qc.setQueryData(keys.settings, saved);
      void qc.invalidateQueries({ queryKey: keys.queueStatus });
    },
  });
}

export function useTestProxy() {
  return useMutation({ mutationFn: settings.testProxy });
}

// ---------------------------------------------------------------- 剧集

export function useSeriesList() {
  return useQuery({ queryKey: keys.seriesList, queryFn: series.list });
}

export function useSeriesEpisodes(seriesId: string | null) {
  return useQuery({
    queryKey: keys.seriesEpisodes(seriesId ?? ''),
    queryFn: () => series.episodes(seriesId!),
    enabled: seriesId !== null,
  });
}

export function useSeriesExtras(seriesId: string) {
  return useQuery({
    queryKey: keys.seriesExtras(seriesId),
    queryFn: () => series.extras(seriesId),
    enabled: seriesId !== '',
    staleTime: 10 * 60_000,
  });
}

// ---------------------------------------------------------------- 浏览与搜索

export function useBrowseCategories() {
  return useQuery({
    queryKey: keys.browseCategories,
    queryFn: browse.categories,
    staleTime: Infinity,
  });
}

export function useBrowseList(category: string, genre: string, page: number) {
  return useQuery({
    queryKey: keys.browseList(category, genre, page),
    queryFn: () => browse.list(category, genre, page),
    // 翻页时保留上一页数据，避免白屏
    placeholderData: (prev) => prev,
    staleTime: 30_000,
  });
}

export function useSearch(keyword: string) {
  const kw = keyword.trim();
  return useQuery({
    queryKey: keys.seriesSearch(kw),
    queryFn: () => search.run(kw),
    // 关键词为空就是浏览模式，不该发嗅探请求
    enabled: kw !== '',
    staleTime: 5 * 60_000,
  });
}

export function useResolveSeries() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (input: string) => series.resolve(input),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: keys.seriesList });
    },
  });
}

/** 从剧集列表移除一部剧。磁盘清理页的「移除记录」用它。 */
export function useRemoveSeries() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (seriesId: string) => series.remove(seriesId),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: keys.seriesList });
    },
  });
}

// ---------------------------------------------------------------- 下载

/**
 * 下载任务列表。
 *
 * 进度事件每 500ms 一次，失效查询等于整表重拉一遍。这里就地合并进缓存：
 * 任务集合变化仍然由 `useDownloadEvents` 触发重新请求，
 * 纯进度更新只改对应任务的那两个字段，缓存里也只有一份真相。
 */
export function useDownloadTasks() {
  const qc = useQueryClient();

  const applyProgress = useCallback(
    (p: DownloadProgress) => {
      qc.setQueryData<DownloadTask[]>(keys.tasks, (tasks) =>
        tasks?.map((task) =>
          task.id === p.id ? { ...task, downloaded: p.downloaded, total: p.total } : task,
        ),
      );
    },
    [qc],
  );
  useEvent<DownloadProgress>(EVENTS.downloadProgress, applyProgress);

  return useQuery({ queryKey: keys.tasks, queryFn: download.tasks });
}

export function useQueueStatus() {
  return useQuery({ queryKey: keys.queueStatus, queryFn: download.status });
}

export function useDownloadActions() {
  const qc = useQueryClient();
  const invalidate = () => {
    void qc.invalidateQueries({ queryKey: keys.tasks });
    void qc.invalidateQueries({ queryKey: keys.queueStatus });
  };

  return {
    start: useMutation({
      mutationFn: ({ seriesId, vids }: { seriesId: string; vids: number[] }) =>
        download.start(seriesId, vids),
      onSuccess: invalidate,
    }),
    stop: useMutation({ mutationFn: download.stop, onSuccess: invalidate }),
    retry: useMutation({ mutationFn: download.retry, onSuccess: invalidate }),
    retryMany: useMutation({ mutationFn: download.retryMany, onSuccess: invalidate }),
    remove: useMutation({
      mutationFn: ({ ids, withFiles }: { ids: string[]; withFiles: boolean }) =>
        download.remove(ids, withFiles),
      onSuccess: invalidate,
    }),
    pauseAll: useMutation({ mutationFn: download.pauseAll, onSuccess: invalidate }),
    resumeAll: useMutation({ mutationFn: download.resumeAll, onSuccess: invalidate }),
  };
}

/**
 * 订阅下载任务的结构性事件。
 *
 * 进度事件不在这里：它由 `useDownloadTasks` 自己去合并，
 * 在根布局再订一份等于同一事件被处理两遍。
 *
 * 注意：事件名数量固定，所以这里逐个显式调用 `useEvent`，
 * 不能放进循环——React Hooks 规则禁止在条件或循环里调 Hook。
 */
export function useDownloadEvents() {
  const qc = useQueryClient();
  const invalidate = useCallback(() => {
    void qc.invalidateQueries({ queryKey: keys.tasks });
    void qc.invalidateQueries({ queryKey: keys.queueStatus });
  }, [qc]);

  useEvent<DownloadTask | null>(EVENTS.downloadTaskAdded, invalidate);
  useEvent<DownloadTask | null>(EVENTS.downloadCompleted, invalidate);
  useEvent<DownloadTask | null>(EVENTS.downloadFailed, invalidate);
  useEvent<DownloadTask | null>(EVENTS.downloadStopped, invalidate);
  useEvent<QueueStatus | null>(EVENTS.downloadQueueChanged, invalidate);
}

// ---------------------------------------------------------------- 合并

export function useMergeTasks() {
  return useQuery({ queryKey: keys.mergeTasks, queryFn: merge.tasks });
}

export function useMergePreflight(seriesId: string | null) {
  return useQuery({
    queryKey: keys.mergePreflight(seriesId ?? ''),
    queryFn: () => merge.preflight(seriesId!),
    enabled: seriesId !== null,
  });
}

export function useMergeActions() {
  const qc = useQueryClient();
  const invalidate = () => void qc.invalidateQueries({ queryKey: keys.mergeTasks });

  return {
    start: useMutation({
      mutationFn: ({
        seriesId,
        outputName,
        mode,
      }: {
        seriesId: string;
        outputName: string;
        mode: MergeMode;
      }) => merge.start(seriesId, outputName, mode),
      onSuccess: invalidate,
    }),
    remove: useMutation({ mutationFn: merge.remove, onSuccess: invalidate }),
  };
}

/**
 * 订阅合并事件：进度、开始、完成、失败。
 *
 * 兼容合并要逐集转码，可能跑好几分钟，光靠 `useMergeTasks` 的一次性查询
 * 看不到过程。收尾事件同时触发失效查询，让列表拿到落盘后的最终状态。
 */
export function useMergeEvents() {
  const qc = useQueryClient();
  const invalidate = useCallback(() => {
    void qc.invalidateQueries({ queryKey: keys.mergeTasks });
  }, [qc]);

  useEvent(EVENTS.mergeProgress, invalidate);
  useEvent(EVENTS.mergeTaskAdded, invalidate);
  useEvent(EVENTS.mergeCompleted, invalidate);
  useEvent(EVENTS.mergeFailed, invalidate);
}

// ---------------------------------------------------------------- 播放与存储

export function usePlay() {
  return useMutation({
    // preferOnline 恒为 true：已下载的那一集后端仍优先走本地文件，
    // 没下载的走在线流——与原版一致，否则「点开没下过的集」永远播不了
    mutationFn: ({ seriesId, vidIndex }: { seriesId: string; vidIndex: number }) =>
      play.series(seriesId, vidIndex, '', true),
  });
}

export function useSavePosition() {
  return useMutation({
    mutationFn: ({
      seriesId,
      vidIndex,
      currentTime,
      duration,
    }: {
      seriesId: string;
      vidIndex: number;
      currentTime: number;
      duration: number;
    }) => play.savePosition(seriesId, vidIndex, currentTime, duration),
  });
}

export function usePlaybackHistory() {
  return useQuery({ queryKey: keys.playbackHistory, queryFn: play.history });
}

export function useClearPlaybackHistory() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: play.clearHistory,
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: keys.playbackHistory });
    },
  });
}

export function useStorageUsage() {
  return useQuery({ queryKey: keys.storageUsage, queryFn: storage.usage });
}

export function useDecodeCapability() {
  return useQuery({
    queryKey: keys.capability,
    queryFn: transcode.capability,
    staleTime: Infinity,
  });
}

export function useStorageActions() {
  const qc = useQueryClient();
  const invalidate = () => {
    void qc.invalidateQueries({ queryKey: keys.storageUsage });
    void qc.invalidateQueries({ queryKey: keys.tasks });
  };
  return {
    deleteSeries: useMutation({ mutationFn: storage.deleteSeries, onSuccess: invalidate }),
    deleteEpisode: useMutation({
      mutationFn: ({ seriesId, vidIndex }: { seriesId: string; vidIndex: number }) =>
        storage.deleteEpisode(seriesId, vidIndex),
      onSuccess: invalidate,
    }),
    deleteAll: useMutation({ mutationFn: storage.deleteAll, onSuccess: invalidate }),
  };
}
