import * as React from 'react';
import { cn } from '@/lib/utils';

interface ProgressProps extends React.ComponentProps<'div'> {
  value?: number;
  /** 进度条颜色，用于区分完成 / 失败 / 运行中 */
  tone?: 'default' | 'success' | 'destructive';
}

function Progress({ className, value = 0, tone = 'default', ...props }: ProgressProps) {
  const clamped = Math.min(100, Math.max(0, value));
  const toneClass =
    tone === 'success' ? 'bg-success' : tone === 'destructive' ? 'bg-destructive' : 'bg-primary';
  return (
    <div
      data-slot="progress"
      role="progressbar"
      aria-valuenow={Math.round(clamped)}
      aria-valuemin={0}
      aria-valuemax={100}
      className={cn('bg-secondary relative h-1.5 w-full overflow-hidden rounded-full', className)}
      {...props}
    >
      <div
        data-slot="progress-indicator"
        className={cn('h-full w-full flex-1 transition-transform', toneClass)}
        style={{ transform: `translateX(-${100 - clamped}%)` }}
      />
    </div>
  );
}

export { Progress };
