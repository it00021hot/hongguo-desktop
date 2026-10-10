/**
 * 剧评列表（详情页「剧评」tab 内容）。
 *
 * 三块结构对齐 hgplayer 剧评页（2026-10-10 抓包对齐）：
 * - 剧均评分块（extra.book_info.score + score_cnt；空 = 暂无评分）
 * - 发评框（评分星级 + 表情 + 文本，见 ReviewComposer）
 * - 评论列表：每条带评分星（expand.score 十分制 ÷2 显示）与评分后缀文案
 *   （"观看1小时后点评"），滚动到底自动翻页（游标 = 响应 cursor 原样回传）
 */
import { useEffect, useRef } from 'react';
import { Loader2, Star } from 'lucide-react';
import { useSeriesComments } from '@/service/queries';
import { t, tf } from '@/locales';
import { cn } from '@/lib/utils';
import { formatCountPrecise } from '@/utils/format';
import { ReviewComposer } from './review-composer';

/** 十分制评分 → 5 星展示（"7" → 3.5 星；半星用填充比例表达）。 */
function ScoreStars({ score }: { score: string }) {
  const value = Number(score) / 2;
  const filled = Math.floor(value);
  const half = value - filled >= 0.5;
  return (
    <span className="flex items-center gap-px" aria-label={tf('detail.rateStar', { n: value })}>
      {[1, 2, 3, 4, 5].map((s) => (
        <Star
          key={s}
          className={cn(
            'size-3',
            s <= filled
              ? 'fill-amber-400 text-amber-400'
              : s === filled + 1 && half
                ? 'fill-amber-400/50 text-amber-400'
                : 'text-muted-foreground/40',
          )}
        />
      ))}
    </span>
  );
}

export function ReviewList({ seriesId }: { seriesId: string }) {
  const { data, hasNextPage, isFetchingNextPage, fetchNextPage } = useSeriesComments(seriesId);
  const pages = data?.pages;
  const comments = pages?.flatMap((p) => p.items) ?? [];
  const first = pages?.[0];
  const reviewScore = first?.score ?? '';
  const scoreCnt = first?.scoreCnt ?? 0;
  const tagStats = first?.tagStats ?? [];

  // 滚动懒加载（browse-page 同款哨兵）：剧评 tab 在页面主滚动流里，
  // 哨兵进入视口即续拉下一页（游标 = 上一页响应的 cursor 原样回传）
  const sentinelRef = useRef<HTMLDivElement | null>(null);
  useEffect(() => {
    const el = sentinelRef.current;
    if (!el) return;
    const io = new IntersectionObserver(
      (entries) => {
        if (entries.some((x) => x.isIntersecting) && hasNextPage && !isFetchingNextPage) {
          void fetchNextPage();
        }
      },
      { rootMargin: '160px' },
    );
    io.observe(el);
    return () => io.disconnect();
  }, [hasNextPage, isFetchingNextPage, fetchNextPage]);

  return (
    <>
      <ReviewComposer seriesId={seriesId} />
      {/* 剧均评分块（hgplayer 同款：9.0 大字 + 星 + 人数 + 标签统计 pill；
          剧均本体 = credibility_score，评分人数 = credibility_score_count） */}
      <div className="mb-2 flex flex-wrap items-center gap-x-3 gap-y-2 rounded-lg border px-4 py-3">
        {reviewScore ? (
          <>
            <div className="flex items-baseline gap-2">
              <span className="text-3xl leading-none font-bold text-amber-500">
                {Number(reviewScore).toFixed(1)}
              </span>
              <ScoreStars score={reviewScore} />
            </div>
            <span className="text-muted-foreground text-sm">
              {tf('detail.ratingCount', { count: formatCountPrecise(scoreCnt) })}
            </span>
            {tagStats.map((tag) => (
              <span
                key={tag.tagName}
                className="bg-muted rounded-full px-3 py-1 text-xs whitespace-nowrap"
              >
                {tag.tagName} {tag.count}
              </span>
            ))}
          </>
        ) : (
          <>
            <span className="flex items-center gap-px">
              {[1, 2, 3, 4, 5].map((s) => (
                <Star key={s} className="text-muted-foreground/40 size-3" />
              ))}
            </span>
            <span className="text-muted-foreground text-sm">{t('detail.noRating')}</span>
          </>
        )}
      </div>
      {comments.length === 0 ? (
        <p className="text-muted-foreground py-10 text-center text-sm">
          {t('detail.commentsEmpty')}
        </p>
      ) : (
        <ul className="divide-border divide-y">
          {comments.map((c) => (
            <li key={c.commentId} className="flex gap-3 py-4">
              <div className="bg-muted grid size-9 shrink-0 place-items-center overflow-hidden rounded-full text-xs">
                {c.avatar ? (
                  <img src={c.avatar} alt="" className="size-full object-cover" />
                ) : (
                  (c.userName[0] ?? '?')
                )}
              </div>
              <div className="min-w-0 flex-1">
                <div className="flex flex-wrap items-baseline gap-2">
                  <span className="truncate text-sm font-medium">{c.userName}</span>
                  {c.score && <ScoreStars score={c.score} />}
                  <span className="text-muted-foreground shrink-0 text-xs">
                    {new Date(c.createTime * 1000).toLocaleDateString()}
                  </span>
                  {c.scoreSuffixText && (
                    <span className="text-muted-foreground/70 shrink-0 text-xs">
                      {c.scoreSuffixText}
                    </span>
                  )}
                </div>
                <p className="mt-1 text-sm leading-relaxed break-words whitespace-pre-wrap">
                  {c.text}
                </p>
                <div className="text-muted-foreground mt-1 flex gap-4 text-xs">
                  <span>♥ {c.diggCount}</span>
                  {c.replyCount > 0 && (
                    <span>{tf('detail.replyCount', { count: c.replyCount })}</span>
                  )}
                </div>
              </div>
            </li>
          ))}
        </ul>
      )}
      {/* 懒加载哨兵 + 拉取中指示 */}
      <div ref={sentinelRef} className="h-px" aria-hidden />
      {isFetchingNextPage && (
        <div className="text-muted-foreground flex items-center justify-center gap-2 py-4 text-xs">
          <Loader2 className="size-3.5 animate-spin" aria-hidden />
          {t('detail.loadMoreReviews')}
        </div>
      )}
    </>
  );
}
