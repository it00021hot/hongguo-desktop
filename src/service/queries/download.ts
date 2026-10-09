import { useCallback } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';

import { download } from '../commands';
import { useEvent } from '../tauri/events';
import { EVENTS } from '../tauri/types';
import type { DownloadProgress, DownloadTask, QueueStatus } from '../schema';
import { keys } from './common';

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
    rescan: useMutation({ mutationFn: download.rescan, onSuccess: invalidate }),
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
