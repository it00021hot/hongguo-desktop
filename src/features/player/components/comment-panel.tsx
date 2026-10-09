//! 沉浸流评论面板：右侧滑出（hgplayer 同款「评论 · N」列）。
//!
//! 列表（头像/昵称/文本/时间/♡）+ 底部发评论框；评论点赞走
//! commentapi/comment/do_action（8/9）。数据与弹幕同端点不同形态——
//! 2026-10-06 抓包重锁：评论形态 business_param 是 need_count/req_type +
//! server_channel=18（沿用弹幕形会被 103001 拒），need_count 顺带带回
//! 评论总数（头部计数用）。
//!
//! 回复：发送走 reply/add 独立端点；回复**列表**服务端无拉取接口
//! （probe 实证 aid 8662 无 handler，hgplayer 也不拉）——自己发的回复
//! 本地追加展示，别人的回复只显示计数。

import { useEffect, useRef, useState } from 'react';
import { CornerDownRight, Heart, Loader2, X } from 'lucide-react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { t, tf } from '@/i18n';
import { cn } from '@/lib/utils';
import { parseEmojiSegments } from '@/lib/danmaku-emoji';
import { interact } from '@/service/commands';
import { useAccount, useComments, useSendComment, useSendReply } from '@/service/queries';
import { EmojiPickerButton } from './emoji-picker';
import { RichEmojiInput, type RichEmojiInputHandle } from './rich-emoji-input';
import type { CommentItem } from '@/service/schema';

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

/** 评论/回复文本：`[名字]` 表情代码渲染成图（hgplayer EmojiText 同款），
 *  其余文本原样保留。 */
function EmojiText({ text }: { text: string }) {
  return (
    <>
      {parseEmojiSegments(text).map((seg, i) =>
        seg.kind === 'text' ? (
          <span key={i}>{seg.value}</span>
        ) : (
          <img
            key={i}
            src={seg.url}
            alt={seg.value}
            title={seg.value}
            draggable={false}
            className="mx-px inline-block size-[1.15em] object-contain align-[-0.18em]"
          />
        ),
      )}
    </>
  );
}

/** 本地追加的一条回复（自己发的；服务端暂无回复列表接口）。 */
interface LocalReply {
  text: string;
  /** 回复「回复」时对方内容摘要（「回复 @xxx」展示用） */
  replyTo?: string;
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
  /** 本地点赞态覆盖（commentId → 点赞后）：列表 best-effort + 乐观 */
  const [liked, setLiked] = useState<Set<string>>(new Set());
  /** 回复输入框展开在哪条评论上（空 = 无） */
  const [replyTarget, setReplyTarget] = useState<string | null>(null);
  /** 回复「回复」时被回复内容摘要（二级回复，仅展示语义） */
  const [replyToReply, setReplyToReply] = useState<{ id: string; text: string } | null>(null);
  const [replyText, setReplyText] = useState('');
  /** 自己发的回复（commentId → 本地追加），服务端暂无回复列表可拉 */
  const [localReplies, setLocalReplies] = useState<Record<string, LocalReply[]>>({});
  /** 发评论 / 回复的富输入框 ref（表情插入走 ref 方法） */
  const composerRef = useRef<RichEmojiInputHandle | null>(null);
  const replyInputRef = useRef<RichEmojiInputHandle | null>(null);
  /** 表情面板开合（底部发评论 / 回复行各一份） */
  const [composerEmojiOpen, setComposerEmojiOpen] = useState(false);
  const [replyEmojiOpen, setReplyEmojiOpen] = useState(false);

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
              { text: content, replyTo: replyToReply?.text },
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

