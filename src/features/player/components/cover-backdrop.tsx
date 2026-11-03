import { useState } from 'react';
import { cn } from '@/lib/utils';

/**
 * 封面占位图（裂图自愈）：onError 后整层退场，不留破图标压在画面上。
 * `hidden` 是「视频已出画」：淡出而非卸载，切剧时 key 随 src 变化重挂载、状态自然归零。
 */
export function CoverBackdrop({ src, hidden }: { src: string; hidden: boolean }) {
  const [broken, setBroken] = useState(false);
  if (broken) return null;
  return (
    <img
      src={src}
      alt=""
      aria-hidden
      onError={() => setBroken(true)}
      className={cn(
        'pointer-events-none absolute inset-0 z-[5] size-full object-cover object-top brightness-[0.55]',
        'transition-opacity duration-500',
        hidden ? 'opacity-0' : 'opacity-100',
      )}
    />
  );
}
