import { useEffect, useRef, useState } from 'react';
import { Link, useRouterState } from '@tanstack/react-router';
import {
  Flame,
  Compass,
  History,
  Trophy,
  Sparkles,
  BellRing,
  Star,
  ThumbsUp,
  ListChecks,
  Combine,
  HardDrive,
  Settings,
  PanelLeftClose,
  PanelLeft,
  UserRound,
  LogOut,
} from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { ConfirmDialog } from '@/components/ui/confirm-dialog';
import { LoginDialog } from '@/features/settings/components/login-dialog';
import { useAccount, useAuthRefresh } from '@/lib/queries';
import { login } from '@/lib/ipc/commands';
import { toast } from 'sonner';
import { useUiStore } from '@/lib/stores/ui';
import { t } from '@/i18n';
import { cn } from '@/lib/utils';

/** 导航项定义。图标与 key 一一对应，`__root.tsx` 用它取标题。
 *
 * 播放页（/player）不设菜单入口：它只能由各页的「播放/继续播放」跳转进入，
 * 独立菜单 + 页内历史与独立的历史页重复。找剧排第二（用户指定，
 * 2026-10-08），紧跟首页推荐。 */
export const NAV_ITEMS = [
  { key: 'home', to: '/', icon: Flame },
  { key: 'browse', to: '/browse', icon: Compass },
  { key: 'rank', to: '/rank', icon: Trophy },
  { key: 'new', to: '/new', icon: Sparkles },
  { key: 'history', to: '/history', icon: History },
  { key: 'collections', to: '/collections', icon: Star },
  { key: 'liked', to: '/liked', icon: ThumbsUp },
  { key: 'reservations', to: '/reservations', icon: BellRing },
  { key: 'tasks', to: '/tasks', icon: ListChecks },
  { key: 'merge', to: '/merge', icon: Combine },
  { key: 'storage', to: '/storage', icon: HardDrive },
  { key: 'settings', to: '/settings', icon: Settings },
] as const;

export function AppSidebar() {
  const collapsed = useUiStore((s) => s.sidebarCollapsed);
  const toggle = useUiStore((s) => s.toggleSidebar);
  const pathname = useRouterState({ select: (s) => s.location.pathname });

  return (
    <aside
      data-collapsed={collapsed}
      className={cn(
        'bg-sidebar text-sidebar-foreground flex h-full shrink-0 flex-col border-r transition-[width] duration-200',
        collapsed ? 'w-14' : 'w-56',
      )}
    >
      {/* 顶栏（红绿灯/logo/应用名）横贯全宽，长在 AppShell 上——侧边栏
          从顶栏下方开始，折叠不再影响顶部区域。 */}
      <nav className="flex-1 space-y-1 p-2">
        {NAV_ITEMS.map((item) => {
          const active = item.to === '/' ? pathname === '/' : pathname.startsWith(item.to);
          const Icon = item.icon;
          const link = (
            <Link
              key={item.key}
              to={item.to}
              className={cn(
                'flex items-center gap-3 rounded-md px-3 py-2 text-sm transition-colors',
                active
                  ? 'bg-sidebar-primary text-sidebar-primary-foreground font-medium'
                  : 'text-sidebar-foreground hover:bg-sidebar-accent hover:text-sidebar-accent-foreground',
                collapsed && 'justify-center px-0',
              )}
            >
              <Icon className="size-4 shrink-0" />
              {!collapsed && <span className="truncate">{t(`nav.${item.key}.title`)}</span>}
            </Link>
          );

          if (!collapsed) return link;
          return (
            <Tooltip key={item.key}>
              <TooltipTrigger asChild>{link}</TooltipTrigger>
              {/* sideOffset 抬出 56px 侧栏：trigger 是整行 39px 宽，
                  默认 offset=4 会让浮层压在侧栏边框上 */}
              <TooltipContent side="right" sideOffset={10}>
                {t(`nav.${item.key}.title`)}
              </TooltipContent>
            </Tooltip>
          );
        })}
      </nav>

      {/* 我的 / 登录（hgplayer 同款贴底账户区）：未登录开登录弹窗，已登录显昵称 */}
      <div className="border-t p-2">
        <AccountButton collapsed={collapsed} />
      </div>

      <div className="border-t p-2">
        <Button
          variant="ghost"
          size="icon"
          onClick={toggle}
          className={cn('w-full', !collapsed && 'justify-start gap-2')}
          aria-label={collapsed ? t('nav.expand') : t('nav.collapse')}
        >
          {collapsed ? <PanelLeft className="size-4" /> : <PanelLeftClose className="size-4" />}
          {!collapsed && <span className="text-sm">{t('nav.collapse')}</span>}
        </Button>
      </div>
    </aside>
  );
}

