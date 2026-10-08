import { Smile } from 'lucide-react';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { DANMAKU_EMOJI_LIST } from '@/lib/danmaku-emoji';
import { t } from '@/i18n';
import { cn } from '@/lib/utils';

/**
 * 弹幕发送框 / 评论区共用的表情选择入口：笑脸按钮 + 上拉网格。
 *
 * 用 Radix DropdownMenu（portal 到 body，回复行在滚动容器里也不会被裁）
 * 而不是手搓 popover：打开时 react-remove-scroll 会拦掉**面板外**的滚轮，
 * 「在选择器里滚一格就切一集」从机制上不可能发生；网格内的滚动不受影响。
 * data-wheel-block 是给舞台滚轮切集的第二道保险（同倍速/清晰度菜单）。
 *
 * onSelect preventDefault 保持面板常开——连选几个表情不用反复点开
 * （hgplayer 同款），Esc / 点外部关闭。插入逻辑归使用方，这里只报
 * 「选了哪个表情」。
 */
export function EmojiPickerButton({
  onPick,
  align = 'start',
  buttonClassName,
}: {
  /** name 不带方括号（如 `微笑`），使用方自己拼 `[name]` 代码 */
  onPick: (name: string) => void;
  align?: 'start' | 'end';
  buttonClassName?: string;
}) {
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <button
          type="button"
          title={t('player.danmakuEmoji')}
          aria-label={t('player.danmakuEmoji')}
          className={cn(
            'grid cursor-pointer place-items-center rounded-full px-1 text-white/60 transition-colors hover:text-white',
            buttonClassName,
          )}
        >
          <Smile className="size-4" aria-hidden />
        </button>
      </DropdownMenuTrigger>
      <DropdownMenuContent
        side="top"
        align={align}
        sideOffset={6}
        data-wheel-block
        className="w-72 rounded-xl border-white/10 bg-black/85 p-2 backdrop-blur-sm"
      >
        {/* hgplayer 同款网格：每格一张表情图，title 即 [名字] 代码 */}
        <div className="grid max-h-44 scrollbar-thin grid-cols-8 gap-0.5 overflow-y-auto">
          {DANMAKU_EMOJI_LIST.map((e) => (
            <DropdownMenuItem
              key={e.name}
              title={e.name}
              onSelect={(ev) => {
                ev.preventDefault();
                onPick(e.name.slice(1, -1));
              }}
              className="grid cursor-pointer place-items-center rounded-md p-1 focus:bg-white/15"
            >
              <img src={e.url} alt={e.name} draggable={false} className="size-6 object-contain" />
            </DropdownMenuItem>
          ))}
        </div>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
