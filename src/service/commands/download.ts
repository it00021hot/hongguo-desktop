import { call } from '../tauri/invoke';

export const download = {
  tasks: () => call('get_download_tasks'),
  status: () => call('get_queue_status'),
  start: (seriesId: string, vids: number[]) => call('download_batch', { seriesId, vids }),
  stop: (taskId: string) => call('stop_download', { taskId }),
  retry: (taskId: string) => call('retry_task', { taskId }),
  retryMany: (taskIds: string[]) => call('retry_tasks', { taskIds }),
  remove: (taskIds: string[], deleteFiles = false) =>
    call('delete_tasks', { taskIds, deleteFiles }),
  pauseAll: () => call('pause_all'),
  resumeAll: () => call('resume_all'),
  // 磁盘上有文件但任务记录丢了：重新登记为已完成。返回补回条数。
  rescan: () => call('rescan_downloads'),
};
