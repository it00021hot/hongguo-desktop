import { call } from '../tauri/invoke';
import {
  browseFiltersSchema,
  feedPageSchema,
  selectorRowSchema,
  type BrowseFilters,
  type FeedPage,
  type SelectorRow,
} from '../schema';

// ---------------------------------------------------------------- 发现（首页推荐流 / 找剧）

export const discover = {
  /**
   * 首页推荐流（书城换一换，hgplayer RecommendTab 同源）。
   * tab：'16'=推荐、'36'=漫剧、'39'=真人剧；sessionId 首页空串（走
   * bookmall/tab cr=4），翻页回传上一页会话 + offset（nextOffset）+
   * filterIds（已下发过的 series_id，服务端排除已见）。
   */
  recommendFeed: (tab: string, offset: number, sessionId: string, filterIds: string[]) =>
    call<FeedPage>('recommend_feed', { tab, offset, sessionId, filterIds }, feedPageSchema),
  /** 找剧筛选面板（八行维度选项） */
  browsePanel: () => call<SelectorRow[]>('browse_panel', undefined, selectorRowSchema.array()),
  /** 找剧一页结果（多维筛选，服务端过滤；sessionId 首页空串、翻页回传） */
  browsePage: (filters: BrowseFilters, offset: number, sessionId = '') =>
    call<FeedPage>(
      // zod default 把 undefined 归一成空串，IPC 参数保持显式
      'browse_page',
      { filters: browseFiltersSchema.parse(filters), offset, sessionId },
      feedPageSchema,
    ),
};
