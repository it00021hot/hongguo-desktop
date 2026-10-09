import { call } from '../tauri/invoke';
import { watchHistoryPageSchema, type WatchHistoryPage } from '../schema';

// ---------------------------------------------------------------- 云端观看历史

/** 云端观看历史（官方 App「历史」同源；登录后可用，匿名回空表）。 */
export const watchHistory = {
  list: (offset = 0) =>
    call<WatchHistoryPage>('watch_history_list', { offset }, watchHistoryPageSchema),
  /**
   * 观看进度云上报（read_history/update + read_progress/upload 双接口）。
   * 后端匿名时静默跳过、失败只记日志——fire-and-forget 即可。
   */
  reportProgress: (seriesId: string, vid: string, vidIndex: number, positionMs: number) =>
    call<void>('cloud_report_progress', { seriesId, vid, vidIndex, positionMs }),
};
