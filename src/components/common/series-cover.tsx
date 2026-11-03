import { useState } from 'react';
import { Tv } from 'lucide-react';
import { cn } from '@/lib/utils';
import { enhancedCover, isRenderableCover } from '@/utils/cover';

interface SeriesCoverProps {
  /** 接口给的原始封面 URL，红果系条目多为 HEIC（WebView2 直挂必裂） */
  cover: string;
  alt: string;
  /** 追加在 `block size-full object-cover` 之后的类 */
  className?: string;
}

/**
 * 剧集封面的统一渲染口径——全应用所有封面出口都该走这一个组件。
 * HEIC 源只有装了 HEVC 扩展的 WebView2 能解（mac 直挂必裂于 Windows）：能直接渲染的原样加载，
 * 不能的走 hongguo-cover 本地转码代理，失败显示占位图。onError 受控且能自动复位（按失败时的 src 对号）。
 */
export function SeriesCover({ cover, alt, className }: SeriesCoverProps) {
  const enhanced = enhancedCover(cover);
  const src = enhanced ?? (isRenderableCover(cover) ? cover : '');
  const [brokenFor, setBrokenFor] = useState('');
  const imgBroken = brokenFor !== '' && brokenFor === src;
  return imgBroken || src === '' ? (
    <div className="text-muted-foreground grid size-full place-items-center">
      <Tv className="size-8" aria-hidden />
    </div>
  ) : (
    <img
      src={src}
      alt={alt}
      loading="lazy"
      className={cn('block size-full object-cover', className)}
      onError={() => setBrokenFor(src)}
    />
  );
}
