//! 回复线程（展开 N 条回复）与回复行：评论（player 浮层）与剧评（series 页）
//! 的收敛实现——两边逻辑一致，只有皮肤与 i18n 键位差异。
//!
//! 皮肤差异是**合法的**（红线 11：player 区允许 hgplayer 深色硬编码色，页面区
//! 用语义令牌），用 `variant` 参数化后逐类名保留，不改动任何一侧的视觉形态。
//! i18n 键位同理（`player.comments.*` ↔ `detail.*`；`anon`/`delete` 两边同键）。
//!
//! 查询结果的解包留在调用方（各自绑定 `useCommentReplies` / `useReviewReplies`
//! 后把解包后的 `replies` 与分页标志传进来），本组件保持无 hook 的纯展示。

import { ChevronDown, Heart, Loader2 } from 'lucide-react';

import { EmojiText } from '@/components/common/emoji/emoji-text';
import { t, tf } from '@/locales';
import { cn } from '@/lib/utils';
import type { ReplyItem } from '@/service/schema';
import { relativeTime } from '@/utils/relative-time';

/** 皮肤：类名与 i18n 键位按 variant 取用。 */
const SKIN = {
  player: {
    toggle:
      'flex cursor-pointer items-center gap-1 text-[11px] text-neutral-400 hover:text-neutral-200',
    thread: 'mt-1.5 flex flex-col gap-2 border-l-2 border-neutral-800 pl-2.5',
    pending: 'grid place-items-center py-2 text-neutral-500',
    loadFailed: 'cursor-pointer text-left text-[11px] text-neutral-500 hover:text-neutral-300',
    more: 'flex w-fit cursor-pointer items-center gap-1 text-[11px] text-neutral-400 hover:text-neutral-200 disabled:opacity-60',
    avatarImg: 'size-8 shrink-0 rounded-full',
    avatarFallback: 'grid size-8 shrink-0 place-items-center rounded-full bg-neutral-700 text-xs',
    name: 'text-[11px] text-neutral-400',
    replyToPrefix: 'text-neutral-500',
    mention: 'text-[#7BB4E8]',
    meta: 'mt-0.5 flex items-center gap-3 text-[10px] text-neutral-500',
    actionBtn: 'cursor-pointer hover:text-neutral-300',
    deleteBtn: 'cursor-pointer hover:text-red-400',
    diggBtn:
      'flex shrink-0 cursor-pointer flex-col items-center gap-0.5 self-start text-neutral-400 hover:text-white',
    diggLiked: 'text-red-400',
    heart: 'size-3',
    heartLiked: 'fill-red-400 text-red-400',
    keys: {
      replyToPrefix: 'player.comments.replyToPrefix',
      reply: 'player.comments.reply',
      like: 'player.interact.like',
      anon: 'player.comments.anon',
      collapseReplies: 'player.comments.collapseReplies',
      expandReplies: 'player.comments.expandReplies',
      moreReplies: 'player.comments.moreReplies',
      repliesLoadFailed: 'player.comments.repliesLoadFailed',
    },
  },
  page: {
    toggle:
      'text-muted-foreground hover:text-foreground flex cursor-pointer items-center gap-1 text-xs',
    thread: 'border-border mt-2 flex flex-col gap-2.5 border-l-2 pl-3',
    pending: 'text-muted-foreground grid place-items-center py-2',
    loadFailed: 'text-muted-foreground hover:text-foreground cursor-pointer text-left text-xs',
    more: 'text-muted-foreground hover:text-foreground flex w-fit cursor-pointer items-center gap-1 text-xs disabled:opacity-60',
    avatarImg: 'size-full object-cover',
    avatarFallback: '',
    name: 'text-muted-foreground text-[11px]',
    replyToPrefix: 'text-muted-foreground',
    mention: '',
    meta: 'text-muted-foreground mt-0.5 flex items-center gap-3 text-[10px]',
    actionBtn: 'hover:text-foreground cursor-pointer',
    deleteBtn: 'cursor-pointer hover:text-destructive',
    diggBtn:
      'text-muted-foreground hover:text-foreground flex shrink-0 cursor-pointer flex-col items-center gap-0.5 self-start',
    diggLiked: 'text-hot',
    heart: 'size-3',
    heartLiked: 'fill-hot text-hot',
    keys: {
      replyToPrefix: 'detail.replyToPrefix',
      reply: 'detail.reply',
      like: 'detail.like',
      anon: 'player.comments.anon',
      collapseReplies: 'detail.collapseReplies',
      expandReplies: 'detail.expandReplies',
      moreReplies: 'detail.moreReplies',
      repliesLoadFailed: 'detail.repliesLoadFailed',
    },
  },
} as const;

