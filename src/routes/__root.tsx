import { createRootRoute, Outlet } from '@tanstack/react-router';
import { TooltipProvider } from '@/components/ui/tooltip';
import { Toaster } from '@/components/ui/sonner';
import { AppShell } from '@/components/layout/app-shell';
import { useDownloadEvents } from '@/lib/queries';

function RootComponent() {
  // 在根布局订阅下载事件：任何页面进入时队列更新都能收到
  useDownloadEvents();

  return (
    <TooltipProvider>
      <AppShell>
        <Outlet />
      </AppShell>
      <Toaster />
    </TooltipProvider>
  );
}

export const Route = createRootRoute({
  component: RootComponent,
});
