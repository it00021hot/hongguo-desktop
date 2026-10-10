//! 沉浸流评论面板：右侧滑出（hgplayer 同款「评论 · N」列）。
//!
//! 列表（头像/昵称/文本/时间/♡）+ 底部发评论框；评论点赞走
//! commentapi/comment/do_action（8/9）。数据与弹幕同端点不同形态——
//! 2026-10-06 抓包重锁：评论形态 business_param 是 need_count/req_type +
//! server_channel=18（沿用弹幕形会被 103001 拒），need_count 顺带带回
//! 评论总数（头部计数用）。
//!
//! 回复：发送走 reply/add 独立端点；回复**列表**自 2026-10-10 抓
//! hgplayer 1.1.8 起可用（reply/list 独立端点，评论维度 src=504/ch=18，
//! 此前「服务端无接口」的结论作废）——「展开 N 条回复」按需拉取，
//! 自己发的回复仍本地追加一份（服务端列表有延迟）。

import { useEffect, useState } from 'react';
import { ChevronDown, CornerDownRight, Heart, Loader2, X } from 'lucide-react';
import { toast } from 'sonner';
import { useQueryClient } from '@tanstack/react-query';
import type { InfiniteData } from '@tanstack/react-query';
import { Button } from '@/components/ui/button';
import { ConfirmDialog } from '@/components/ui/confirm-dialog';
import { t, tf } from '@/locales';
import { cn } from '@/lib/utils';
import { interact } from '@/service/commands';
import {
  useAccount,
  useCommentReplies,
  useComments,
  useSendComment,
  useSendReply,
} from '@/service/queries';
import { keys } from '@/service/queries/common';
import { EmojiText } from '@/components/common/emoji/emoji-text';
import { EmojiSendBox } from '@/components/common/emoji/emoji-send-box';
import type { CommentItem, CommentPage, ReplyItem, ReplyPage } from '@/service/schema';

interface Props {
  seriesId: string;
  vid: string;
  onClose: () => void;
}

/** 相对时间（评论区样式：xx 分钟前 / 昨天 / 日期）。 */
function relativeTime(unixSec: number): string {
  const diff = Date.now() / 1000 - unixSec;
  if (unixSec <= 0) return '';
  if (diff < 60) return t('player.comments.justNow');
  if (diff < 3600) return tf('player.comments.minutesAgo', { n: Math.floor(diff / 60) });
  if (diff < 86400) return tf('player.comments.hoursAgo', { n: Math.floor(diff / 3600) });
  if (diff < 172800) return t('player.comments.yesterday');
  const d = new Date(unixSec * 1000);
  return `${d.getMonth() + 1}/${d.getDate()}`;
}

/** 本地追加的一条回复（自己发的；服务端列表有延迟，先上屏） */
interface LocalReply {
  text: string;
  /** 回复「回复」时对方内容摘要（「回复 @xxx」展示用） */
  replyTo?: string;
}

/** 一条回复行：头像 / 昵称 / 回复@谁 / 表情文本 / 时间 / ♡（回复点赞与
 *  评论点赞同形态，object_id 传 reply_id——2026-10-10 抓包实锤）。
 *  自己的回复（userId 对上登录 uid）带删除入口。 */