export type ReplyThreadVariant = keyof typeof SKIN;

interface RowActions {
  onDigg: (id: string, digg: boolean) => void;
  onReplyTo: (reply: ReplyItem) => void;
  onDelete: (reply: ReplyItem) => void;
}

/** 一条回复行：头像 / 昵称 / 表情文本 / 时间 / ♡（回复点赞与评论点赞同形态，
 *  object_id 传 reply_id）。自己的回复（userId 对上登录 uid）带删除入口。
 *  `aiName` 对上回复作者时：名字带官方同款渐变 Ai 后缀 + 「内容由AI生成」
 *  标记（@小果AI 回复的官方样式）。 */
function ReplyRow({
  reply,
  myUserId,
  variant,
  aiName,
  onDigg,
  onReplyTo,
  onDelete,
}: {
  reply: ReplyItem;
  myUserId: string;
  variant: ReplyThreadVariant;
  aiName?: string;
} & RowActions) {
  const skin = SKIN[variant];
  const isLiked = reply.userDigg;
  const count = reply.diggCount;
  const isMine = !!myUserId && reply.userId === myUserId;
  const isAi = !!aiName && reply.userName === aiName;
  return (
    <div className="flex gap-2">
      {variant === 'page' ? (
        // 页面侧：底色圆壳常驻，图片 object-cover 填充（语义令牌）
        <div className={cn(skin.avatarFallback, 'overflow-hidden rounded-full')}>
          {reply.avatar ? (
            <img src={reply.avatar} alt="" loading="lazy" className={skin.avatarImg} />
          ) : (
            (reply.userName[0] ?? '?')
          )}
        </div>
      ) : // player 侧：有图直出，无图落底色占位
      reply.avatar ? (
        <img src={reply.avatar} alt="" loading="lazy" className={skin.avatarImg} />
      ) : (
        <div className={skin.avatarFallback}>{(reply.userName || '友').slice(0, 1)}</div>
      )}
      <div className="min-w-0 flex-1">
        <p className={skin.name}>
          {reply.userName || t(skin.keys.anon)}
          {isAi && (
            <span className="bg-gradient-to-r from-amber-400 to-pink-500 bg-clip-text font-semibold text-transparent">
              {' '}
              Ai
            </span>
          )}
        </p>
        <p className="mt-0.5 text-xs leading-snug break-words whitespace-pre-wrap">
          {reply.replyToName && (
            <span className={skin.replyToPrefix}>
              {t(skin.keys.replyToPrefix)}{' '}
              {variant === 'player' ? (
                <span className={skin.mention}>@{reply.replyToName}</span>
              ) : (
                <>@{reply.replyToName}</>
              )}
              ：
            </span>
          )}
          <EmojiText text={reply.text} />
        </p>
        <div className={skin.meta}>
          <span>{relativeTime(reply.createTime)}</span>
          {isAi && <span>{t('player.comments.aiGenerated')}</span>}
          <button type="button" onClick={() => onReplyTo(reply)} className={skin.actionBtn}>
            {t(skin.keys.reply)}
          </button>
          {isMine && (
            <button type="button" onClick={() => onDelete(reply)} className={skin.deleteBtn}>
              {t('common.delete')}
            </button>
          )}
        </div>
      </div>
      <button
        type="button"
        onClick={() => onDigg(reply.replyId, !isLiked)}
        className={cn(skin.diggBtn, isLiked && skin.diggLiked)}
        title={t(skin.keys.like)}
      >
        <Heart className={cn(skin.heart, isLiked && skin.heartLiked)} />
        {count > 0 && <span className="text-[9px] tabular-nums">{count}</span>}
      </button>
    </div>
  );
}

