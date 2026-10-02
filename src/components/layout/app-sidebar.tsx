import { Link, useRouterState } from '@tanstack/react-router';
import {
  Compass,
  Download,
  Play,
  ListChecks,
  Combine,
  HardDrive,
  Settings,
  PanelLeftClose,
  PanelLeft,
} from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { useUiStore } from '@/lib/stores/ui';
import { usePlayerStore } from '@/lib/stores/player';
import { t } from '@/i18n';
import { cn } from '@/lib/utils';

/** 导航项定义。图标与 key 一一对应，`__root.tsx` 用它取标题。 */
export const NAV_ITEMS = [
  { key: 'browse', to: '/', icon: Compass },
  { key: 'download', to: '/download', icon: Download },
  { key: 'player', to: '/player', icon: Play },
  { key: 'tasks', to: '/tasks', icon: ListChecks },
  { key: 'merge', to: '/merge', icon: Combine },
  { key: 'storage', to: '/storage', icon: HardDrive },
  { key: 'settings', to: '/settings', icon: Settings },
] as const;

export function AppSidebar() {
  const collapsed = useUiStore((s) => s.sidebarCollapsed);
  const toggle = useUiStore((s) => s.toggleSidebar);
  const clearTarget = usePlayerStore((s) => s.clear);
  const pathname = useRouterState({ select: (s) => s.location.pathname });

  return (
    <aside
      data-collapsed={collapsed}
      className={cn(
        'bg-sidebar text-sidebar-foreground flex h-full shrink-0 flex-col border-r transition-[width] duration-200',
        collapsed ? 'w-14' : 'w-56',
      )}
    >
      <div className="flex h-14 items-center gap-2 border-b px-3">
        <span className="bg-primary text-primary-foreground grid size-8 shrink-0 place-items-center rounded-lg text-sm font-semibold">
          红
        </span>
        {!collapsed && (
          <div className="min-w-0 flex-1">
            <p className="truncate text-sm font-semibold">{t('app.name')}</p>
            <p className="text-muted-foreground truncate text-xs">{t('app.tagline')}</p>
          </div>
        )}
      </div>

      <nav className="flex-1 space-y-1 p-2">
        {NAV_ITEMS.map((item) => {
          const active = item.to === '/' ? pathname === '/' : pathname.startsWith(item.to);
          const Icon = item.icon;
          const link = (
            <Link
              key={item.key}
              to={item.to}
              // 「播放」项的含义是播放记录页。播放中点它必须清掉目标：
              // 不清的话路由回到 /player 而 store 里还指着同一集，
              // 渲染的还是同一个播放器——按钮看着能点，什么也没发生，
              // 播完想换一部剧就没有回去的入口了。
              onClick={item.key === 'player' ? clearTarget : undefined}
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
              <TooltipContent side="right">{t(`nav.${item.key}.title`)}</TooltipContent>
            </Tooltip>
          );
        })}
      </nav>

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
