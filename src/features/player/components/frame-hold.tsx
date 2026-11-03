import { useEffect, useRef } from 'react';
import { cn } from '@/lib/utils';

/**
 * 末帧定格层：把 use-frame-hold 抓下的 canvas 帧钉在画面上，新流首帧出画后交叉淡出。
 * canvas 是切换提交期命令式抓取的产物，样式全走宿主子选择器、object-fit 与 <video> 同款（定格帧与真画面逐像素对齐）；
 * 渲染在带切换动画的舞台容器外——定格帧是静止参考帧，不跟新内容一起滑入。
 */
export function FrameHold({
  frame,
  hidden,
  cover,
}: {
  frame: HTMLCanvasElement;
  hidden: boolean;
  cover: boolean;
}) {
  const hostRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    hostRef.current?.replaceChildren(frame);
  }, [frame]);
  return (
    <div
      ref={hostRef}
      aria-hidden
      className={cn(
        'pointer-events-none absolute inset-0 z-[5] transition-opacity duration-300',
        cover ? '[&>canvas]:object-cover' : '[&>canvas]:object-contain',
        '[&>canvas]:size-full',
        hidden && 'opacity-0',
      )}
    />
  );
}
