/**
 * 剧评列表（详情页「剧评」tab 内容）。
 *
 * 三块结构（2026-10-10 抓包对齐）：
 * - 剧均评分块（credibility_score + score_cnt；空 = 暂无评分）
 * - 发评框（评分星级 + 表情 + 文本，见 ReviewComposer）
 * - 评论列表：每条带评分星（expand.score 十分制 ÷2 显示）、评分后缀文案、
 *   ♡ 点赞（do_action object_type=2 剧评形态）与「回复 / 展开 N 条回复」
 *   （reply/add 剧评形态 commit_source=13 + reply/list src=501），滚动到底
 *   自动翻页（游标 = 响应 cursor 原样回传）
 */
import { useEffect, useMemo, useRef, useState } from 'react';
import { Heart, Loader2, Star } from 'lucide-react';
import { toast } from 'sonner';
import { useQueryClient } from '@tanstack/react-query';
import {
  useAccount,
  useReviewDigg,
  useReviewReplies,
  useReviewReplyDigg,
  useSendReviewReply,
  useSeriesComments,
} from '@/service/queries';
import {
  appendReply,
  bumpReplyCount,
  removeComment,
  removeReply,
} from '@/service/queries/comment-cache';
import { dedupBy, keys } from '@/service/queries/common';
import { interact } from '@/service/commands';
import { t, tf } from '@/locales';
import { cn } from '@/lib/utils';
import { formatCountPrecise } from '@/utils/format';
import { SENTINEL_ROOT_MARGIN_PX } from '@/utils/infinite-scroll';
import { ConfirmDialog } from '@/components/ui/confirm-dialog';
import { EmojiText } from '@/components/common/emoji/emoji-text';
import { ReplyThread } from '@/components/common/reply-thread';
import { EmojiSendBox } from '@/components/common/emoji/emoji-send-box';
import type { ReplyItem } from '@/service/schema';
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
              ? 'fill-star text-star'
              : s === filled + 1 && half
                ? 'fill-star/50 text-star'
                : 'text-muted-foreground/40',
          )}
        />
      ))}
    </span>
  );
}

/** 剧评皮肤的回复线程：解包剧评回复查询，交给共享 ReplyThread
 *  （皮肤与 i18n 键位差异收敛在 `components/common/reply-thread`；player
 *  侧评论同源）。回复点赞**不是**剧评形态——与评论点赞同端点，object_id
 *  传 reply_id，2026-10-10 CDP 实测即时生效。 */
function ReviewReplySection({
  seriesId,
  commentId,
  replyCount,
  myUserId,
  open,
  onToggle,
  onDigg,
  onReplyTo,
  onDeleteReply,
}: {
  seriesId: string;
  commentId: string;
  replyCount: number;
  myUserId: string;
  open: boolean;
  onToggle: () => void;
  onDigg: (id: string, digg: boolean) => void;
  onReplyTo: (reply: ReplyItem) => void;
  onDeleteReply: (reply: ReplyItem) => void;
}) {
  const q = useReviewReplies(seriesId, commentId, open);
  const replies = dedupBy(q.data?.pages.flatMap((p) => p.items) ?? [], (r) => r.replyId);
  return (
    <ReplyThread
      variant="page"
      replyCount={replyCount}
      myUserId={myUserId}
      open={open}
      onToggle={onToggle}
      replies={replies}
      isPending={q.isPending}
      error={q.error}
      refetch={() => void q.refetch()}
      hasNextPage={q.hasNextPage}
      isFetchingNextPage={q.isFetchingNextPage}
      fetchNextPage={() => void q.fetchNextPage()}
      onDigg={onDigg}
      onReplyTo={onReplyTo}
      onDeleteReply={onDeleteReply}
    />
  );
}

