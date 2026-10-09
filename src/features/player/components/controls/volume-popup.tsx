/** 音量按钮 + 竖条浮层：开合状态在全局播放 store。 */
import { useEffect, useRef } from 'react';
import { Volume2, VolumeX } from 'lucide-react';
import { usePlayerStore } from '@/stores/player';
import { t } from '@/locales';
import { IconButton } from './icon-button';
import { VerticalSlider } from './sliders';

/**
 * 音量按钮 + 竖条浮层。
 *
 * 开合：hover 展开、移出收起——但滑条 `setPointerCapture` 会诱发本层**伪
 * mouseleave**（点击/拖动音量的一瞬浮层被收走，想从 100 连点到 30 必须
 * 反复重开）。对策：浮层内任何 pointerdown 置 hold，mouseleave 见 hold
 * 不收；window pointerup 清 hold 后按指针落点裁决（还在按钮/浮层上就
 * 保持，出去了才收）。
 */
export function VolumePopup({
  volume,
  muted,
  onToggleMute,
  onSetVolume,
}: {
  volume: number;
  muted: boolean;
  onToggleMute: () => void;
  onSetVolume: (v: number) => void;
}) {
  const volumeOpen = usePlayerStore((s) => s.volumeOpen);
  const setVolumeOpen = usePlayerStore((s) => s.setVolumeOpen);
  const wrapRef = useRef<HTMLDivElement | null>(null);
  const holdRef = useRef(false);

  useEffect(() => {
    const onUp = (e: PointerEvent) => {
      if (!holdRef.current) return;
      holdRef.current = false;
      const { clientX: x, clientY: y } = e;
      // capture 释放要等事件派发完：推一拍再查落点，elementFromPoint 才准
      setTimeout(() => {
        const hit = document.elementFromPoint(x, y);
        const inside = hit != null && wrapRef.current?.contains(hit);
        if (!inside) setVolumeOpen(false);
      }, 0);
    };
    window.addEventListener('pointerup', onUp);
    return () => window.removeEventListener('pointerup', onUp);
  }, [setVolumeOpen]);

  return (
    <div
      ref={wrapRef}
      className="relative flex items-center"
      onMouseEnter={() => setVolumeOpen(true)}
      onMouseLeave={() => {
        if (holdRef.current) return;
        setVolumeOpen(false);
      }}
    >
      <IconButton label={t('player.mute')} onClick={onToggleMute}>
        {muted || volume === 0 ? <VolumeX className="size-4" /> : <Volume2 className="size-4" />}
      </IconButton>
      {volumeOpen && (
        // 浮层必须与按钮**几何贴合**（无 margin 间隙）：鼠标从按钮移向
        // 浮层的路径一旦离开 wrapper 的后代区域，mouseleave 就会把
        // 浮层整个卸载——间隙就是「想移过去却直接隐藏」的元凶。
        // 视觉留白放进浮层自己的 padding 里。
        <div
          className="absolute bottom-full left-1/2 -translate-x-1/2 rounded-lg bg-black/80 px-3 pt-1 pb-3 backdrop-blur-sm"
          onPointerDownCapture={() => {
            holdRef.current = true;
          }}
        >
          <div className="mb-1 text-center font-mono text-[10px] text-white/90">
            {Math.round((muted ? 0 : volume) * 100)}
          </div>
          <VerticalSlider
            value={muted ? 0 : volume}
            onChange={onSetVolume}
            label={t('player.volume')}
          />
        </div>
      )}
    </div>
  );
}
