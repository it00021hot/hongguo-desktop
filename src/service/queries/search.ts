import { useInfiniteQuery, useQuery } from '@tanstack/react-query';

import { seriesSearch } from '../commands';
import type { SearchPage, SearchResult } from '../schema';
import { dedupBy, fuzzyStaleTime, keys, useInfiniteStream } from './common';

// ---------------------------------------------------------------- 官方 App 搜索

/** 综合首页的精选与翻页列表有重复：按 seriesId 去重。 */
function appSearchItems(pages: SearchPage[]): SearchResult[] {
  return dedupBy(
    pages.flatMap((p) => p.items),
    (i) => i.seriesId,
  );
}

/**
 * 官方 App 搜索（无限滚动）。
 *
 * 首页只有「精选」少数几条（平台搜索的固定形态），`hasMore` 翻页才是
 * 完整列表；翻页必须带首页发放的 searchId。结果按关键词进缓存：
 * 重复搜索同一关键词秒出，不再全量重拉。
 */
export function useSeriesSearchApp(query: string) {
  const kw = query.trim();
  const query_ = useInfiniteQuery({
    queryKey: keys.appSeriesSearch(kw),
    queryFn: ({ pageParam }) =>
      pageParam.offset === 0
        ? seriesSearch.run(kw)
        : seriesSearch.run(kw, pageParam.offset, pageParam.searchId),
    initialPageParam: { offset: 0, searchId: '' },
    getNextPageParam: (last) =>
      last.hasMore ? { offset: last.nextOffset, searchId: last.searchId } : undefined,
    // 空关键词是浏览模式，不该发搜索请求
    enabled: kw !== '',
    staleTime: fuzzyStaleTime(keys.appSeriesSearch(kw)),
  });
  return useInfiniteStream(query_, appSearchItems);
}

/**
 * 搜索联想：输入即查，抖动节流内重复输入秒出。
 *
 * 防抖在调用侧（组件里 300ms 才把词递进来），这里只管查；
 * 失败后端已吞掉返回空表，下拉无感消失。
 */
export function useSearchSuggest(q: string) {
  const kw = q.trim();
  return useQuery({
    queryKey: keys.searchSuggest(kw),
    queryFn: () => seriesSearch.suggest(kw),
    // 至少 2 个字才值得打接口（单字联想噪声大）
    enabled: kw.length >= 2,
    staleTime: fuzzyStaleTime(keys.searchSuggest(kw)),
    retry: false,
  });
}