export function ReviewList({ seriesId }: { seriesId: string }) {
  const { data, hasNextPage, isFetchingNextPage, fetchNextPage } = useSeriesComments(seriesId);
  const pages = data?.pages;
  // 按 commentId 去重：服务端相邻分页重叠条目，不去重 React 报 duplicate
  // key 且同一条重复渲染（2026-10-10 实测 228 条里 38 组重复）
  const comments = useMemo(
    () => dedupBy(pages?.flatMap((p) => p.items) ?? [], (c) => c.commentId),
    [pages],
  );
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
      { rootMargin: `${SENTINEL_ROOT_MARGIN_PX}px` },
    );
    io.observe(el);
    return () => io.disconnect();
  }, [hasNextPage, isFetchingNextPage, fetchNextPage]);

  // 点赞：useReviewDigg 乐观更新剧评缓存（显示直读 c.userDigg，无需旁路态）；
  // 回复点赞走 useReviewReplyDigg（评论形态端点，不是剧评形态）
  const digg = useReviewDigg(seriesId);
  const reviewReplyDigg = useReviewReplyDigg(seriesId);
  const onDigg = (id: string, next: boolean) => {
    digg.mutate({ reviewId: id, digg: next });
  };
  const onReplyDigg = (parentId: string, id: string, next: boolean) => {
    reviewReplyDigg.mutate({ commentId: parentId, replyId: id, digg: next });
  };

  // 回复：剧评形态 reply/add（commit_source=13）；回复「回复」带 replyToReplyId
  const sendReply = useSendReviewReply(seriesId);
  const [replyTarget, setReplyTarget] = useState<string | null>(null);
  const [replyToReply, setReplyToReply] = useState<{ id: string; name: string } | null>(null);
  const [replyText, setReplyText] = useState('');
  /** 展开了回复区的剧评（受控：发送成功强制展开） */
  const [openReplies, setOpenReplies] = useState<Set<string>>(new Set());
  const setReplySectionOpen = (reviewId: string, open: boolean) =>
    setOpenReplies((prev) => {
      const next = new Set(prev);
      if (open) next.add(reviewId);
      else next.delete(reviewId);
      return next;
    });
  const submitReply = (reviewId: string) => {
    const content = replyText.trim();
    if (!content || sendReply.isPending) return;
    sendReply.mutate(
      { replyToCommentId: reviewId, replyToReplyId: replyToReply?.id, text: content },
      {
        onSuccess: (reply) => {
          // 服务端回显的完整回复对象直接进缓存（头像/昵称/uid/时间齐全），
          // 父剧评 replyCount +1
          appendReply(queryClient, keys.reviewReplies(seriesId, reviewId), reply);
          bumpReplyCount(queryClient, keys.seriesComments(seriesId), reviewId, 1);
          setReplyText('');
          setReplyTarget(null);
          setReplyToReply(null);
          // 发送成功强制展开回复区，新回复立刻可见
          setReplySectionOpen(reviewId, true);
        },
      },
    );
  };

  // 删除自己的剧评（service_id=2）/ 回复（service_id=4）；本地摘除免重取
  const { data: account } = useAccount();
  const myUserId = account?.userId ?? '';
  const queryClient = useQueryClient();
  /** 待确认删除目标：serviceId 2 = 剧评本体，4 = 回复（parentId 在场） */
  const [deleteTarget, setDeleteTarget] = useState<{
    id: string;
    serviceId: 2 | 4;
    parentId?: string;
  } | null>(null);
  const confirmDelete = () => {
    const target = deleteTarget;
    setDeleteTarget(null);
    if (!target) return;
    interact
      .deleteComment(target.id, target.serviceId)
      .then(() => {
        if (target.serviceId === 4 && target.parentId) {
          // 回复：回复缓存摘除 + total-1，父剧评 replyCount-1——与发送
          // 成功的 +1 对称
          removeReply(queryClient, keys.reviewReplies(seriesId, target.parentId), target.id);
          bumpReplyCount(queryClient, keys.seriesComments(seriesId), target.parentId, -1);
        } else {
          removeComment(queryClient, keys.seriesComments(seriesId), target.id);
          if (replyTarget === target.id) setReplyTarget(null);
        }
        toast.success(t('common.deleted'));
      })
      .catch((e) => toast.error(String(e)));
  };

  return (
    <>
      <ReviewComposer seriesId={seriesId} />
      {/* 剧均评分块（9.0 大字 + 星 + 人数 + 标签统计 pill；
          剧均本体 = credibility_score，评分人数 = credibility_score_count） */}
      <div className="border-border mb-2 flex flex-wrap items-center gap-x-3 gap-y-2 rounded-lg border px-4 py-3">
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
          {comments.map((c) => {
            const isLiked = c.userDigg;
            const isMine = !!myUserId && c.userId === myUserId;
            return (
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
                      {new Date(c.createTime).toLocaleDateString()}
                    </span>
                    {c.scoreSuffixText && (
                      <span className="text-muted-foreground/70 shrink-0 text-xs">
                        {c.scoreSuffixText}
                      </span>
                    )}
                  </div>
                  <p className="mt-1 text-sm leading-relaxed break-words whitespace-pre-wrap">
                    <EmojiText text={c.text} />
                  </p>
                  <div className="text-muted-foreground mt-1.5 flex items-center gap-4 text-xs">
                    <button
                      type="button"
                      onClick={() => onDigg(c.commentId, !isLiked)}
                      className={cn(
                        'hover:text-foreground flex cursor-pointer items-center gap-1',
                        isLiked && 'text-hot',
                      )}
                      title={t('detail.like')}
                    >
                      <Heart className={cn('size-3.5', isLiked && 'fill-hot text-hot')} />
                      {c.diggCount > 0 && <span className="tabular-nums">{c.diggCount}</span>}
                    </button>
                    <button
                      type="button"
                      onClick={() => {
                        setReplyTarget(replyTarget === c.commentId ? null : c.commentId);
                        setReplyToReply(null);
                        setReplyText('');
                      }}
                      className="hover:text-foreground cursor-pointer"
                    >
                      {t('detail.reply')}
                    </button>
                    {isMine && (
                      <button
                        type="button"
                        onClick={() => setDeleteTarget({ id: c.commentId, serviceId: 2 })}
                        className="hover:text-destructive cursor-pointer"
                      >
                        {t('common.delete')}
                      </button>
                    )}
                  </div>
                  {/* 回复列表（剧评维度）：展开按需拉取；自己发的回复本地追加 */}
                  <ReviewReplySection
                    seriesId={seriesId}
                    commentId={c.commentId}
                    replyCount={c.replyCount}
                    myUserId={myUserId}
                    open={openReplies.has(c.commentId)}
                    onToggle={() => setReplySectionOpen(c.commentId, !openReplies.has(c.commentId))}
                    onDigg={(id, d) => onReplyDigg(c.commentId, id, d)}
                    onReplyTo={(r) => {
                      setReplyTarget(c.commentId);
                      setReplyToReply({ id: r.replyId, name: r.userName });
                      setReplyText('');
                    }}
                    onDeleteReply={(r) =>
                      setDeleteTarget({ id: r.replyId, serviceId: 4, parentId: c.commentId })
                    }
                  />
                  {/* 回复输入框 */}
                  {replyTarget === c.commentId && (
                    <EmojiSendBox
                      autoFocus
                      value={replyText}
                      onChange={setReplyText}
                      onSubmit={() => submitReply(c.commentId)}
                      onEscape={() => setReplyTarget(null)}
                      placeholder={
                        replyToReply
                          ? tf('detail.replyPlaceholder', { name: replyToReply.name })
                          : t('detail.replyToComment')
                      }
                      maxLength={200}
                      pending={sendReply.isPending}
                      pickerAlign="left"
                      className="mt-2 gap-1.5"
                      inputClassName="bg-muted h-8 min-w-0 flex-1 rounded-md px-3 text-xs leading-8 whitespace-pre"
                      sendClassName="h-8 px-3 text-xs"
                    />
                  )}
                </div>
              </li>
            );
          })}
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
      {/* 删除确认（comment/del：剧评 service_id=2 / 回复 4） */}
      <ConfirmDialog
        open={!!deleteTarget}
        onOpenChange={(open) => !open && setDeleteTarget(null)}
        title={t('common.deleteConfirmTitle')}
        description={t('common.deleteConfirmDesc')}
        confirmLabel={t('common.delete')}
        onConfirm={confirmDelete}
      />
    </>
  );
}
