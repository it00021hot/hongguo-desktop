/** 剧评输入框：剧评 tab 顶部的提交入口。 */
import { useState } from 'react';
import { Loader2 } from 'lucide-react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { useSendSeriesReview } from '@/service/queries';
import { t } from '@/locales';

/** 剧评输入框（剧评 tab 顶部；回车/发布提交，成功后缓存失效重取置顶）。
 *  登录门槛走后端拒绝 + toast 指路（与互动按钮同一口径）。 */
export function ReviewComposer({ seriesId }: { seriesId: string }) {
  const send = useSendSeriesReview(seriesId);
  const [text, setText] = useState('');
  const submit = () => {
    const content = text.trim();
    if (!content || send.isPending) return;
    send.mutate(content, {
      onSuccess: () => {
        toast.success(t('detail.commentSent'));
        setText('');
      },
      onError: (e) => toast.error(String(e)),
    });
  };
  return (
    <div className="mb-2 flex items-center gap-2">
      <input
        value={text}
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === 'Enter' && !e.nativeEvent.isComposing) submit();
        }}
        placeholder={t('detail.commentPlaceholder')}
        maxLength={500}
        className="bg-muted/50 focus-visible:ring-ring h-9 min-w-0 flex-1 rounded-full border px-4 text-sm outline-none focus-visible:ring-2"
      />
      <Button
        size="sm"
        className="shrink-0 gap-1"
        disabled={!text.trim() || send.isPending}
        onClick={submit}
      >
        {send.isPending && <Loader2 className="size-3.5 animate-spin" aria-hidden />}
        {t('detail.commentSend')}
      </Button>
    </div>
  );
}
