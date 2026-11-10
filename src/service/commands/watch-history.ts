import { call } from '../tauri/invoke';
import type { GuestPlaybackReport } from '../schema/history';

/** 云端观看历史（官方 App「历史」同源；登录后可用，匿名回空表）。 */
export const watchHistory = {
  list: (offset = 0) => call('watch_history_list', { offset }),
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
  /** 批量删除云端历史（read_history/update 的 is_delete+use_soft_delete 形态）。 */
  delete: (items: { seriesId: string; vid: string; vidIndex: number }[]) =>
    call('watch_history_delete', { items }),
  /** 本地观看历史（未登录模式的历史页数据源；字段与云端条目同形）。 */
  localList: () => call('local_history_list'),
  /** 批量删除本地观看历史（按剧）。 */
  localDelete: (seriesIds: string[]) => call('local_history_delete', { seriesIds }),
};
