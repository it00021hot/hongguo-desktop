import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { MutationCache, QueryCache, QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { ReactQueryDevtools } from '@tanstack/react-query-devtools';
import { RouterProvider, createRouter } from '@tanstack/react-router';
import { ErrorBoundary } from 'react-error-boundary';
import { toast } from 'sonner';
import { routeTree } from './pages/routeTree.gen';
import { PendingFallback, RendererCrashed, RouteError } from './components/common/route-fallbacks';
import { applyTheme, useThemeStore } from './stores/theme';
import { useUiStore } from './stores/ui';
import { setAndroidLandscape } from './utils/android-shell';
import './styles/index.css';

/** 全局错误 toast 的文案：kind 译文 + 后端原文（若两者不同）。
 * 「视频处理失败（接口返回 7001: 无权限）」比一句孤立译文可定位得多。 */
function toastText(error: unknown): string {
  const message = error instanceof Error ? error.message : String(error);
  const raw = error instanceof Error && error.cause != null ? error.cause : null;
  const cause = typeof raw === 'string' ? raw : raw instanceof Error ? raw.message : '';
  return cause && cause !== message ? `${message}（${cause}）` : message;
}

const queryClient = new QueryClient({
  // 全局错误面：查询/变更失败统一 toast（消息走 call() 归一的 i18n 文案）。组件层
  // 就地展示了错误时用 meta.silentOnError 跳过；kind 译文后括 error.cause 原文便于定位
  queryCache: new QueryCache({
    onError: (error, query) => {
      // 降级型查询（meta.silentOnError）失败本就有就地/无感处理，全局不再弹
      if (query?.meta?.silentOnError === true) return;
      toast.error(toastText(error));
    },
  }),
  mutationCache: new MutationCache({
    onError: (error, _vars, _ctx, mutation) => {
      if (mutation.meta?.silentOnError === true) return;
      toast.error(toastText(error));
    },
  }),
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
  // 渲染异常 / 不存在的路由给中文兜底页；页面 lazy 化后 pending 期间给骨架
  defaultErrorComponent: RouteError,
  defaultPendingComponent: PendingFallback,
  // search 参数按原样字符串解析/序列化：默认 JSON.parse 会把 19 位纯数字剧集
  // id parse 成 number 丢精度（末位归零），裸数字深链落到错的剧；应用里 search 只装字符串
  parseSearch: (str) => Object.fromEntries(new URLSearchParams(str)),
  stringifySearch: (search) => {
    const params = new URLSearchParams();
    for (const [key, value] of Object.entries(search)) {
      if (value !== undefined) params.set(key, String(value));
    }
    // 返回值直接拼在 pathname 后（默认实现同款契约）：带 `?` 前缀、空 search 返回空串
    const s = params.toString();
    return s ? `?${s}` : '';
  },
});

// 启动即应用主题，避免首屏闪烁
applyTheme(useThemeStore.getState().theme);

// Android 系统返回桥（MainActivity.onBackPressed 经 evaluateJavascript 调）：
// 横屏全屏中 → 先退出全屏；否则 web 路由回退；栈底转退出确认。返回 true = 已被前端消费。
declare global {
  interface Window {
    __hgAndroidBack?: () => boolean;
  }
}
window.__hgAndroidBack = () => {
  if (useUiStore.getState().playerFrameless) {
    useUiStore.getState().setPlayerFrameless(false);
    setAndroidLandscape(false);
    return true;
  }
  if (window.history.length > 1) {
    router.history.back();
    return true;
  }
  window.dispatchEvent(new CustomEvent('hongguo:android-exit'));
  return true;
};

const container = document.getElementById('root');
if (!container) throw new Error('找不到 #root 挂载点');

createRoot(container).render(
  <StrictMode>
    <QueryClientProvider client={queryClient}>
      {/* 顶层渲染异常（路由组件之外）兜底；路由内的由 defaultErrorComponent 接，应用壳得以保留 */}
      <ErrorBoundary FallbackComponent={RendererCrashed} onReset={() => window.location.reload()}>
        <RouterProvider router={router} />
      </ErrorBoundary>
      {import.meta.env.DEV && (
        <ReactQueryDevtools initialIsOpen={false} buttonPosition="bottom-left" />
      )}
    </QueryClientProvider>
  </StrictMode>,
);
