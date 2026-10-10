/** 控制栏内的弹幕发送框（hgplayer 同款，常驻控制栏左段）。 */
import { useState } from 'react';
import { toast } from 'sonner';
import { useAccount, useSendDanmaku } from '@/service/queries';
import { t } from '@/locales';
import { EmojiSendBox } from '@/components/common/emoji/emoji-send-box';

/** 控制栏内的弹幕发送框（hgplayer 同款：常驻控制栏左段）。
 *
 * 输入 Enter / 点「发送」提交；offset 取控件自己持有的播放秒数（实时）。
 * 未登录点发送给指路提示；vid 未就绪时静默忽略。
 * 表情：hgplayer 同款 `[名字]` 代码——选择器插入代码，弹幕层渲染成图。
 * 输入/表情/发送接线统一走 EmojiSendBox（与评论/回复/剧评同源）。 */
export function DanmakuSendBox({ vid, currentSec }: { vid: string; currentSec: number }) {
  const [text, setText] = useState('');
  const send = useSendDanmaku();
  const { data: account } = useAccount();
  const loggedIn = !!account;

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
    <EmojiSendBox
      value={text}
      onChange={setText}
      onSubmit={submit}
      placeholder={t('player.interact.danmakuPlaceholder')}
      maxLength={100}
      pending={send.isPending}
      pickerAlign="left"
      className="relative ml-2 h-8 w-44 min-w-0 shrink gap-1 overflow-hidden rounded-full bg-white/15 pr-1 pl-3 backdrop-blur-sm sm:w-52"
      inputClassName="h-full min-w-0 flex-1 scrollbar-none overflow-x-auto text-xs leading-8 whitespace-pre text-white"
      sendClassName="h-6 cursor-pointer rounded-full bg-red-500 px-2.5 text-xs text-white shadow-none hover:bg-red-500 disabled:opacity-40"
    />
  );
}