/** 贴底账户入口：未登录「我的 / 登录」开登录弹窗；已登录显昵称 + 退出钮。 */
function AccountButton({ collapsed }: { collapsed: boolean }) {
  const { data: account } = useAccount();
  const refreshAuth = useAuthRefresh();
  const [loginOpen, setLoginOpen] = useState(false);
  // 退出登录是危险动作：清掉本机登录态，先确认再执行
  const [logoutConfirm, setLogoutConfirm] = useState(false);

  // 旧登录态库里没有头像：挂载后静默补拉一次 user_info（后端顺带刷新
  // 昵称落库）。一次性 flag 兜底——若服务端就是不回头像，也不反复重试。
  const avatarFetched = useRef(false);
  const hasAccount = account != null;
  useEffect(() => {
    if (avatarFetched.current || !hasAccount || account?.avatarUrl) return;
    avatarFetched.current = true;
    login
      .userInfo()
      .then(() => refreshAuth())
      .catch(() => {});
  }, [hasAccount, account?.avatarUrl, refreshAuth]);

  const label = account?.userName?.trim() || t('nav.accountFallback');

  const row = (
    <button
      type="button"
      onClick={() => {
        if (!account) setLoginOpen(true);
      }}
      className={cn(
        'text-sidebar-foreground hover:bg-sidebar-accent flex w-full cursor-pointer items-center gap-3 rounded-md px-3 py-2 text-sm transition-colors',
        collapsed && 'justify-center px-0',
        account && 'cursor-default',
      )}
      // 折叠态由 Radix Tooltip 出标签：原生 title 会叠出第二层浮层
    >
      {/* 有头像用官方头像（登录响应下发），没有退回通用图标——
          这样折叠态下也能一眼分出登录/未登录 */}
      {account?.avatarUrl ? (
        <img src={account.avatarUrl} alt="" className="size-5 shrink-0 rounded-full object-cover" />
      ) : (
        <UserRound className="size-4 shrink-0" />
      )}
      {!collapsed && (
        <>
          <span className="truncate">{label}</span>
          {!account && (
            <span className="text-primary ml-auto shrink-0 text-xs">{t('nav.login')}</span>
          )}
        </>
      )}
    </button>
  );

  const logout = () => {
    void login
      .logout()
      .then(() => {
        refreshAuth();
        toast.success(t('nav.loggedOut'));
        setLogoutConfirm(false);
      })
      .catch((e) => toast.error(String(e)));
  };

  return (
    <>
      {collapsed ? (
        <Tooltip>
          <TooltipTrigger asChild>{row}</TooltipTrigger>
          <TooltipContent side="right">{label}</TooltipContent>
        </Tooltip>
      ) : (
        row
      )}
      {account != null && (
        <Button
          variant="ghost"
          size="icon"
          onClick={() => setLogoutConfirm(true)}
          className={cn('text-muted-foreground size-7 w-full', !collapsed && 'justify-start gap-2')}
          title={t('nav.logout')}
        >
          <LogOut className="size-3.5" />
          {!collapsed && <span className="text-xs">{t('nav.logout')}</span>}
        </Button>
      )}
      <ConfirmDialog
        open={logoutConfirm}
        onOpenChange={setLogoutConfirm}
        title={t('nav.logoutConfirmTitle')}
        description={t('nav.logoutConfirmBody')}
        confirmLabel={t('nav.logout')}
        onConfirm={logout}
      />
      <LoginDialog open={loginOpen} onOpenChange={setLoginOpen} onSuccess={refreshAuth} />
    </>
  );
}
