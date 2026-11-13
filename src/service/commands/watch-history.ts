import { call } from '../tauri/invoke';
import type { GuestPlaybackReport, HistoryAccount } from '../schema/history';

/** 公共 Rust 服务统一读取、合并及删除观看历史。 */
export const watchHistory = {
  list: (expectedAccount: HistoryAccount | null) => call('watch_history_list', { expectedAccount }),
  /**
   * 记录观看历史：本地必记（未登录也有历史），登录态顺路上报云端，后端失败只记日志。
   * meta 必须与 Rust 命令同形嵌套（平铺被 serde 静默丢弃，见 registry 契约表注释）。
   */
  record: (item: {
    seriesId: string;
    vid: string;
    vidIndex: number;
    positionMs: number;
    durationMs: number;
    meta: { title: string; cover: string; episodeCnt: number; guest?: GuestPlaybackReport };
  }) => call('record_watch_history', item),
  delete: (
    items: { seriesId: string; vid: string; vidIndex: number }[],
    expectedAccount: HistoryAccount | null,
  ) => call('watch_history_delete', { items, expectedAccount }),
};
