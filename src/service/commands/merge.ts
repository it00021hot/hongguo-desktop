import { call } from '../tauri/invoke';
import type { MergeMode } from '../schema';

export const merge = {
  tasks: () => call('get_merge_tasks'),
  candidates: () => call('get_merge_candidates'),
  preflight: (seriesId: string) => call('merge_preflight', { seriesId }),
  start: (seriesId: string, outputName: string, mode: MergeMode) =>
    call('merge_series', { seriesId, outputName, mode }),
  // 只删任务记录，合并产物是独立文件，不在删除范围内
  remove: (id: string) => call('delete_merge_task', { id }),
  // 定位产物文件（打开所在文件夹并选中）
  openOutput: (id: string) => call('open_merge_output', { id }),
};
