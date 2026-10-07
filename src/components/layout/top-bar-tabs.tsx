//! 顶栏中部 tab 插槽的共享渲染件（首页/排行榜/新剧共用）。
//!
//! 插槽本体是 AppShell 顶栏里 `id={TOP_BAR_SLOT_ID}` 的容器，页面把自己
//! 的分类 tab portal 进去——顶栏常驻薄行形态，页面里不再有第二层 tab 条。

import { useSyncExternalStore } from 'react';
import { createPortal } from 'react-dom';
import { TOP_BAR_SLOT_ID } from './app-shell';
import { cn } from '@/lib/utils';

// 插槽是 AppShell 顶栏里的容器、页面是它的兄弟子树——首帧 render 阶段
// DOM 还没提交，id 直取要等提交后才有值。用 useSyncExternalStore 订阅
// DOM 变化重查：插槽挂载/换页的瞬间自动跟上，不需要 effect 里 setState。
function subscribeSlot(onChange: () => void) {
  const observer = new MutationObserver(onChange);
  observer.observe(document.body, { childList: true, subtree: true });
  return () => observer.disconnect();
}

/**
 * 把内容 portal 进顶栏中部插槽。
 *
 * 插槽用 DOM id 直取：比 context/ref state 链路稳（那套在运行中出现过
 * 拿不到元素、tab 整排消失的问题）。
 */
export function TopBarTabsPortal({ children }: { children: React.ReactNode }) {
  const slot = useSyncExternalStore(
    subscribeSlot,
    () => document.getElementById(TOP_BAR_SLOT_ID),
    () => null,
  );
  if (!slot) return null;
  return createPortal(
    // pointer-events-auto：插槽容器是 none（空白带让给拖窗），内容自己接事件
    <div className="pointer-events-auto flex shrink-0 scrollbar-none items-center gap-0.5 overflow-x-auto">
      {children}
    </div>,
    slot,
  );
}

/** 顶栏里的一个 tab 胶囊：选中 = 主色胶囊，全应用同一套 tab 语言。 */
export function TopBarTab({
  active,
  onClick,
  children,
}: {
  active: boolean;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        'cursor-pointer rounded-full px-3 py-1 text-xs transition-[color,background-color,transform] focus-visible:outline-none active:scale-95',
        active
          ? 'bg-primary text-primary-foreground font-medium'
          : 'text-muted-foreground hover:bg-accent hover:text-accent-foreground',
      )}
    >
      {children}
    </button>
  );
}
