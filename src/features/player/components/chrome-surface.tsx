/**
 * 播放器悬浮层统一显隐壳。
 *
 * 所有压在画面上的 chrome（控制栏/简介/互动栏/小屏紧凑条/连播角标…）的
 * 「随 chromeShown 淡入淡出、隐藏时不接指针」行为只允许从这里出——
 * 各组件自写 opacity/pointer-events 就会出现「控制栏收了简介没收」的
 * 精神分裂。定位与内容皮肤仍由使用方经 className 给，这里只管显隐。
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
