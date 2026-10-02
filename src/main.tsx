import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { RouterProvider, createRouter } from '@tanstack/react-router';
import { routeTree } from './routeTree.gen';
import { applyTheme, useThemeStore } from './lib/stores/mod';
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