import { call } from '../tauri/invoke';
import { calendarPageSchema, rankPageSchema, type CalendarPage, type RankPage } from '../schema';

// ---------------------------------------------------------------- 排行榜 / 新剧 / 搜索（官方 App API）

export const rank = {
  /**
   * 拉一个榜单（任意 内容tab × 子榜 × 筛选 组合）。
   * selected/sub 用响应 tabs schema 下发的 id；panel 为空串 = 总榜（无筛选）。
   */
  list: (selected: string, sub: string, panel: string = '', offset = 0, sessionId = '') =>
    call<RankPage>(
      'rank_list',
      {
        selected,
        sub,
        panel: panel === '' ? undefined : panel,
        offset: offset || undefined,
        sessionId: sessionId || undefined,
      },
      rankPageSchema,
    ),
  /** 新剧推荐（gender: 2=全部；offset 步长 18）。 */
  newDrama: (gender: number, offset?: number) =>
    call<RankPage>('new_drama_list', { gender, offset: offset ?? 0 }, rankPageSchema),
  /** 上新日历（date 传返回值 dates 里的日期，不传取默认日）。 */
  calendar: (date?: string) =>
    call<CalendarPage>(
      'new_drama_calendar',
      date != null ? { date } : undefined,
      calendarPageSchema,
    ),
  /** 我的预约（匿名通常空表；登录后条目带 hasSubscribed）。 */
  reservations: (isOnline = true) =>
    call<CalendarPage>('reservation_list', { isOnline }, calendarPageSchema),
  /** 预约 / 取消预约（需要登录）。 */
  reserve: (seriesId: string, reserve = true) =>
    call<void>('reservation_reserve', { seriesId, reserve }),
};
