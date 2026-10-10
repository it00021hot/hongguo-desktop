import { call } from '../tauri/invoke';
import { searchPageSchema, suggestItemSchema, type SearchPage, type SuggestItem } from '../schema';

/** 官方 App 搜索（综合 tab，首页是精选少数，翻页才是完整列表）。 */
export const seriesSearch = {
  run: (query: string, offset?: number, searchId?: string) =>
    call<SearchPage>(
      'search_series_cmd',
      offset != null && offset > 0 ? { query, offset, searchId: searchId ?? '' } : { query },
      searchPageSchema,
    ),
  /** 输入联想（失败后端已吞掉返回空表，前端无感降级） */
  suggest: (q: string) =>
    call<SuggestItem[]>('search_suggest_cmd', { q }, suggestItemSchema.array()),
};
