/**
 * 播放器悬浮层统一显隐壳：所有压在画面上的 chrome 的「随 shown 淡入淡出、隐藏时不接指针」
 * 行为只允许从这里出（各组件自写会出现「控制栏收了简介没收」）。定位与皮肤由使用方经 className 给。
 */
import type { HTMLAttributes } from 'react';
import { cn } from '@/lib/utils';

export function ChromeSurface({
  shown,
  className,
  ...rest
}: HTMLAttributes<HTMLDivElement> & { shown: boolean }) {
  return (
    <div
      aria-hidden={!shown}
      data-shown={shown}
      className={cn(
        'transition-opacity duration-200',
        shown ? 'opacity-100' : 'pointer-events-none opacity-0',
        className,
      )}
      {...rest}
    />
  );
}
