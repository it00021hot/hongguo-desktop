import { useRouterState } from '@tanstack/react-router';
import { AppSidebar, NAV_ITEMS } from './app-sidebar';
import { ThemeSwitch } from './theme-switch';
import { WindowButtons } from './window-controls';
import { isMac } from '@/lib/platform';
import { t } from '@/i18n';

/** 由当前路径反查导航 key，用于顶栏标题。 */
function navKeyFor(pathname: string): string {
  if (pathname === '/') return 'browse';
  // 播放页不在侧栏菜单里（只能由播放入口跳转），标题单独指认
  if (pathname.startsWith('/player')) return 'player';
  const hit = NAV_ITEMS.find((item) => item.to !== '/' && pathname.startsWith(item.to));
  return hit?.key ?? 'browse';
}

export function AppShell({ children }: { children: React.ReactNode }) {
  const pathname = useRouterState({ select: (s) => s.location.pathname });
  const navKey = navKeyFor(pathname);
  // 沉浸流（/）：顶栏只留浮动的窗口按钮，56px 标题行还给画面
  const immersive = pathname === '/';
  // mac 的交通灯在侧边栏左上（见 app-sidebar），主区不重复给一套
  const mac = isMac();

  return (
    <div className="flex h-screen w-screen overflow-hidden">
      <a href="#content" className="skip-to-content">
        {t('common.skipToContent')}
      </a>

      <AppSidebar />

      <div className="relative flex min-w-0 flex-1 flex-col">
        {/* 窗口无边框，这行顶栏兼作拖拽区。按钮与主题切换在右侧成组，
            不额外占一行标题栏——应用名侧边栏顶部已经有了。

            ⚠️ 必须写 `deep`：Tauri 2.x 的 `data-tauri-drag-region` 裸属性**只认
            自己**，点子元素会被判成「不是拖拽区」而直接返回 false（见 tauri 的
            src/window/scripts/drag.js）。所以整棵子树都要能拖，就得显式写 deep。

            沉浸流（/）：标题整行撤掉，窗口按钮浮在画面右上——56px 还给画面。 */}
        {immersive ? (
          <header
            data-tauri-drag-region="deep"
            className="absolute right-0 top-0 z-50 flex h-14 items-center justify-end pr-2"
          >
            <div className="flex shrink-0 items-center">
              <ThemeSwitch />
              {!mac && <WindowButtons />}
            </div>
          </header>
        ) : (
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
            {/* 页面级头部扩展位：页面经 portal 往这里挂内容（如排行榜的
                内容 tab 与标题同行，hgplayer 同款）。空页面时整块随
                justify-between 塌掉，不影响其他页。 */}
            <div id="header-extra" className="ml-4 min-w-0 flex-1 self-stretch" />
            <div className="flex shrink-0 items-center">
              <ThemeSwitch />
              {!mac && <WindowButtons />}
            </div>
          </header>
        )}

        <main id="content" className="min-h-0 flex-1 scrollbar-thin overflow-y-auto">
          {children}
        </main>
      </div>
    </div>
  );
}
