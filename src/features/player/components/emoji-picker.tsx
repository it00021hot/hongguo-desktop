import { useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { Smile } from 'lucide-react';
import { DANMAKU_EMOJI_LIST } from '@/utils/danmaku-emoji';
import { t } from '@/locales';
import { cn } from '@/lib/utils';

/**
 * 弹幕发送框 / 评论区共用的表情选择入口：笑脸按钮 + 上拉网格。
 *
 * 焦点纪律（保住输入框光标的关键，对齐 hgplayer）：trigger 与表情格子
 * 都 mousedown preventDefault——焦点全程留在输入框，插入永远落在真实
 * 光标处，选完接着打字无感；格子连选不关面板，点外部/Esc 收起。
 *
 * 滚轮纪律：面板打开期间在 document capture 层吞掉全部 wheel——
 * 「在选择器边上一滚就切集」不再可能；网格自身滚动是浏览器默认行为，
 * 不受 stopPropagation 影响。
 *
 * 面板 portal 到 body 用 fixed 定位：回复行在滚动容器里也不会被裁。
 * 插入逻辑归使用方，这里只报「选了哪个表情」（名字不带方括号）。
 */
export function EmojiPickerButton({
  open,
  onToggle,
  onPick,
  align = 'left',
}: {
  open: boolean;
  onToggle: () => void;
  onPick: (name: string) => void;
  align?: 'left' | 'right';
}) {
  const rootRef = useRef<HTMLDivElement | null>(null);
  const popRef = useRef<HTMLDivElement | null>(null);
  /** fixed 定位的锚点（触发按钮上沿），打开瞬间量一次 */
  const [anchor, setAnchor] = useState<{ left: number; bottom: number } | null>(null);

  const toggle = () => {
    const btn = rootRef.current?.querySelector('button');
    const r = btn?.getBoundingClientRect();
    if (r) {
      setAnchor({
        left: align === 'right' ? r.right - 288 : r.left,
        bottom: window.innerHeight - r.top + 8,
      });
    }
    onToggle();
  };

  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      const target = e.target as Node;
      if (rootRef.current?.contains(target) || popRef.current?.contains(target)) return;
      onToggle();
    };
    const stopWheel = (e: WheelEvent) => e.stopPropagation();
    document.addEventListener('mousedown', onDown);
    document.addEventListener('wheel', stopWheel, { capture: true, passive: true });
    return () => {
      document.removeEventListener('mousedown', onDown);
      document.removeEventListener('wheel', stopWheel, { capture: true });
    };
  }, [open, onToggle]);

  return (
    <div ref={rootRef} className="relative flex shrink-0 items-center">
      <button
        type="button"
        title={t('player.danmakuEmoji')}
        aria-label={t('player.danmakuEmoji')}
        // 焦点纪律：点按钮不夺走输入框焦点（光标原地不动）
        onMouseDown={(e) => e.preventDefault()}
        onClick={toggle}
        className={cn(
          'grid cursor-pointer place-items-center rounded-full px-1 text-white/60 transition-colors hover:text-white',
          open && 'text-white',
        )}
      >
        <Smile className="size-4" aria-hidden />
      </button>
      {open &&
        anchor &&
        createPortal(
          <div
            ref={popRef}
            data-wheel-block
            style={{ position: 'fixed', left: anchor.left, bottom: anchor.bottom }}
            className="z-50 w-72 rounded-xl border border-white/10 bg-black/85 p-2 backdrop-blur-sm"
          >
            {/* hgplayer 同款网格：每格一张表情图，title 即 [名字] 代码 */}
            <div className="grid max-h-44 scrollbar-thin grid-cols-8 gap-0.5 overflow-y-auto">
              {DANMAKU_EMOJI_LIST.map((e) => (
                <button
                  key={e.name}
                  type="button"
                  title={e.name}
                  onMouseDown={(e) => e.preventDefault()}
                  onClick={() => onPick(e.name.slice(1, -1))}
                  className="grid cursor-pointer place-items-center rounded-md p-1 transition-colors hover:bg-white/15"
                >
                  <img
                    src={e.url}
                    alt={e.name}
                    draggable={false}
                    className="size-6 object-contain"
                  />
                </button>
              ))}
            </div>
          </div>,
          document.body,
        )}
    </div>
  );
}
