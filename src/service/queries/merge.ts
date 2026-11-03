import { useCallback } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';

import { merge } from '../commands';
import { useEvent } from '../tauri/events';
import { EVENTS } from '../tauri/types';
import type { MergeMode, MergeTask } from '../schema';
import { keys } from './common';

export function useMergeTasks() {
  return useQuery({ queryKey: keys.mergeTasks, queryFn: merge.tasks });
}

/** 可合并的剧按下载队列算，下载一完成就要重取（刚下完的剧此刻才第一次成为可合并项）；挂下载收尾事件失效。 */
export function useMergeCandidates() {
  const qc = useQueryClient();
  const invalidate = useCallback(() => {
    void qc.invalidateQueries({ queryKey: keys.mergeCandidates });
  }, [qc]);

  useEvent(EVENTS.downloadTaskAdded, invalidate);
  useEvent(EVENTS.downloadCompleted, invalidate);
  useEvent(EVENTS.downloadStopped, invalidate);
  useEvent(EVENTS.downloadQueueChanged, invalidate);

  return useQuery({ queryKey: keys.mergeCandidates, queryFn: merge.candidates });
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
    // 打开产物所在文件夹：不改动任何数据，无需失效查询
    openOutput: useMutation({ mutationFn: merge.openOutput }),
  };
}

/**
 * 订阅合并事件：兼容合并可能跑好几分钟，收尾事件触发失效让列表拿到落盘后的最终状态。
 * 进度不失效查询，就地合并进缓存（同 useDownloadTasks）。
 */
export function useMergeEvents() {
  const qc = useQueryClient();
  const invalidate = useCallback(() => {
    void qc.invalidateQueries({ queryKey: keys.mergeTasks });
  }, [qc]);

  // 进度不失效查询：后端按 1% 步进推送（载荷已落库），就地合并只是让界面即时跟上
  const applyProgress = useCallback(
    (snapshot: MergeTask) => {
      qc.setQueryData<MergeTask[]>(keys.mergeTasks, (tasks) =>
        tasks?.map((task) =>
          task.id === snapshot.id ? { ...task, percent: snapshot.percent } : task,
        ),
      );
    },
    [qc],
  );
  useEvent(EVENTS.mergeProgress, applyProgress);
  useEvent(EVENTS.mergeTaskAdded, invalidate);
  useEvent(EVENTS.mergeCompleted, invalidate);
  useEvent(EVENTS.mergeFailed, invalidate);
}
