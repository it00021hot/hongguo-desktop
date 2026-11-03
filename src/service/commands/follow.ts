import { call } from '../tauri/invoke';

/** 追更域 IPC 封装（登录墙在 service 层，未登录报 `Auth` 原文）。 */
export const follow = {
  update: (seriesId: string, follow: boolean) => call('follow_update', { seriesId, follow }),
  /** 追更列表一页；cursor 传上页响应的 cursor（首页空串）。 */
  updates: (cursor: string) => call('follow_updates', { cursor }),
  /** 是否已追某剧（追更列表第一页本地判成员）。 */
  isFollowing: (seriesId: string) => call('is_following', { seriesId }),
  /** 更新提醒列表（本地库，新→旧）。 */
  notices: () => call('follow_notices'),
  /** 标记提醒已读，返回改动行数。 */
  noticesRead: (ids: number[]) => call('follow_notices_read', { ids }),
  noticesClear: () => call('follow_notices_clear'),
  /** 手动检查更新，返回新增提醒数（未登录报错——用户主动动作要有回音）。 */
  checkUpdates: () => call('follow_check_updates'),
};
