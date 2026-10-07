import { createRootRoute, Outlet } from '@tanstack/react-router';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { TooltipProvider } from '@/components/ui/tooltip';
import { Toaster } from '@/components/ui/sonner';
import { AppShell } from '@/components/layout/app-shell';
import { useDownloadEvents } from '@/lib/queries';

/** 小窗播放（label = "mini"）：不进应用壳——侧栏/顶栏/关闭确认都不属于它，
 *  整个窗口就是播放器本体。窗口 label 是静态事实，模块加载时判一次即可。 */
const IS_MINI_WINDOW = getCurrentWindow().label === 'mini';

function RootComponent() {
  // 在根布局订阅下载事件：任何页面进入时队列更新都能收到
  // （小窗里没有下载 UI，但订阅无害且省得按路由分流）
  useDownloadEvents();

  return (
    <TooltipProvider>
      {IS_MINI_WINDOW ? (
        <Outlet />
      ) : (
        <AppShell>
          <Outlet />
        </AppShell>
      )}
      <Toaster />
    </TooltipProvider>
  );
}

export const Route = createRootRoute({
  component: RootComponent,
});
