import type { ReactNode } from 'react';
import { cn } from '@/lib/utils';

/**
 * 「旧数据刷新中」的统一视觉：内容降透明度 + 禁点，新数据到达后淡回。
 *
 * 配合 TanStack 的 keepPreviousData / placeholderData 使用——只在手里有
 * 旧数据、后台在取新数据时激活；真没数据（isPending）的场景走骨架屏，
 * 不要套这个组件，否则首次加载是一片不可点的空白。
 *
 * 禁点是刻意的：刷新期间旧列表上的按钮（翻页/筛选/预约）点了都会以
 * 旧状态为准，半透明展示可以，误操作不行。
 */
export function RefreshShade({
  refreshing,
  children,
  className,
}: {
  refreshing: boolean;
  children: ReactNode;
  className?: string;
}) {
  return (
    <div
      className={cn(
        'transition-opacity duration-200',
        refreshing && 'pointer-events-none opacity-50',
        className,
      )}
    >
      {children}
    </div>
  );
}
