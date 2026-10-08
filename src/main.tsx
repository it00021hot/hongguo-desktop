import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { RouterProvider, createRouter } from '@tanstack/react-router';
import { routeTree } from './routeTree.gen';
import { applyTheme, useThemeStore } from './lib/stores/theme';
import './styles/index.css';

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      // 桌面应用是本地数据源，窗口聚焦时重取意义不大
      refetchOnWindowFocus: false,
      retry: 1,
      staleTime: 5_000,
    },
  },
});

const router = createRouter({
  routeTree,
  context: { queryClient },
  // search 参数按原样字符串解析/序列化。默认实现走 JSON.parse/stringify：
  // 19 位纯数字剧集 id 会被 parse 成 number 丢精度（末位归零），
  // 分享/手输的裸数字深链因此落到错的剧。应用里 search 只装字符串
  // （detail 的 seriesId），字符串保真即可。
  parseSearch: (str) => Object.fromEntries(new URLSearchParams(str)),
  stringifySearch: (search) => {
    const params = new URLSearchParams();
    for (const [key, value] of Object.entries(search)) {
      if (value !== undefined) params.set(key, String(value));
    }
    // router 把返回值直接拼在 pathname 后（默认实现同款契约）：
    // 带 `?` 前缀、空 search 返回空串。漏了 `?` 会拼出
    // /detailseriesId=… 这种不存在的路径 → 整个详情页 Not Found
    const s = params.toString();
    return s ? `?${s}` : '';
  },
});

// 启动即应用主题，避免首屏闪烁
applyTheme(useThemeStore.getState().theme);

const container = document.getElementById('root');
if (!container) throw new Error('找不到 #root 挂载点');

createRoot(container).render(
  <StrictMode>
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>
  </StrictMode>,
);

// dev-reload
