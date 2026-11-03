import type { ReactNode } from 'react';
import { cn } from '@/lib/utils';

/**
 * 「旧数据刷新中」的统一视觉：内容降透明度 + 禁点，新数据到达后淡回。配合
 * keepPreviousData / placeholderData 用——真没数据（isPending）走骨架屏，别套这个（首次加载是一片不可点空白）。
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
