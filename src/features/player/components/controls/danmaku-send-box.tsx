/** 控制栏内的弹幕发送框（hgplayer 同款，常驻控制栏左段）。 */
import { useRef, useState } from 'react';
import { toast } from 'sonner';
import { useAccount, useSendDanmaku } from '@/service/queries';
import { t } from '@/locales';
import { EmojiPickerButton } from '../emoji-picker';
import { RichEmojiInput, type RichEmojiInputHandle } from '../rich-emoji-input';

/** 控制栏内的弹幕发送框（hgplayer 同款：常驻控制栏左段）。
 *
 * 输入 Enter / 点「发送」提交；offset 取控件自己持有的播放秒数（实时）。
 * 未登录点发送给指路提示；vid 未就绪时静默忽略。
 * 表情：hgplayer 同款 `[名字]` 代码——选择器插入代码，弹幕层渲染成图。 */
export function DanmakuSendBox({ vid, currentSec }: { vid: string; currentSec: number }) {
  const [text, setText] = useState('');
  const [emojiOpen, setEmojiOpen] = useState(false);
  const send = useSendDanmaku();
  const { data: account } = useAccount();
  const loggedIn = !!account;
  const richRef = useRef<RichEmojiInputHandle | null>(null);

  const pickEmoji = (name: string) => {
    richRef.current?.insertEmoji(name);
  };

  const submit = () => {
    const content = text.trim();
    if (!content || send.isPending) return;
    if (!loggedIn) {
      toast.info(t('player.interact.loginRequired'));
      return;
    }
    if (!vid.includes(':')) return;
    send.mutate(
      { vid, text: content, offsetMs: Math.round(currentSec * 1000) },
      {
        onSuccess: () => {
          toast.success(t('player.interact.danmakuSent'));
          setText('');
        },
        onError: (e) => toast.error(String(e)),
      },
    );
  };

  return (
    <div className="relative ml-2 flex h-8 w-44 min-w-0 shrink items-center gap-1 overflow-hidden rounded-full bg-white/15 pr-1 pl-3 backdrop-blur-sm sm:w-52">
      <RichEmojiInput
        ref={richRef}
        value={text}
        onChange={setText}
        onEnter={submit}
        placeholder={t('player.interact.danmakuPlaceholder')}
        maxLength={100}
        className="h-full min-w-0 flex-1 scrollbar-none overflow-x-auto text-xs leading-8 whitespace-pre text-white"
      />
      <EmojiPickerButton
        open={emojiOpen}
        onToggle={() => setEmojiOpen((o) => !o)}
        onPick={pickEmoji}
      />
      <button
        type="button"
        onClick={submit}
        disabled={!text.trim() || send.isPending}
        className="grid h-6 shrink-0 cursor-pointer place-items-center rounded-full bg-red-500 px-2.5 text-xs text-white transition-opacity disabled:opacity-40"
      >
        {t('player.interact.send')}
      </button>
    </div>
  );
}
