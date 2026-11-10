/** 窄屏竖屏发弹幕：点左下「弹」弹出底栏输入；底边跟原生 IME 高度走，不被键盘盖住。 */
import { useState, type RefObject } from 'react';
import { toast } from 'sonner';
import { useAccount, useSendDanmaku } from '@/service/queries';
import { t } from '@/locales';
import { EmojiSendBox } from '@/components/common/emoji/emoji-send-box';
import { useImeBottom } from '@/utils/ime-inset';

export function PhoneDanmakuSend({
  open,
  onOpenChange,
  videoRef,
  vid,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  videoRef: RefObject<HTMLVideoElement | null>;
  /** 「vid:seriesId」组合形态 */
  vid: string;
}) {
  const [text, setText] = useState('');
  // 关栏清草稿：渲染期跟随 open 复位（含父层直接 setOpen(false)），避免 effect 同步 setState
  const [textForOpen, setTextForOpen] = useState(open);
  if (open !== textForOpen) {
    setTextForOpen(open);
    if (!open) setText('');
  }
  const imeBottom = useImeBottom();
  const send = useSendDanmaku();
  const { data: account } = useAccount();
  const loggedIn = !!account;

  if (!open) return null;

  const submit = () => {
    const content = text.trim();
    if (!content || send.isPending) return;
    if (!loggedIn) {
      toast.info(t('player.interact.loginRequired'));
      return;
    }
    if (!vid.includes(':')) return;
    const offsetMs = Math.round((videoRef.current?.currentTime ?? 0) * 1000);
    send.mutate(
      { vid, text: content, offsetMs },
      {
        onSuccess: () => {
          toast.success(t('player.interact.danmakuSent'));
          setText('');
        },
      },
    );
  };

  return (
    <>
      <button
        type="button"
        aria-label={t('common.close')}
        className="fixed inset-0 z-40 cursor-default"
        onClick={(e) => {
          e.stopPropagation();
          onOpenChange(false);
        }}
      />
      {/* fixed + 原生 IME 抬升：不吃 absolute/100vh，华为 WebView 也能贴在键盘上方 */}
      <div
        className="fixed inset-x-0 z-50 px-3 pt-2 pb-3"
        style={{ bottom: imeBottom }}
        onClick={(e) => e.stopPropagation()}
        onPointerDown={(e) => e.stopPropagation()}
      >
        <EmojiSendBox
          value={text}
          onChange={setText}
          onSubmit={submit}
          onEscape={() => onOpenChange(false)}
          placeholder={t('player.interact.danmakuPlaceholder')}
          maxLength={100}
          pending={send.isPending}
          autoFocus
          pickerAlign="left"
          className="h-11 w-full gap-1.5 rounded-full bg-black/70 pr-1.5 pl-3 backdrop-blur-md"
          inputClassName="h-full min-w-0 flex-1 scrollbar-none overflow-x-auto text-sm leading-[2.75rem] whitespace-pre text-white"
          sendClassName="h-8 rounded-full px-3 text-xs"
        />
      </div>
    </>
  );
}
