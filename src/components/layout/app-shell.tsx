import { useRouterState } from '@tanstack/react-router';
import { AppSidebar, NAV_ITEMS } from './app-sidebar';
import { ThemeSwitch } from './theme-switch';
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

  return (
    <div className="flex h-screen w-screen overflow-hidden">
      <a href="#content" className="skip-to-content">
        跳到主内容
      </a>

      <AppSidebar />

      <div className="flex min-w-0 flex-1 flex-col">
        <header className="flex h-14 shrink-0 items-center justify-between gap-4 border-b px-6">
          <div className="grid gap-0.5">
            <h1 className="truncate text-base font-semibold">{t(`nav.${navKey}.title`)}</h1>
            <p className="text-muted-foreground hidden truncate text-xs md:block">
              {t(`nav.${navKey}.description`)}
            </p>
          </div>
          <ThemeSwitch />
        </header>

        <main id="content" className="scrollbar-thin min-h-0 flex-1 overflow-y-auto">
          {children}
        </main>
      </div>
    </div>
  );
}