function ReplyRow({
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
      {reply.avatar ? (
        <img src={reply.avatar} alt="" loading="lazy" className="size-6 shrink-0 rounded-full" />
      ) : (
        <div className="grid size-6 shrink-0 place-items-center rounded-full bg-neutral-700 text-[10px]">
          {(reply.userName || '友').slice(0, 1)}
        </div>
      )}
      <div className="min-w-0 flex-1">
        <p className="text-[11px] text-neutral-400">
          {reply.userName || t('player.comments.anon')}
          {reply.replyToName && (
            <span className="ml-1 text-neutral-500">
              {t('player.comments.replyToPrefix')} @{reply.replyToName}
            </span>
          )}
        </p>
        <p className="mt-0.5 text-xs leading-snug break-words whitespace-pre-wrap">
          <EmojiText text={reply.text} />
        </p>
        <div className="mt-0.5 flex items-center gap-3 text-[10px] text-neutral-500">
          <span>{relativeTime(reply.createTime)}</span>
          <button
            type="button"
            onClick={() => onReplyTo(reply)}
            className="cursor-pointer hover:text-neutral-300"
          >
            {t('player.comments.reply')}
          </button>
          {isMine && (
            <button
              type="button"
              onClick={() => onDelete(reply)}
              className="cursor-pointer hover:text-red-400"
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
          'flex shrink-0 cursor-pointer flex-col items-center gap-0.5 self-start text-neutral-400 hover:text-white',
          isLiked && 'text-red-400',
        )}
        title={t('player.interact.like')}
      >
        <Heart className={cn('size-3', isLiked && 'fill-red-400 text-red-400')} />
        {reply.diggCount > 0 && <span className="text-[9px] tabular-nums">{reply.diggCount}</span>}
      </button>
    </div>
  );
}

