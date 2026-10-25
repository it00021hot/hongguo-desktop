import { useEffect } from 'react';
import { Smile } from 'lucide-react';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import { DANMAKU_EMOJI_LIST } from '@/utils/danmaku-emoji';
import { t } from '@/locales';
import { cn } from '@/lib/utils';

/**
 * 弹幕发送框 / 评论区共用的表情选择入口：笑脸按钮 + 上拉网格。
 *
 * 焦点纪律（保住输入框光标的关键）：trigger 与表情格子
 * 都 mousedown preventDefault，且 Popover 的开/关自动聚焦一并禁掉——
 * 焦点全程留在输入框，插入永远落在真实光标处，选完接着打字无感；
 * 格子连选不关面板，点外部/Esc 由 Popover 收起。
 *
 * 滚轮纪律：面板打开期间在 document capture 层吞掉全部 wheel——
 * 「在选择器边上一滚就切集」不再可能；data-wheel-block 让播放器的
 * wheel 交互裁决跳过面板自身（网格滚动是浏览器默认行为）。
 *
 * 定位交给 Popover（side=top + 碰撞规避，回复框贴右缘自动收进来），
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
  useEffect(() => {
    if (!open) return;
    const stopWheel = (e: WheelEvent) => e.stopPropagation();
    document.addEventListener('wheel', stopWheel, { capture: true, passive: true });
    return () => document.removeEventListener('wheel', stopWheel, { capture: true });
  }, [open]);

  return (
    <Popover open={open} onOpenChange={onToggle}>
      <PopoverTrigger asChild>
        <button
          type="button"
          title={t('player.danmakuEmoji')}
          aria-label={t('player.danmakuEmoji')}
          // 焦点纪律：点按钮不夺走输入框焦点（光标原地不动）
          onMouseDown={(e) => e.preventDefault()}
          className={cn(
            'grid cursor-pointer place-items-center rounded-full px-1 text-white/60 transition-colors hover:text-white',
            open && 'text-white',
          )}
        >
          <Smile className="size-4" aria-hidden />
        </button>
      </PopoverTrigger>
      <PopoverContent
        side="top"
        align={align === 'right' ? 'end' : 'start'}
        sideOffset={8}
        // 焦点纪律的另一半：开关都不夺焦（radix 默认把焦点挪进浮层）
        onOpenAutoFocus={(e) => e.preventDefault()}
        onCloseAutoFocus={(e) => e.preventDefault()}
        data-wheel-block
        /* 播放器深色面板皮肤（消费方都在豁免区），盖掉 Popover 默认底色 */
        className="w-72 rounded-xl border-white/10 bg-black/85 p-2 backdrop-blur-sm"
      >
        {/* 网格：每格一张表情图，title 即 [名字] 代码 */}
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
              <img src={e.url} alt={e.name} draggable={false} className="size-6 object-contain" />
            </button>
          ))}
        </div>
      </PopoverContent>
    </Popover>
  );
}
