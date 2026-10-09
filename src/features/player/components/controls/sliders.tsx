/** 播放器自绘滑杆：竖向音量条（VolumePopup 用）与弹幕设置面板横滑条行（DisplaySlider）。 */
import { useRef } from 'react';

/**
 * 竖向滑条（音量浮层用）。div 自绘而不是 `<input type=range>` 转向：
 * 竖向 range 的厂商伪元素在 WebView2 上表现不可控，自绘三段（轨道/已填/
 * 滑块）和 ScrubBar 同一套视觉。
 */
export function VerticalSlider({
  value,
  onChange,
  label,
}: {
  value: number;
  onChange: (v: number) => void;
  label: string;
}) {
  const trackRef = useRef<HTMLDivElement>(null);

  const ratioAt = (clientY: number) => {
    const track = trackRef.current;
    if (!track) return 0;
    const rect = track.getBoundingClientRect();
    if (rect.height <= 0) return 0;
    // 竖向：顶部 = 1
    return Math.min(Math.max(1 - (clientY - rect.top) / rect.height, 0), 1);
  };

  return (
    <div
      ref={trackRef}
      role="slider"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={Math.round(value * 100)}
      tabIndex={0}
      className="relative h-24 w-6 cursor-pointer"
      onPointerDown={(e) => {
        e.currentTarget.setPointerCapture(e.pointerId);
        onChange(ratioAt(e.clientY));
      }}
      onPointerMove={(e) => {
        if (e.buttons === 1) onChange(ratioAt(e.clientY));
      }}
      onKeyDown={(e) => {
        if (e.key === 'ArrowUp') onChange(Math.min(value + 0.05, 1));
        if (e.key === 'ArrowDown') onChange(Math.max(value - 0.05, 0));
      }}
    >
      <div className="absolute top-0 bottom-0 left-1/2 w-1 -translate-x-1/2 rounded-full bg-white/25" />
      <div
        className="absolute bottom-0 left-1/2 w-1 -translate-x-1/2 rounded-full bg-white"
        style={{ height: `${value * 100}%` }}
      />
      <div
        className="absolute left-1/2 size-3 -translate-x-1/2 translate-y-1/2 rounded-full bg-white"
        style={{ bottom: `${value * 100}%` }}
      />
    </div>
  );
}

/** 弹幕设置面板的一行：label + 百分比 + 白色横滑条（复用 .volume-range 样式）。 */
export function DisplaySlider({
  label,
  value,
  min,
  max,
  onChange,
}: {
  label: string;
  value: number;
  min: number;
  max: number;
  onChange: (v: number) => void;
}) {
  return (
    <label className="flex flex-col gap-1.5">
      <span className="flex items-center justify-between text-xs text-white/90">
        {label}
        <span className="font-mono text-white/70">{Math.round(value * 100)}%</span>
      </span>
      <input
        type="range"
        min={min}
        max={max}
        step={0.05}
        value={value}
        onChange={(e) => onChange(Number(e.target.value))}
        aria-label={label}
        className="volume-range w-full"
      />
    </label>
  );
}