/** 「展开 N 条回复」区：原地内联展开（hgplayer 同款），展开才拉首页，
 *  hasMore 续拉。开合受控于父组件——回复发送成功后强制展开，让本地追加的
 *  那条立刻可见（折叠时发回复 = 「没效果」的根源）。
 *
 *  `replyCount <= 0` 整段不渲染（含切换按钮）。查询分页标志由调用方解包
 *  传入，重试按钮直连 `refetch`。 */
export function ReplyThread({
  variant,
  replyCount,
  myUserId,
  open,
  onToggle,
  replies,
  isPending,
  error,
  refetch,
  hasNextPage,
  isFetchingNextPage,
  fetchNextPage,
  pendingAi = false,
  aiName,
  onDigg,
  onReplyTo,
  onDeleteReply,
}: {
  variant: ReplyThreadVariant;
  replyCount: number;
  myUserId: string;
  open: boolean;
  onToggle: () => void;
  replies: ReplyItem[];
  isPending: boolean;
  error: unknown;
  refetch: () => void;
  hasNextPage: boolean;
  isFetchingNextPage: boolean;
  fetchNextPage: () => void;
  /** @小果AI 提及评论的「思考中…」占位：发送成功即显示，回复到达后由
   *  调用方撤除。占位期无回复也强制展开线程（官方同款）。 */
  pendingAi?: boolean;
  /** AI bot 用户名：回复作者对上时带渐变 Ai 后缀 + AI 生成标记 */
  aiName?: string;
  onDigg: (id: string, digg: boolean) => void;
  onReplyTo: (reply: ReplyItem) => void;
  onDeleteReply: (reply: ReplyItem) => void;
}) {
  if (replyCount <= 0 && !pendingAi) return null;
  const skin = SKIN[variant];
  return (
    <div className={variant === 'player' ? 'mt-1' : 'mt-1.5'}>
      {replyCount > 0 && (
        <button type="button" onClick={onToggle} className={skin.toggle}>
          {open ? (
            <>
              {t(skin.keys.collapseReplies)}
              <ChevronDown className="size-3 rotate-180" />
            </>
          ) : (
            <>
              {tf(skin.keys.expandReplies, { n: replyCount })}
              <ChevronDown className="size-3" />
            </>
          )}
        </button>
      )}
      {(open || pendingAi) && (
        <div className={skin.thread}>
          {pendingAi && (
            <div className={cn(skin.pending, 'flex items-center gap-2 py-1 text-xs')}>
              <Loader2 className="size-3.5 animate-spin" aria-hidden />
              <span>{t('player.comments.aiThinking')}</span>
            </div>
          )}
          {isPending && (
            <div className={skin.pending}>
              <Loader2 className="size-3.5 animate-spin" />
            </div>
          )}
          {error ? (
            <button type="button" onClick={() => void refetch()} className={skin.loadFailed}>
              {t(skin.keys.repliesLoadFailed)}
            </button>
          ) : null}
          {replies.map((r) => (
            <ReplyRow
              key={r.replyId}
              reply={r}
              myUserId={myUserId}
              variant={variant}
              aiName={aiName}
              onDigg={onDigg}
              onReplyTo={onReplyTo}
              onDelete={onDeleteReply}
            />
          ))}
          {hasNextPage && (
            <button
              type="button"
              disabled={isFetchingNextPage}
              onClick={() => void fetchNextPage()}
              className={skin.more}
            >
              {isFetchingNextPage && <Loader2 className="size-3 animate-spin" aria-hidden />}
              {t(skin.keys.moreReplies)}
            </button>
          )}
        </div>
      )}
    </div>
  );
}
