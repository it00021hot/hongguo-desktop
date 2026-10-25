import { useEffect } from 'react';
import { Smile } from 'lucide-react';
import { Popover, PopoverAnchor, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import { t } from '@/locales';
import { cn } from '@/lib/utils';
import type { Emoticon } from '@/service/schema';
import { EmoticonPanel } from './emoticon-panel';

/**
 * 弹幕发送框 / 评论区共用的表情选择入口：笑脸按钮 + 面板。
 *
 * 焦点纪律（保住输入框光标的关键）：trigger 与面板里的表情格子
 * 都 mousedown preventDefault，且 Popover 的开/关自动聚焦一并禁掉——
 * 焦点全程留在输入框，插入永远落在真实光标处，选完接着打字无感；
 * 格子连选不关面板，点外部/Esc 由 Popover 收起。
 *
 * 滚轮纪律：面板打开期间在 document capture 层吞掉全部 wheel——
 * 「在选择器边上一滚就切集」不再可能；data-wheel-block 让播放器的
 * wheel 交互裁决跳过面板自身（网格滚动是浏览器默认行为）。
 *
 * 定位：默认贴按钮浮起（side=top + 碰撞规避）；发送区带附加行时
 * （评论区：附件预览 / 回复目标）改锚到**发送区顶边**——面板浮在评论
 * 列表上方，发送区自己那几行不会被面板盖住。详见 `anchorTo`。
 *
 * 面板内四档（系统表情 / 第三方 GIF / 自定义 / 搜索）见 EmoticonPanel；
 * 这里只报「选了哪个表情」——系统表情给文本码，表情图片给图片载荷。
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
   * 面板锚点：
   * - `trigger`（默认）：贴按钮浮起，适合发送区没有附加行的场景；
   * - `send-area`：贴在**发送区顶边**，发送区自己的行（附件预览、回复
   *   目标）不会被面板盖住。调用方需把发送区容器设为 `relative`
   *   （锚点元素靠它定位）。
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
