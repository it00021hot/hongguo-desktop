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
            不额外占一行标题栏——应用名侧边栏顶部已经有了。

            ⚠️ 必须写 `deep`：Tauri 2.x 的 `data-tauri-drag-region` 裸属性**只认
            自己**，点子元素会被判成「不是拖拽区」而直接返回 false（见 tauri 的
            src/window/scripts/drag.js）。所以整棵子树都要能拖，就得显式写 deep。
            同样地，标题那两行**不能**再挂裸属性——它们会先于 header 命中并把
            拖拽挡掉。按钮不受影响：drag.js 里 clickable 元素会直接阻断拖拽、
            放行点击。 */}
        <header
          data-tauri-drag-region="deep"
          className="flex h-14 shrink-0 items-center justify-between gap-4 border-b pl-6"
        >
          <div className="grid gap-0.5">
            <h1 className="truncate text-base font-semibold">{t(`nav.${navKey}.title`)}</h1>
            <p className="text-muted-foreground hidden truncate text-xs md:block">
              {t(`nav.${navKey}.description`)}
            </p>
          </div>
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
