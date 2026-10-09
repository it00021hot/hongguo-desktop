import { useCallback } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';

import { merge } from '../commands/merge';
import { useEvent } from '../tauri/events';
import { EVENTS } from '../tauri/types';
import type { MergeMode, MergeTask } from '../schema';
import { keys } from './common';

// ---------------------------------------------------------------- 合并

export function useMergeTasks() {
  return useQuery({ queryKey: keys.mergeTasks, queryFn: merge.tasks });
}

/**
 * 可合并的剧列表。
 *
 * 候选是按下载队列算出来的，所以下载一完成就要重取：刚下完的那部剧
 * 在这一刻才第一次成为可合并项。挂在下载收尾事件上而不是只靠进页面时拉一次，
 * 否则用户下完切到合并页看到的还是「没有已下载的分集」。
 */
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
 * 订阅合并事件：进度、开始、完成、失败。
 *
 * 兼容合并要逐集转码，可能跑好几分钟，光靠 `useMergeTasks` 的一次性查询
 * 看不到过程。收尾事件触发失效查询，让列表拿到落盘后的最终状态。
 */
export function useMergeEvents() {
  const qc = useQueryClient();
  const invalidate = useCallback(() => {
    void qc.invalidateQueries({ queryKey: keys.mergeTasks });
  }, [qc]);

  // 进度**不失效查询**，照下载那边的做法就地合并进缓存（见 useDownloadTasks）。
  // 后端按 1% 步进推送（集内回调在后端打了闸），载荷里 percent/episodeCount
  // 都已填好且同步落了库；就地合并只是让界面即时跟上，不依赖下次重查。
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
  useEvent<MergeTask>(EVENTS.mergeProgress, applyProgress);
  useEvent(EVENTS.mergeTaskAdded, invalidate);
  useEvent(EVENTS.mergeCompleted, invalidate);
  useEvent(EVENTS.mergeFailed, invalidate);
}
