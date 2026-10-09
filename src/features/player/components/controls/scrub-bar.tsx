/** 自绘进度条：大屏控制栏与小屏控制条共用，大小屏进度条一个口径。 */
import { useRef } from 'react';
import { t } from '@/locales';

/**
 * 可点击可拖动的进度条。
 *
 * 用 div 而不是 `<input type=range>`：要显示缓冲进度、要跟随容器宽度，
 * 而 range 的原生滑块样式在 WebView2 上跨版本表现不一致。小窗控制条
 * （mini-screen-controls）复用同一份，大小屏进度条一个口径。
 */
export function ScrubBar({
  current,
  duration,
  onSeek,
  onScrubStart,
  onScrubEnd,
}: {
  current: number;
  duration: number;
  onSeek: (ratio: number) => void;
  onScrubStart: () => void;
  onScrubEnd: () => void;
}) {
  const trackRef = useRef<HTMLDivElement>(null);
  const ratio = duration > 0 ? Math.min(current / duration, 1) : 0;

  const ratioAt = (clientX: number) => {
    const track = trackRef.current;
    if (!track) return 0;
    const rect = track.getBoundingClientRect();
    if (rect.width <= 0) return 0;
    return (clientX - rect.left) / rect.width;
  };

  return (
    <div
      ref={trackRef}
      role="slider"
      aria-label={t('common.progress')}
      aria-valuemin={0}
      aria-valuemax={Math.floor(duration)}
      aria-valuenow={Math.floor(current)}
      tabIndex={0}
      className="group/bar relative h-4 w-full cursor-pointer"
      onPointerDown={(e) => {
        if (duration <= 0) return;
        e.currentTarget.setPointerCapture(e.pointerId);
        onScrubStart();
        onSeek(ratioAt(e.clientX));
      }}
      onPointerMove={(e) => {
        if (e.buttons === 1 && duration > 0) onSeek(ratioAt(e.clientX));
      }}
      onPointerUp={() => onScrubEnd()}
      onKeyDown={(e) => {
        if (e.key === 'ArrowLeft') onSeek(Math.max(ratio - 5 / (duration || 1), 0));
        if (e.key === 'ArrowRight') onSeek(Math.min(ratio + 5 / (duration || 1), 1));
      }}
    >
      {/* 进度条固定白色系，不跟主题走。静止态 2px 半透明（细条贴着画面
          不挡内容），悬停/拖动时涨到 4px 并提亮——B站/YouTube 同款的
          「平时隐身、上手好用」。 */}
      <div className="absolute inset-x-0 top-1/2 h-0.5 -translate-y-1/2 rounded-full bg-white/20 transition-all duration-150 group-hover/bar:h-1" />
      <div
        className="absolute top-1/2 left-0 h-0.5 -translate-y-1/2 rounded-full bg-white/60 transition-all duration-150 group-hover/bar:h-1 group-hover/bar:bg-white/90"
        style={{ width: `${ratio * 100}%` }}
      />
      <div
        className="absolute top-1/2 size-3 -translate-x-1/2 -translate-y-1/2 rounded-full bg-white opacity-0 transition-opacity group-hover/bar:opacity-100"
        style={{ left: `${ratio * 100}%` }}
      />
    </div>
  );
}
