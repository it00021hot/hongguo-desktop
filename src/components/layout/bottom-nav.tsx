import { Link, useRouterState } from '@tanstack/react-router';
import { Flame, Compass, Trophy, History, UserRound } from 'lucide-react';
import { useAccount } from '@/service/queries';
import { t } from '@/locales';
import { cn } from '@/lib/utils';

/**
 * 移动端底部导航条（窄屏专属，`md` 以下显示——桌面由侧边栏承担导航）。
 *
 * 形态对齐官方 App 的底部 tab：等宽五项、图标+文字、当前项高亮。入口取
 * 侧边栏 NAV_ITEMS 的核心五项（导航 key 与路由一一对应，标题同一 i18n 源），
 * 账户入口直接进设置页（登录弹窗在设置页账户卡同一入口，窄屏不重复做一套）。
 */
const TAB_ITEMS = [
  { to: '/', icon: Flame, labelKey: 'nav.home.title' },
  { to: '/browse', icon: Compass, labelKey: 'nav.browse.title' },
  { to: '/rank', icon: Trophy, labelKey: 'nav.rank.title' },
  { to: '/history', icon: History, labelKey: 'nav.history.title' },
  { to: '/settings', icon: UserRound, labelKey: 'nav.settings.title' },
] as const;

export function BottomNav() {
  const pathname = useRouterState({ select: (s) => s.location.pathname });
  const { data: account } = useAccount();
  const hasAccount = account != null;

  return (
    <nav
      aria-label={t('nav.expand')}
      className="bg-sidebar text-sidebar-foreground sticky bottom-0 z-20 grid grid-cols-5 border-t md:hidden"
    >
      {TAB_ITEMS.map(({ to, icon: Icon, labelKey }) => {
        const active = to === '/' ? pathname === '/' : pathname.startsWith(to);
        // 「我的」入口：未登录显登录态文案，已登录与桌面账户区同语义
        const label = to === '/settings' && !hasAccount ? t('nav.login') : t(labelKey);
        return (
          <Link
            key={to}
            to={to}
            className={cn(
              'flex flex-col items-center gap-0.5 py-1.5 text-[10px] transition-colors',
              active
                ? 'text-primary'
                : 'text-sidebar-foreground/70 hover:bg-sidebar-accent hover:text-sidebar-accent-foreground',
            )}
          >
            <Icon className="size-5" />
            <span className="truncate">{label}</span>
          </Link>
        );
      })}
    </nav>
  );
}
