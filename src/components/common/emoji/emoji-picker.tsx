import { useEffect } from 'react';
import { Smile } from 'lucide-react';
import { Popover, PopoverAnchor, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import { t } from '@/locales';
import { cn } from '@/lib/utils';
import type { Emoticon } from '@/service/schema';
import { EmoticonPanel } from './emoticon-panel';

/**
 * 弹幕发送框 / 评论区共用的表情选择入口：笑脸按钮 + 面板。
 * 焦点纪律：trigger 与格子均 mousedown preventDefault、禁 Popover 自动聚焦——焦点全程留在输入框，选完接着打字；格子连选不关面板。
 * 滚轮纪律：面板打开期间 document capture 层吞掉全部 wheel（「在选择器边上一滚就切集」不可能发生）；
 * 默认贴按钮浮起，发送区带附加行时改锚 `send-area`（面板浮在评论列表上方）。
 */
export function EmojiPickerButton({
  open,
  onToggle,
  onPick,
  onPickImage,
  align = 'left',
  anchorTo = 'trigger',
}: {
  open: boolean;
  onToggle: () => void;
  onPick: (name: string) => void;
  /** 选中表情图片（第三方 GIF / 自定义表情 / 搜索结果），带场景值 */
  onPickImage?: (item: Emoticon, sceneType: number) => void;
  align?: 'left' | 'right';
  /**
   * 面板锚点：`trigger`（默认）贴按钮浮起；`send-area` 贴发送区顶边，
   * 发送区自己的行（附件预览/回复目标）不会被面板盖住。调用方需把发送区容器设为 `relative`。
   */
  anchorTo?: 'trigger' | 'send-area';
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
      {/*
        自定义锚点：**必须排在 PopoverTrigger 之后**。
        Radix 的 PopoverTrigger 在 `hasCustomAnchor` 翻转前会被
        `Popper.Anchor` 包一层，按钮的 ref 会把 Popper 的 reference 抢走；
        翻转时 React 重建按钮，旧节点游离，而 Radix 跳过 `ref(null)`
        （源码里是 `if (node)`），reference 就永远停在这个游离节点上——
        量出 0 矩形，面板飞到视口左上角。React 按子节点顺序挂 ref，
        锚点排在 trigger 之后 → 锚点后挂 → reference 归锚点。
        锚点本体：铺满发送区的一张透明层（容器需 position:relative），
        面板按 side=top 贴在它**顶边**（= 发送区顶边）；铺满而非零高线，
        是为了让锚点高度跟着发送区走——floating-ui 的 autoUpdate 只盯
        锚点尺寸变化，零高锚点在附件预览出现时尺寸不变 → 面板不重定位，
        正好压住新长出来的那一行。
      */}
      {anchorTo === 'send-area' && (
        <PopoverAnchor className="pointer-events-none absolute inset-0" aria-hidden />
      )}
      <PopoverContent
        side="top"
        align={align === 'right' ? 'end' : 'start'}
        sideOffset={8}
        // 焦点纪律的另一半：开关都不夺焦（radix 默认把焦点挪进浮层）
        onOpenAutoFocus={(e) => e.preventDefault()}
        onCloseAutoFocus={(e) => e.preventDefault()}
        data-wheel-block
        /* 播放器深色面板皮肤（消费方都在豁免区），盖掉 Popover 默认底色 */
        className="w-auto rounded-xl border-white/10 bg-black/90 p-2 backdrop-blur-sm"
      >
        <EmoticonPanel
          allowImages={!!onPickImage}
          onPickEmoji={onPick}
          onPickImage={(item, scene) => onPickImage?.(item, scene)}
        />
      </PopoverContent>
    </Popover>
  );
}
