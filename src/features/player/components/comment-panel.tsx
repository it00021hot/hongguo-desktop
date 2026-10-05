//! 沉浸流评论面板：右侧滑出（hgplayer 同款「评论 · N」列）。
//!
//! 列表（头像/昵称/文本/时间/♡）+ 底部发评论框；评论点赞走
//! commentapi/comment/do_action（8/9）。数据与弹幕同端点不同形态
//! （ct=4/src=4），服务端偶发 110001 时面板内显示错误可重试。

import { useState } from 'react';
import { Heart, Loader2, X } from 'lucide-react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { t, tf } from '@/i18n';
import { cn } from '@/lib/utils';
import { interact } from '@/lib/ipc/commands';
import { useAccount, useComments, useSendComment } from '@/lib/queries';
import type { CommentItem } from '@/lib/schema';

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

export function CommentPanel({ vid, onClose }: Props) {
  const { data: account } = useAccount();
  const loggedIn = !!account;
  const { data: comments, isPending, error, refetch } = useComments(vid);
  const send = useSendComment();
  const [text, setText] = useState('');
  /** 本地点赞态覆盖（commentId → 点赞后）：列表 best-effort + 乐观 */
  const [liked, setLiked] = useState<Set<string>>(new Set());

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
    <div className="absolute inset-y-0 right-0 z-40 flex w-[360px] max-w-[85%] flex-col border-l border-white/10 bg-neutral-950/95 text-neutral-100 shadow-2xl backdrop-blur-sm">
      {/* 头部 */}
      <div className="flex h-12 shrink-0 items-center justify-between border-b border-white/10 px-4">
        <p className="text-sm font-semibold">
          {t('player.comments.title')}
          {comments && <span className="ml-2 text-xs text-neutral-400">{comments.length}</span>}
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
      <div className="min-h-0 flex-1 overflow-y-auto scrollbar-thin px-4 py-3">
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
                <img src={c.avatar} alt="" loading="lazy" className="size-8 shrink-0 rounded-full" />
              ) : (
                <div className="grid size-8 shrink-0 place-items-center rounded-full bg-neutral-700 text-xs">
                  {(c.userName || '友').slice(0, 1)}
                </div>
              )}
              <div className="min-w-0 flex-1">
                <p className="text-xs text-neutral-400">{c.userName || t('player.comments.anon')}</p>
                <p className="mt-0.5 whitespace-pre-wrap break-words text-sm leading-snug">
                  {c.text}
                </p>
                <div className="mt-1 flex items-center gap-3 text-[11px] text-neutral-500">
                  <span>{relativeTime(c.createTime)}</span>
                  {c.replyCount > 0 && (
                    <span>{tf('player.comments.replies', { n: c.replyCount })}</span>
                  )}
                </div>
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
                {c.diggCount > 0 && <span className="text-[10px] tabular-nums">{c.diggCount}</span>}
              </button>
            </li>
          ))}
        </ul>
      </div>

      {/* 底部发评论 */}
      <div className="shrink-0 border-t border-white/10 p-3">
        <div className="flex items-center gap-2">
          <Input
            value={text}
            onChange={(e) => setText(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter') submit();
            }}
            placeholder={t('player.comments.placeholder')}
            className="h-9 flex-1 border-none bg-neutral-800/80 text-sm text-white placeholder:text-neutral-500 focus-visible:ring-0"
            maxLength={200}
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
