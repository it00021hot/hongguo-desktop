import { createRootRoute, Outlet } from '@tanstack/react-router';
import { useEffect } from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { TooltipProvider } from '@/components/ui/tooltip';
import { Toaster } from '@/components/ui/sonner';
import { AppShell } from '@/components/layout/app-shell';
import { UpdateDialog } from '@/features/update/update-dialog';
import { UpdateProvider } from '@/features/update/update-provider';
import { useDownloadEvents } from '@/service/queries';
import { useLocaleStore } from '@/stores/locale';
import { t } from '@/locales';

/** 小窗播放（label = "mini"）：不进应用壳——侧栏/顶栏/关闭确认都不属于它，
 *  整个窗口就是播放器本体。窗口 label 是静态事实，模块加载时判一次即可。 */
const IS_MINI_WINDOW = getCurrentWindow().label === 'mini';

function RootComponent() {
  // 在根布局订阅下载事件：任何页面进入时队列更新都能收到
  // （小窗里没有下载 UI，但订阅无害且省得按路由分流）
  useDownloadEvents();

  // 原生窗口标题跟随界面语言。Dock/包名由 InfoPlist.strings 本地化
  // （见 scripts/make-macos-lproj.py），而 Mission Control 与「窗口」菜单
  // 读的是窗口标题——不同步就会与 Dock 的叫法对不上（中文系统里一个
  // 「红果播放器」一个「红果短剧」）。语言切换后立即生效。
  const locale = useLocaleStore((s) => s.locale);
  useEffect(() => {
    if (IS_MINI_WINDOW) return;
    void getCurrentWindow()
      .setTitle(t('app.name'))
      .catch(() => undefined);
  }, [locale]);

  return (
    <TooltipProvider>
      {IS_MINI_WINDOW ? (
        <Outlet />
      ) : (
        // 更新检查/弹层只在主窗口：小窗是纯播放器，不该弹更新框
        <UpdateProvider>
          <AppShell>
            <Outlet />
          </AppShell>
          <UpdateDialog />
        </UpdateProvider>
      )}
      <Toaster />
    </TooltipProvider>
  );
}

export const Route = createRootRoute({
  component: RootComponent,
});
