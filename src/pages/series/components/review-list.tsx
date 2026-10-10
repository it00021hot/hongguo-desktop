/**
 * 剧评列表（详情页「剧评」tab 内容）。
 *
 * 三块结构对齐 hgplayer 剧评页（2026-10-10 抓包对齐）：
 * - 剧均评分块（credibility_score + score_cnt；空 = 暂无评分）
 * - 发评框（评分星级 + 表情 + 文本，见 ReviewComposer）
 * - 评论列表：每条带评分星（expand.score 十分制 ÷2 显示）、评分后缀文案、
 *   ♡ 点赞（do_action object_type=2 剧评形态）与「回复 / 展开 N 条回复」
 *   （reply/add 剧评形态 commit_source=13 + reply/list src=501），滚动到底
 *   自动翻页（游标 = 响应 cursor 原样回传）
 */
import { useEffect, useRef, useState } from 'react';
import { ChevronDown, Heart, Loader2, Star } from 'lucide-react';
import { toast } from 'sonner';
import { useQueryClient } from '@tanstack/react-query';
import type { InfiniteData } from '@tanstack/react-query';
import {
  useAccount,
  useReviewDigg,
  useReviewReplies,
  useSendReviewReply,
  useSeriesComments,
} from '@/service/queries';
import { keys } from '@/service/queries/common';
import { interact } from '@/service/commands';
import { t, tf } from '@/locales';
import { cn } from '@/lib/utils';
import { formatCountPrecise } from '@/utils/format';
import { ConfirmDialog } from '@/components/ui/confirm-dialog';
import { EmojiText } from '@/components/common/emoji/emoji-text';
import { EmojiSendBox } from '@/components/common/emoji/emoji-send-box';
import type { CommentPage, ReplyItem, ReplyPage } from '@/service/schema';
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

/** 一条剧评回复行（回复点赞与评论点赞同形态，object_id 传 reply_id）。
 *  自己的回复（userId 对上登录 uid）带删除入口。 */
function ReviewReplyRow({
  reply,
  liked,
  myUserId,
  onDigg,
  onReplyTo,
  onDelete,
}: {
  reply: ReplyItem;
  liked: Set<string>;
  myUserId: string;
  onDigg: (id: string, digg: boolean) => void;
  onReplyTo: (reply: ReplyItem) => void;
  onDelete: (reply: ReplyItem) => void;
}) {
  const isLiked = liked.has(reply.replyId) || reply.userDigg;
  const isMine = !!myUserId && reply.userId === myUserId;
  return (
    <div className="flex gap-2">
      <div className="bg-muted grid size-6 shrink-0 place-items-center overflow-hidden rounded-full text-[10px]">
        {reply.avatar ? (
          <img src={reply.avatar} alt="" loading="lazy" className="size-full object-cover" />
        ) : (
          (reply.userName[0] ?? '?')
        )}
      </div>
      <div className="min-w-0 flex-1">
        <p className="text-muted-foreground text-[11px]">
          {reply.userName || t('player.comments.anon')}
          {reply.replyToName && (
            <span className="ml-1 opacity-70">
              {t('detail.replyToPrefix')} @{reply.replyToName}
            </span>
          )}
        </p>
        <p className="mt-0.5 text-xs leading-snug break-words whitespace-pre-wrap">
          <EmojiText text={reply.text} />
        </p>
        <div className="text-muted-foreground mt-0.5 flex items-center gap-3 text-[10px]">
          <span>{new Date(reply.createTime * 1000).toLocaleDateString()}</span>
          <button
            type="button"
            onClick={() => onReplyTo(reply)}
            className="hover:text-foreground cursor-pointer"
          >
            {t('detail.reply')}
          </button>
          {isMine && (
            <button
              type="button"
              onClick={() => onDelete(reply)}
              className="cursor-pointer hover:text-red-500"
            >
              {t('common.delete')}
            </button>
          )}
        </div>
      </div>
      <button
        type="button"
        onClick={() => onDigg(reply.replyId, !isLiked)}
        className={cn(
          'text-muted-foreground hover:text-foreground flex shrink-0 cursor-pointer flex-col items-center gap-0.5 self-start',
          isLiked && 'text-red-500',
        )}
        title={t('detail.like')}
      >
        <Heart className={cn('size-3', isLiked && 'fill-red-500 text-red-500')} />
        {reply.diggCount > 0 && <span className="text-[9px] tabular-nums">{reply.diggCount}</span>}
      </button>
    </div>
  );
}

