import { useRouterState } from '@tanstack/react-router';
import { AppSidebar, NAV_ITEMS } from './app-sidebar';
import { ThemeSwitch } from './theme-switch';
import { WindowButtons } from './window-controls';
import { isMac } from '@/lib/platform';
import { t } from '@/i18n';

/** 由当前路径反查导航 key，用于顶栏标题。 */
function navKeyFor(pathname: string): string {
  if (pathname === '/') return 'browse';
  const hit = NAV_ITEMS.find((item) => item.to !== '/' && pathname.startsWith(item.to));
  return hit?.key ?? 'browse';
}

export function AppShell({ children }: { children: React.ReactNode }) {
  const pathname = useRouterState({ select: (s) => s.location.pathname });
  const navKey = navKeyFor(pathname);
  // mac 的交通灯在侧边栏左上（见 app-sidebar），主区不重复给一套
  const mac = isMac();

  return (
    <div className="flex h-screen w-screen overflow-hidden">
      <a href="#content" className="skip-to-content">
        {t('common.skipToContent')}
      </a>

      <AppSidebar />

      <div className="flex min-w-0 flex-1 flex-col">
        {/* 窗口无边框，这行顶栏兼作拖拽区。按钮与主题切换在右侧成组，
            不额外占一行标题栏——应用名侧边栏顶部已经有了。 */}
        <header
          data-tauri-drag-region
          className="flex h-14 shrink-0 items-center justify-between gap-4 border-b pl-6"
        >
          <div className="grid gap-0.5">
            <h1 data-tauri-drag-region className="truncate text-base font-semibold">
              {t(`nav.${navKey}.title`)}
            </h1>
            <p
              data-tauri-drag-region
              className="text-muted-foreground hidden truncate text-xs md:block"
            >
              {t(`nav.${navKey}.description`)}
            </p>
          </div>
          {/* 拖拽区不能盖住可点元素：主题切换与窗口按钮都不带该属性 */}
          <div className="flex shrink-0 items-center">
            <ThemeSwitch />
            {!mac && <WindowButtons />}
          </div>
        </header>

        <main id="content" className="min-h-0 flex-1 scrollbar-thin overflow-y-auto">
          {children}
        </main>
      </div>
    </div>
  );
}
