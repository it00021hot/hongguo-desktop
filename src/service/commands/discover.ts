import { browseFiltersSchema, type BrowseFilters } from '../schema';
import { call } from '../tauri/invoke';

// ---------------------------------------------------------------- 发现（首页推荐流 / 找剧）

export const discover = {
  /**
   * 首页推荐流（书城换一换，recommend/tab 同源）。
   * tab：'16'=推荐、'36'=漫剧、'39'=真人剧；sessionId 首页空串（走
   * bookmall/tab cr=4），翻页回传上一页会话 + offset（nextOffset）+
   * filterIds（已下发过的 series_id，服务端排除已见）。
   */
  recommendFeed: (tab: string, offset: number, sessionId: string, filterIds: string[]) =>
    call('recommend_feed', { tab, offset, sessionId, filterIds }),
  /** 首页顶栏 tab 列表（服务端下发；关注 45 的显隐由壳按登录态过滤） */
  recommendTabs: () => call('recommend_tabs'),
  /** 找剧筛选面板（八行维度选项） */
  browsePanel: () => call('browse_panel'),
  /** 找剧一页结果（多维筛选，服务端过滤；sessionId 首页空串、翻页回传） */
  browsePage: (filters: BrowseFilters, offset: number, sessionId = '') =>
    call(
      'browse_page',
      // zod default 把 undefined 归一成空串，IPC 参数保持显式
      { filters: browseFiltersSchema.parse(filters), offset, sessionId },
    ),
};
