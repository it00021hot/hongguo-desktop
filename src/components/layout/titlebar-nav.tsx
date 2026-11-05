/** 顶栏左簇：后退 / 前进 / 侧栏折叠（红绿灯右侧，IDE 同款）。
 *  推荐播「返回」pill 已撤；从其它页起播靠后退钮 + returnTo。 */
import { useCallback } from 'react';
import { ArrowLeft, ArrowRight, PanelLeft, PanelLeftClose } from 'lucide-react';
import { useRouter, useRouterState } from '@tanstack/react-router';
import { Button } from '@/components/ui/button';
import { usePlayerStore } from '@/stores/player';
import { useUiStore } from '@/stores/ui';
import { t } from '@/locales';

/** TanStack Router 在 history.state 上挂的 idx；DOM 类型是 any，收成 unknown 再收窄。 */
function historyIdx(): number {
  if (typeof window === 'undefined') return 0;
  const state: unknown = window.history.state;
  if (typeof state !== 'object' || state === null || !('idx' in state)) return 0;
  const idx = Reflect.get(state, 'idx');
  return typeof idx === 'number' ? idx : 0;
}

/** useRouter() 的 history 在本仓库 ESLint 下被推成 any；与详情页同口径收窄。 */
type NavHistory = {
  push: (to: string) => void;
  back: () => void;
  forward: () => void;
};

export function TitlebarNav() {
  const router = useRouter() as { history: NavHistory };
  const locationKey = useRouterState({ select: (s) => s.location.href });
  const collapsed = useUiStore((s) => s.sidebarCollapsed);
  const toggleSidebar = useUiStore((s) => s.toggleSidebar);
  const returnTo = usePlayerStore((s) => s.returnTo);
  const setReturnTo = usePlayerStore((s) => s.setReturnTo);
  // 读 locationKey 仅为订阅路由变化；idx 取自 history.state
  void locationKey;
  const idx = historyIdx();
  const canBack = idx > 0 || !!returnTo;
  const canForward =
    typeof window !== 'undefined' && idx < Math.max(0, window.history.length - 1);

  const goBack = useCallback(() => {
    const target = usePlayerStore.getState().returnTo;
    if (target) {
      setReturnTo(null);
      router.history.push(target);
      return;
    }
    router.history.back();
  }, [router, setReturnTo]);

  const goForward = useCallback(() => {
    router.history.forward();
  }, [router]);

  // 顶栏 44px 里 items-center；红绿灯由壳层按 Cursor 公式钉 origin，这里不动。
  // 命中盒 28（size-7）+ 图标 16（size-4）。
  return (
    <div className="relative z-10 flex h-full shrink-0 items-center gap-0.5">
      <Button
        variant="ghost"
        size="icon-sm"
        aria-label={t('nav.back')}
        title={t('nav.backHint')}
        disabled={!canBack}
        onClick={goBack}
        className="size-7"
      >
        <ArrowLeft className="size-4" />
      </Button>
      <Button
        variant="ghost"
        size="icon-sm"
        aria-label={t('nav.forward')}
        title={t('nav.forwardHint')}
        disabled={!canForward}
        onClick={goForward}
        className="size-7"
      >
        <ArrowRight className="size-4" />
      </Button>
      <Button
        variant="ghost"
        size="icon-sm"
        aria-label={collapsed ? t('nav.expand') : t('nav.collapse')}
        title={collapsed ? t('nav.expand') : t('nav.collapse')}
        onClick={toggleSidebar}
        className="hidden size-7 md:inline-flex"
      >
        {collapsed ? <PanelLeft className="size-4" /> : <PanelLeftClose className="size-4" />}
      </Button>
    </div>
  );
}