/** 「展开 N 条回复」区（剧评维度 src=501/ch=34，展开才拉首页）。 */
function ReviewReplySection({
  seriesId,
  commentId,
  replyCount,
  liked,
  myUserId,
  onDigg,
  onReplyTo,
  onDeleteReply,
}: {
  seriesId: string;
  commentId: string;
  replyCount: number;
  liked: Set<string>;
  myUserId: string;
  /** 自己发的回复（本地先上屏；id 供服务端列表迟到后去重） */
  localReplies: { id?: string; text: string; replyTo?: string }[];
  open: boolean;
  onToggle: () => void;
  onDigg: (id: string, digg: boolean) => void;
  onReplyTo: (reply: ReplyItem) => void;
  onDeleteReply: (reply: ReplyItem) => void;
}) {
  const { data, isPending, error, refetch, isFetchingNextPage, hasNextPage, fetchNextPage } =
    useReviewReplies(seriesId, commentId, open);
  const replies = data?.pages.flatMap((p) => p.items) ?? [];
  // 服务端列表迟到后按本地已追加的 reply_id 去重
  const localIds = new Set(localReplies.map((r) => r.id).filter(Boolean));
  const serverReplies = replies.filter((r) => !localIds.has(r.replyId));
  if (replyCount <= 0) return null;
  return (
    <div className="mt-1.5">
      <button
        type="button"
        onClick={onToggle}
        className="text-muted-foreground hover:text-foreground flex cursor-pointer items-center gap-1 text-xs"
      >
        {open ? (
          <>
            {t('detail.collapseReplies')}
            <ChevronDown className="size-3 rotate-180" />
          </>
        ) : (
          <>
            {tf('detail.expandReplies', { n: replyCount })}
            <ChevronDown className="size-3" />
          </>
        )}
      </button>
      {open && (
        <div className="border-border mt-2 flex flex-col gap-2.5 border-l-2 pl-3">
          {isPending && (
            <div className="text-muted-foreground grid place-items-center py-2">
              <Loader2 className="size-3.5 animate-spin" />
            </div>
          )}
          {error && (
            <button
              type="button"
              onClick={() => void refetch()}
              className="text-muted-foreground hover:text-foreground cursor-pointer text-left text-xs"
            >
              {t('detail.repliesLoadFailed')}
            </button>
          )}
          {serverReplies.map((r) => (
            <ReviewReplyRow
              key={r.replyId}
              reply={r}
              liked={liked}
              myUserId={myUserId}
              onDigg={onDigg}
              onReplyTo={onReplyTo}
              onDelete={onDeleteReply}
            />
          ))}
          {/* 自己发的回复（本地追加，服务端列表有延迟） */}
          {localReplies.map((r, i) => (
            <div key={`local-${i}`} className="text-xs leading-snug">
              <span className="text-muted-foreground mr-1 inline-flex items-center">
                {t('player.comments.me')}
              </span>
              {r.replyTo && (
                <span className="text-muted-foreground mr-1">
                  {t('detail.replyToPrefix')}@{r.replyTo.slice(0, 12)}
                </span>
              )}
              <span className="break-words whitespace-pre-wrap">
                <EmojiText text={r.text} />
              </span>
            </div>
          ))}
          {hasNextPage && (
            <button
              type="button"
              disabled={isFetchingNextPage}
              onClick={() => void fetchNextPage()}
              className="text-muted-foreground hover:text-foreground flex w-fit cursor-pointer items-center gap-1 text-xs disabled:opacity-60"
            >
              {isFetchingNextPage && <Loader2 className="size-3 animate-spin" aria-hidden />}
              {t('detail.moreReplies')}
            </button>
          )}
        </div>
      )}
    </div>
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

  // 点赞：useReviewDigg 乐观更新缓存；liked 集合是本地已点覆盖（防回包漂移）
  const digg = useReviewDigg(seriesId);
  const [liked, setLiked] = useState<Set<string>>(new Set());
  const onDigg = (id: string, next: boolean) => {
    setLiked((prev) => {
      const s = new Set(prev);
      if (next) s.add(id);
      else s.delete(id);
      return s;
    });
    digg.mutate({ reviewId: id, digg: next }, { onError: (e) => toast.error(String(e)) });
  };

  // 回复：剧评形态 reply/add（commit_source=13）；回复「回复」带 replyToReplyId
  const sendReply = useSendReviewReply(seriesId);
  const [replyTarget, setReplyTarget] = useState<string | null>(null);
  const [replyToReply, setReplyToReply] = useState<{ id: string; name: string } | null>(null);
  const [replyText, setReplyText] = useState('');
  /** 自己发的回复（剧评 id → 本地先上屏；服务端列表有索引延迟） */
  const [localReplies, setLocalReplies] = useState<
    Record<string, { id?: string; text: string; replyTo?: string }[]>
  >({});
  /** 展开了回复区的剧评（受控：发送成功强制展开，对齐 hgplayer） */
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
        onSuccess: (replyId) => {
          setLocalReplies((prev) => ({
            ...prev,
            [reviewId]: [
              ...(prev[reviewId] ?? []),
              { id: replyId, text: content, replyTo: replyToReply?.name },
            ],
          }));
          setReplyText('');
          setReplyTarget(null);
          setReplyToReply(null);
          // hgplayer 同款：发送成功强制展开回复区，新回复立刻可见
          setReplySectionOpen(reviewId, true);
        },
        onError: (e) => toast.error(String(e)),
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
          queryClient.setQueryData<InfiniteData<ReplyPage, string>>(
            keys.reviewReplies(seriesId, target.parentId),
            (prev) =>
              prev && {
                ...prev,
                pages: prev.pages.map((p) => ({
                  ...p,
                  items: p.items.filter((r) => r.replyId !== target.id),
                })),
              },
          );
        } else {
          queryClient.setQueryData<InfiniteData<CommentPage, string>>(
            keys.seriesComments(seriesId),
            (prev) =>
              prev && {
                ...prev,
                pages: prev.pages.map((p) => ({
                  ...p,
                  items: p.items.filter((c) => c.commentId !== target.id),
                })),
              },
          );
          if (replyTarget === target.id) setReplyTarget(null);
        }
        toast.success(t('common.deleted'));
      })
      .catch((e) => toast.error(String(e)));
  };

  return (
    <>
      <ReviewComposer seriesId={seriesId} />
      {/* 剧均评分块（hgplayer 同款：9.0 大字 + 星 + 人数 + 标签统计 pill；
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
            const isLiked = liked.has(c.commentId) || c.userDigg;
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
                      {new Date(c.createTime * 1000).toLocaleDateString()}
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
                        isLiked && 'text-red-500',
                      )}
                      title={t('detail.like')}
                    >
                      <Heart className={cn('size-3.5', isLiked && 'fill-red-500 text-red-500')} />
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
                        className="cursor-pointer hover:text-red-500"
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
                    liked={liked}
                    myUserId={myUserId}
                    localReplies={localReplies[c.commentId] ?? []}
                    open={openReplies.has(c.commentId)}
                    onToggle={() => setReplySectionOpen(c.commentId, !openReplies.has(c.commentId))}
                    onDigg={onDigg}
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
