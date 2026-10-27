import { call } from '../tauri/invoke';

// ---------------------------------------------------------------- 排行榜 / 新剧 / 搜索（官方 App API）

export const rank = {
  /**
   * 拉一个榜单（任意 内容tab × 子榜 × 筛选 × 月榜 组合）。
   * selected/sub 用响应 tabs schema 下发的 id；panel 为空串 = 总榜（无筛选）；
   * background 为月榜选项 id（date_202609 形态，空串 = 当前月）；
   * offset/sessionId/rankVersion 是上一页响应下发的翻页游标。
   */
  list: (
    selected: string,
    sub: string,
    panel: string = '',
    background: string = '',
    offset = 0,
    sessionId = '',
    rankVersion = '',
  ) =>
    call('rank_list', {
      selected,
      sub,
      panel: panel === '' ? undefined : panel,
      background: background === '' ? undefined : background,
      offset: offset || undefined,
      sessionId: sessionId || undefined,
      rankVersion: rankVersion || undefined,
    }),
  /** 新剧推荐（gender: 2=全部；offset 步长 18；翻页回传响应的 sessionId）。 */
  newDrama: (gender: number, offset?: number, sessionId?: string) =>
    call('new_drama_list', { gender, offset: offset ?? 0, sessionId: sessionId ?? '' }),
  /** 上新日历（date 传返回值 dates 里的日期，不传取默认日；游标懒翻页）。 */
  calendar: (date?: string, offset?: number, sessionId?: string) =>
    call('new_drama_calendar', {
      ...(date != null && date !== '' ? { date } : {}),
      ...(offset ? { offset } : {}),
      ...(sessionId ? { sessionId } : {}),
    }),
  /**
   * 我的预约一页（匿名通常空表；登录后条目带 hasSubscribed）。
   * offset/sessionId 翻页续传（响应下发回传），与上新日历同构。
   */
  reservations: (isOnline = true, offset = 0, sessionId = '') =>
    call('reservation_list', {
      isOnline,
      ...(offset ? { offset } : {}),
      ...(sessionId ? { sessionId } : {}),
    }),
  /** 预约 / 取消预约（需要登录）。 */
  reserve: (seriesId: string, reserve = true) => call('reservation_reserve', { seriesId, reserve }),
  /** 批量删除预约（subscribe_delete；需要登录）。 */
  deleteReservations: (
    itemIds: string[],
    allSelect = false,
    notDelItemIds: string[] = [],
    isOnline = true,
  ) => call('reservations_delete', { itemIds, allSelect, notDelItemIds, isOnline }),
};