/** 「展开 N 条回复」区：展开才拉首页，一页 10 条，hasMore 续拉。 */
function ReplySection({
  vid,
  commentId,
  replyCount,
  localReplies,
  liked,
  myUserId,
  onDigg,
  onReplyTo,
  onDeleteReply,
}: {
  vid: string;
  commentId: string;
  replyCount: number;
  localReplies: LocalReply[];
  liked: Set<string>;
  myUserId: string;
  onDigg: (id: string, digg: boolean) => void;
  onReplyTo: (reply: ReplyItem) => void;
  onDeleteReply: (reply: ReplyItem) => void;
}) {
  const [open, setOpen] = useState(false);
  const { data, isPending, isFetchingNextPage, hasNextPage, fetchNextPage } = useCommentReplies(
    vid,
    commentId,
    open,
  );
  const replies = data?.pages.flatMap((p) => p.items) ?? [];
  return (
    <div className="mt-1">
      {replyCount > 0 && (
        <button
          type="button"
          onClick={() => setOpen((o) => !o)}
          className="flex cursor-pointer items-center gap-1 text-[11px] text-neutral-400 hover:text-neutral-200"
        >
          {open ? (
            <>
              {t('player.comments.collapseReplies')}
              <ChevronDown className="size-3 rotate-180" />
            </>
          ) : (
            <>
              {tf('player.comments.expandReplies', { n: replyCount })}
              <ChevronDown className="size-3" />
            </>
          )}
        </button>
      )}
      {open && (
        <div className="mt-1.5 flex flex-col gap-2 border-l-2 border-neutral-800 pl-2.5">
          {isPending && (
            <div className="grid place-items-center py-2 text-neutral-500">
              <Loader2 className="size-3.5 animate-spin" />
            </div>
          )}
          {replies.map((r) => (
            <ReplyRow
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
              <span className="mr-1 inline-flex items-center text-neutral-500">
                <CornerDownRight className="mr-0.5 inline size-3" />
                {t('player.comments.me')}
              </span>
              {r.replyTo && (
                <span className="mr-1 text-neutral-500">
                  {t('player.comments.replyToPrefix')}@{r.replyTo.slice(0, 12)}
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
              className="flex w-fit cursor-pointer items-center gap-1 text-[11px] text-neutral-400 hover:text-neutral-200 disabled:opacity-60"
            >
              {isFetchingNextPage && <Loader2 className="size-3 animate-spin" aria-hidden />}
              {t('player.comments.moreReplies')}
            </button>
          )}
        </div>
      )}
    </div>
  );
}

export function CommentPanel({ vid, onClose }: Props) {
  // Esc 关闭（面板盖住互动栏按钮时这是最直接的退出路径）
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      // 输入法组合中的 Esc 是取消拼音，不是关面板
      if (e.isComposing || e.keyCode === 229) return;
      if (e.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onClose]);
  const { data: account } = useAccount();
  const loggedIn = !!account;
  const { data, isPending, error, refetch, fetchNextPage, hasNextPage, isFetchingNextPage } =
    useComments(vid);
  // 翻页数据拍平；total 取第一页的 need_count 回传（互动栏同源）
  const comments = data?.pages.flatMap((p) => p.items);
  const total = data?.pages[0]?.total || comments?.length || 0;
  const send = useSendComment();
  const sendReply = useSendReply();
  const [text, setText] = useState('');
  /** 本地点赞态覆盖（评论/回复 id → 点赞后）：列表 best-effort + 乐观 */
  const [liked, setLiked] = useState<Set<string>>(new Set());
  /** 回复输入框展开在哪条评论上（空 = 无） */
  const [replyTarget, setReplyTarget] = useState<string | null>(null);
  /** 回复「回复」时被回复对象（二级回复；name 供占位文案，id 供
   *  reply_to_reply_id） */
  const [replyToReply, setReplyToReply] = useState<{ id: string; name: string } | null>(null);
  const [replyText, setReplyText] = useState('');
  /** 自己发的回复（commentId → 本地追加），服务端列表有延迟先上屏 */
  const [localReplies, setLocalReplies] = useState<Record<string, LocalReply[]>>({});
  /** 待确认删除的目标（comment/del service_id=4；parentId 在场 = 回复） */
  const [deleteTarget, setDeleteTarget] = useState<{ id: string; parentId?: string } | null>(null);
  const queryClient = useQueryClient();
  const myUserId = account?.userId ?? '';

  /** 删除自己的评论 / 回复（本地摘除，免重取整列表）。 */
  const confirmDelete = () => {
    const target = deleteTarget;
    setDeleteTarget(null);
    if (!target) return;
    interact
      .deleteComment(target.id, 4)
      .then(() => {
        if (target.parentId) {
          queryClient.setQueryData<InfiniteData<ReplyPage, string>>(
            keys.commentReplies(vid, target.parentId),
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
            keys.comments(vid),
            (prev) =>
              prev && {
                ...prev,
                pages: prev.pages.map((p, i) => ({
                  ...p,
                  total: i === 0 ? Math.max(0, p.total - 1) : p.total,
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

  const submit = () => {
    const content = text.trim();
    if (!content || send.isPending) return;
    if (!loggedIn) {
      toast.info(t('player.interact.loginRequired'));
      return;
    }
    send.mutate(
      { vid, text: content },
      {
        onSuccess: () => {
          setText('');
        },
        onError: (e) => toast.error(String(e)),
      },
    );
  };

  const submitReply = (parent: CommentItem) => {
    const content = replyText.trim();
    if (!content || sendReply.isPending) return;
    if (!loggedIn) {
      toast.info(t('player.interact.loginRequired'));
      return;
    }
    sendReply.mutate(
      {
        vid,
        replyToCommentId: parent.commentId,
        replyToReplyId: replyToReply?.id,
        text: content,
      },
      {
        onSuccess: () => {
          setLocalReplies((prev) => ({
            ...prev,
            [parent.commentId]: [
              ...(prev[parent.commentId] ?? []),
              { text: content, replyTo: replyToReply?.name },
            ],
          }));
          setReplyText('');
          setReplyTarget(null);
          setReplyToReply(null);
        },
        onError: (e) => toast.error(String(e)),
      },
    );
  };

  /** 评论 / 回复通用点赞（do_action 8/9，回复同形态传 reply_id）。 */
  const onDigg = (id: string, nextLiked: boolean) => {
    if (!loggedIn) {
      toast.info(t('player.interact.loginRequired'));
      return;
    }
    interact
      .commentDigg(id, nextLiked)
      .then(() => {
        setLiked((prev) => {
          const next = new Set(prev);
          if (nextLiked) next.add(id);
          else next.delete(id);
          return next;
        });
      })
      .catch((e) => toast.error(String(e)));
  };

  /** 回复某条回复（二级回复）：定位到所属评论的回复框并带上 @ 对方。 */
  const openReplyTo = (commentId: string, reply: ReplyItem) => {
    setReplyTarget(commentId);
    setReplyToReply({ id: reply.replyId, name: reply.userName || t('player.comments.anon') });
    setReplyText('');
  };

  return (
    <div
      data-wheel-block
      className="absolute right-0 bottom-14 z-40 flex h-[68%] w-[380px] max-w-[85%] flex-col rounded-tl-xl border-t border-l border-white/10 bg-neutral-950/95 text-neutral-100 shadow-2xl backdrop-blur-sm"
    >
      {/* 头部 */}
      <div className="flex h-12 shrink-0 items-center justify-between border-b border-white/10 px-4">
        <p className="text-sm font-semibold">
          {t('player.comments.title')}
          {total > 0 && <span className="ml-2 text-xs text-neutral-400">{total}</span>}
        </p>
        <button
          type="button"
          onClick={onClose}
          className="grid size-7 place-items-center rounded-md text-neutral-400 hover:bg-neutral-800 hover:text-white"
          aria-label={t('common.close')}
        >
          <X className="size-4" />
        </button>
      </div>

      {/* 列表 */}
      <div className="min-h-0 flex-1 scrollbar-thin overflow-y-auto px-4 py-3">
        {/* vid 未就绪（档案还没解析完）：查询是禁用态，别挂一个永转的圈 */}
        {!vid.includes(':') ? (
          <p className="text-muted-foreground py-10 text-center text-xs">
            {t('player.comments.preparing')}
          </p>
        ) : (
          <>
            {isPending && (
              <div className="grid place-items-center py-10 text-neutral-400">
                <Loader2 className="size-5 animate-spin" />
              </div>
            )}
            {error && (
              <div className="grid place-items-center gap-2 py-10 text-center text-xs text-neutral-400">
                <p>{t('player.comments.loadFailed')}</p>
                <Button size="sm" variant="outline" onClick={() => void refetch()}>
                  {t('feed.retry')}
                </Button>
              </div>
            )}
            {comments?.length === 0 && (
              <p className="text-muted-foreground py-10 text-center text-xs">
                {t('player.comments.empty')}
              </p>
            )}
            <ul className="flex flex-col gap-4">
              {(comments ?? []).map((c) => {
                const isLiked = liked.has(c.commentId) || c.userDigg;
                const isMine = !!myUserId && c.userId === myUserId;
                return (
                  <li key={c.commentId} className="flex gap-2.5">
                    {c.avatar ? (
                      <img
                        src={c.avatar}
                        alt=""
                        loading="lazy"
                        className="size-8 shrink-0 rounded-full"
                      />
                    ) : (
                      <div className="grid size-8 shrink-0 place-items-center rounded-full bg-neutral-700 text-xs">
                        {(c.userName || '友').slice(0, 1)}
                      </div>
                    )}
                    <div className="min-w-0 flex-1">
                      <p className="text-xs text-neutral-400">
                        {c.userName || t('player.comments.anon')}
                      </p>
                      <p className="mt-0.5 text-sm leading-snug break-words whitespace-pre-wrap">
                        <EmojiText text={c.text} />
                      </p>
                      <div className="mt-1 flex items-center gap-3 text-[11px] text-neutral-500">
                        <span>{relativeTime(c.createTime)}</span>
                        <button
                          type="button"
                          onClick={() => {
                            setReplyTarget(replyTarget === c.commentId ? null : c.commentId);
                            setReplyToReply(null);
                            setReplyText('');
                          }}
                          className="cursor-pointer hover:text-neutral-300"
                        >
                          {t('player.comments.reply')}
                        </button>
                        {isMine && (
                          <button
                            type="button"
                            onClick={() => setDeleteTarget({ id: c.commentId })}
                            className="cursor-pointer hover:text-red-400"
                          >
                            {t('common.delete')}
                          </button>
                        )}
                      </div>
                      {/* 回复列表（2026-10-10 起 reply/list 可用）：展开按需拉取，
                    自己发的回复本地追加；回复行的「回复」走二级回复 */}
                      <ReplySection
                        vid={vid}
                        commentId={c.commentId}
                        replyCount={c.replyCount}
                        localReplies={localReplies[c.commentId] ?? []}
                        liked={liked}
                        myUserId={myUserId}
                        onDigg={onDigg}
                        onReplyTo={(r) => openReplyTo(c.commentId, r)}
                        onDeleteReply={(r) =>
                          setDeleteTarget({ id: r.replyId, parentId: c.commentId })
                        }
                      />
                      {/* 回复输入框 */}
                      {replyTarget === c.commentId && (
                        <EmojiSendBox
                          autoFocus
                          value={replyText}
                          onChange={setReplyText}
                          onSubmit={() => submitReply(c)}
                          onEscape={() => setReplyTarget(null)}
                          placeholder={
                            replyToReply
                              ? tf('player.comments.replyPlaceholder', { name: replyToReply.name })
                              : t('player.comments.replyToComment')
                          }
                          maxLength={200}
                          pending={sendReply.isPending}
                          pickerAlign="right"
                          className="mt-1.5 gap-1.5"
                          inputClassName="h-8 min-w-0 flex-1 scrollbar-none overflow-x-auto rounded-md bg-neutral-800/80 px-3 text-xs leading-8 whitespace-pre text-white"
                          sendClassName="h-8 bg-red-500 px-3 text-xs hover:bg-red-500/90"
                        />
                      )}
                    </div>
                    <button
                      type="button"
                      onClick={() => onDigg(c.commentId, !isLiked)}
                      className={cn(
                        'flex shrink-0 cursor-pointer flex-col items-center gap-0.5 self-start pt-1 text-neutral-400 hover:text-white',
                        isLiked && 'text-red-400',
                      )}
                      title={t('player.interact.like')}
                    >
                      <Heart className={cn('size-4', isLiked && 'fill-red-400 text-red-400')} />
                      {c.diggCount > 0 && (
                        <span className="text-[10px] tabular-nums">{c.diggCount}</span>
                      )}
                    </button>
                  </li>
                );
              })}
            </ul>
            {/* 加载更多（翻页）：第一页秒开，剩余的按需续拉 */}
            {hasNextPage && (
              <div className="grid place-items-center py-3">
                <Button
                  size="sm"
                  variant="outline"
                  disabled={isFetchingNextPage}
                  onClick={() => void fetchNextPage()}
                >
                  {isFetchingNextPage ? (
                    <Loader2 className="mr-1 size-3.5 animate-spin" aria-hidden />
                  ) : null}
                  {tf('player.comments.loadMore', {
                    n: Math.max(total - (comments?.length ?? 0), 0),
                  })}
                </Button>
              </div>
            )}
          </>
        )}
      </div>

      {/* 底部发评论 */}
      <div className="shrink-0 border-t border-white/10 p-3">
        <EmojiSendBox
          value={text}
          onChange={setText}
          onSubmit={submit}
          placeholder={t('player.comments.placeholder')}
          maxLength={200}
          pending={send.isPending}
          pickerAlign="right"
          inputClassName="h-9 min-w-0 flex-1 scrollbar-none overflow-x-auto rounded-md bg-neutral-800/80 px-3 text-sm leading-9 whitespace-pre text-white"
          sendClassName="h-9 bg-red-500 px-4 hover:bg-red-500/90"
        />
      </div>

      {/* 删除确认（comment/del service_id=4：评论与回复同款） */}
      <ConfirmDialog
        open={!!deleteTarget}
        onOpenChange={(open) => !open && setDeleteTarget(null)}
        title={t('common.deleteConfirmTitle')}
        description={t('common.deleteConfirmDesc')}
        confirmLabel={t('common.delete')}
        onConfirm={confirmDelete}
      />
    </div>
  );
}
