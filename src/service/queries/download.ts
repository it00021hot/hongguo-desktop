import { useCallback } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';

import { download } from '../commands';
import type { DownloadProgress, DownloadTask } from '../schema';
import { useEvent } from '../tauri/events';
import { EVENTS } from '../tauri/types';
import { keys } from './common';

/**
 * 下载任务列表：进度事件（每 500ms）不整表重拉，就地合并进缓存只改对应任务字段；
 * 任务集合变化仍由 `useDownloadEvents` 触发重新请求。
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
  useEvent(EVENTS.downloadProgress, applyProgress);

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
 * 订阅下载任务的结构性事件；进度事件不在这里（由 `useDownloadTasks` 自己合并，
 * 根布局再订一份等于同一事件处理两遍）。事件名数量固定，须逐个显式调用 `useEvent`，不能放进循环。
 */
export function useDownloadEvents() {
  const qc = useQueryClient();
  const invalidate = useCallback(() => {
    void qc.invalidateQueries({ queryKey: keys.tasks });
    void qc.invalidateQueries({ queryKey: keys.queueStatus });
  }, [qc]);

  useEvent(EVENTS.downloadTaskAdded, invalidate);
  useEvent(EVENTS.downloadCompleted, invalidate);
  useEvent(EVENTS.downloadFailed, invalidate);
  useEvent(EVENTS.downloadStopped, invalidate);
  useEvent(EVENTS.downloadQueueChanged, invalidate);
}