  const onDigg = (item: CommentItem) => {
    if (!loggedIn) {
      toast.info(t('player.interact.loginRequired'));
      return;
    }
    const nextLiked = !liked.has(item.commentId);
    interact
      .commentDigg(item.commentId, nextLiked)
      .then(() => {
        setLiked((prev) => {
          const next = new Set(prev);
          if (nextLiked) next.add(item.commentId);
          else next.delete(item.commentId);
          return next;
        });
      })
      .catch((e) => toast.error(String(e)));
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
              {(comments ?? []).map((c) => (
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
                      {/* 回复入口：服务端暂无回复列表接口，这里只做「回复他」+
                      自己回复的本地展示；别人的回复数量只读 */}
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
                      {c.replyCount > 0 && (
                        <span title={t('player.comments.repliesHiddenTip')}>
                          {tf('player.comments.replies', { n: c.replyCount })}
                        </span>
                      )}
                    </div>
                    {/* 自己发的回复（本地追加） */}
                    {(localReplies[c.commentId]?.length ?? 0) > 0 && (
                      <div className="mt-1.5 flex flex-col gap-1.5 border-l-2 border-neutral-800 pl-2.5">
                        {localReplies[c.commentId]!.map((r, i) => (
                          <div key={i} className="text-xs leading-snug">
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
                      </div>
                    )}
                    {/* 回复输入框 */}
                    {replyTarget === c.commentId && (
                      <div className="mt-1.5 flex items-center gap-1.5">
                        <RichEmojiInput
                          ref={replyInputRef}
                          autoFocus
                          value={replyText}
                          onChange={setReplyText}
                          onEnter={() => submitReply(c)}
                          onEscape={() => setReplyTarget(null)}
                          placeholder={
                            replyToReply
                              ? tf('player.comments.replyPlaceholder', { name: replyToReply.text })
                              : t('player.comments.replyToComment')
                          }
                          maxLength={200}
                          className="h-8 min-w-0 flex-1 scrollbar-none overflow-x-auto rounded-md bg-neutral-800/80 px-3 text-xs leading-8 whitespace-pre text-white"
                        />
                        <EmojiPickerButton
                          open={replyEmojiOpen}
                          onToggle={() => setReplyEmojiOpen((o) => !o)}
                          align="right"
                          onPick={(name) => replyInputRef.current?.insertEmoji(name)}
                        />
                        <Button
                          size="sm"
                          className="h-8 shrink-0 bg-red-500 px-3 text-xs text-white hover:bg-red-500/90"
                          disabled={!replyText.trim() || sendReply.isPending}
                          onClick={() => submitReply(c)}
                        >
                          {t('player.interact.send')}
                        </Button>
                      </div>
                    )}
                  </div>
                  <button
                    type="button"
                    onClick={() => onDigg(c)}
                    className={cn(
                      'flex shrink-0 cursor-pointer flex-col items-center gap-0.5 self-start pt-1 text-neutral-400 hover:text-white',
                      (liked.has(c.commentId) || c.userDigg) && 'text-red-400',
                    )}
                    title={t('player.interact.like')}
                  >
                    <Heart
                      className={cn(
                        'size-4',
                        (liked.has(c.commentId) || c.userDigg) && 'fill-red-400 text-red-400',
                      )}
                    />
                    {c.diggCount > 0 && (
                      <span className="text-[10px] tabular-nums">{c.diggCount}</span>
                    )}
                  </button>
                </li>
              ))}
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
        <div className="flex items-center gap-2">
          <RichEmojiInput
            ref={composerRef}
            value={text}
            onChange={setText}
            onEnter={submit}
            placeholder={t('player.comments.placeholder')}
            maxLength={200}
            className="h-9 min-w-0 flex-1 scrollbar-none overflow-x-auto rounded-md bg-neutral-800/80 px-3 text-sm leading-9 whitespace-pre text-white"
          />
          <EmojiPickerButton
            open={composerEmojiOpen}
            onToggle={() => setComposerEmojiOpen((o) => !o)}
            align="right"
            onPick={(name) => composerRef.current?.insertEmoji(name)}
          />
          <Button
            size="sm"
            className="h-9 shrink-0 bg-red-500 px-4 text-white hover:bg-red-500/90"
            disabled={!text.trim() || send.isPending}
            onClick={submit}
          >
            {t('player.interact.send')}
          </Button>
        </div>
      </div>
    </div>
  );
}
