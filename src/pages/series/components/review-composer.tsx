/** 剧评输入框：剧评 tab 顶部的提交入口（评分 + 表情 + 文本，对齐 hgplayer）。 */
import { useState } from 'react';
import { Star } from 'lucide-react';
import { toast } from 'sonner';
import { useSendSeriesReview } from '@/service/queries';
import { t, tf } from '@/locales';
import { cn } from '@/lib/utils';
import { EmojiSendBox } from '@/components/common/emoji/emoji-send-box';

/**
 * 剧评输入框（剧评 tab 顶部；Enter/发布提交，成功后缓存失效重取置顶）。
 *
 * 评分随发送走（hgplayer 1.1.6 抓包：business_param.score 十分制 = 星级
 * ×2），「我的评分」五星级未选时禁发——score 是剧评身份的一部分，不选
 * 等于没评。表情/输入/发送接线统一走 EmojiSendBox（与弹幕/评论同源）。
 * 登录门槛走后端拒绝 + toast 指路（与互动按钮同一口径）。
 */
export function ReviewComposer({ seriesId }: { seriesId: string }) {
  const send = useSendSeriesReview(seriesId);
  const [text, setText] = useState('');
  /** 我的评分：5 星制（1–5），发送时换算十分制 ×2；0 = 未选 */
  const [stars, setStars] = useState(0);
  const [hoverStars, setHoverStars] = useState(0);

  const submit = () => {
    const content = text.trim();
    if (!content || stars < 1 || send.isPending) return;
    send.mutate(
      { text: content, score: stars * 2 },
      {
        onSuccess: () => {
          toast.success(tf('detail.reviewSent', { score: stars * 2 }));
          setText('');
        },
        onError: (e) => toast.error(String(e)),
      },
    );
  };

  const shown = hoverStars || stars;
  return (
    <div className="bg-card/60 mb-2 rounded-lg border p-3">
      {/* 我的评分（hgplayer 同款：星级 + 「我的评分」标签） */}
      <div className="mb-2 flex items-center gap-2">
        <span className="text-sm font-medium">{t('detail.myRating')}</span>
        <div
          className="flex items-center gap-0.5"
          onMouseLeave={() => setHoverStars(0)}
          role="radiogroup"
          aria-label={t('detail.myRating')}
        >
          {[1, 2, 3, 4, 5].map((s) => (
            <button
              key={s}
              type="button"
              aria-label={tf('detail.rateStar', { n: s })}
              className="cursor-pointer p-0.5"
              onMouseEnter={() => setHoverStars(s)}
              onClick={() => setStars(s)}
            >
              <Star
                className={cn(
                  'size-5 transition-colors',
                  s <= shown ? 'fill-amber-400 text-amber-400' : 'text-muted-foreground/40',
                )}
              />
            </button>
          ))}
        </div>
        {stars > 0 && (
          <span className="text-muted-foreground text-xs">{t('detail.ratedSuffix')}</span>
        )}
      </div>
      <EmojiSendBox
        value={text}
        onChange={setText}
        onSubmit={submit}
        placeholder={t('detail.commentPlaceholder')}
        sendLabel={t('detail.reviewSend')}
        maxLength={500}
        pending={send.isPending}
        canSend={!!text.trim() && stars >= 1}
        pickerAlign="right"
        className="gap-3"
        inputClassName="bg-muted/50 focus-visible:ring-ring h-16 min-w-0 flex-1 resize-none overflow-y-auto whitespace-pre-wrap rounded-md border px-3 py-2 text-sm leading-6"
        sendClassName="h-9 gap-1 bg-red-500 px-4 hover:bg-red-500/90"
      />
    </div>
  );
}
