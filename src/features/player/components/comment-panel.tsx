//! 沉浸流评论面板：右侧滑出（「评论 · N」列）。
//!
//! 列表（头像/昵称/文本/时间/♡）+ 底部发评论框；评论点赞走
//! commentapi/comment/do_action（8/9）。数据与弹幕同端点不同形态——
//! 2026-10-06 抓包重锁：评论形态 business_param 是 need_count/req_type +
//! server_channel=18（沿用弹幕形会被 103001 拒），need_count 顺带带回
//! 评论总数（头部计数用）。
//!
//! 回复：发送走 reply/add 独立端点；回复**列表**自 2026-10-10 抓包
//! 确认可用（reply/list 独立端点，评论维度 src=504/ch=18，
//! 此前「服务端无接口」的结论作废）——「展开 N 条回复」原地内联展开
//! （不隐藏其他评论），自己发的回复仍本地追加一份（服务端列表有延迟）。
//! 回复行格式（2026-10-10 截图）：`回复 @xxx：` 前缀在
//! 正文行（@ 浅蓝高亮）、相对时间（N 天前）、头像 32px。

import { useEffect, useMemo, useRef, useState } from 'react';
import { Check, Heart, Loader2, X } from 'lucide-react';
import { toast } from 'sonner';
import { useQueryClient, type InfiniteData } from '@tanstack/react-query';
import { Button } from '@/components/ui/button';
import { ConfirmDialog } from '@/components/ui/confirm-dialog';
import { t, tf } from '@/locales';
import { cn } from '@/lib/utils';
import { danmaku, emoticon as emoticonCmd, interact } from '@/service/commands';
import {
  emoticonPreviewUrl,
  useAccount,
  useCollectEmoticon,
  useCommentDigg,
  useComments,
  useMentionList,
  useReplyDigg,
  useSendComment,
  useSendReply,
} from '@/service/queries';
import {
  appendReply,
  bumpReplyCount,
  removeComment,
  removeReply,
} from '@/service/queries/comment-cache';
import { dedupBy, keys } from '@/service/queries/common';
import { relativeTime } from '@/utils/relative-time';
import { isImeComposing } from '@/utils/ime';
import { revokePreview, type LocalImageAttachment } from '@/utils/image-attachment';
import { EmojiText } from '@/components/common/emoji/emoji-text';
import { EmoticonActionsMenu } from '@/components/common/emoji/emoticon-actions-menu';
import type { LongPressPoint } from '@/components/common/emoji/use-long-press';
import { CommentImage } from './comment-image';
import { ReplySection } from './comment-reply-section';
import { EmojiSendBox, type EmojiSendBoxHandle } from '@/components/common/emoji/emoji-send-box';

/** aibot_mention_context 带的最近评论条数（官方实包 4–6 条）。 */
const MENTION_CONTEXT_LIMIT = 6;
/** @小果AI 回复轮询间隔与放弃时限（官方到达 10–40s，留足余量）。 */
const AI_REPLY_POLL_MS = 5000;
const AI_REPLY_WAIT_MS = 120000;

/** 评论正文：`@昵称\uFFFC` 提及前缀（U+FFFC 是提及段占位符）渲染成
 *  官方同款蓝色 @昵称（AI bot 加渐变 Ai 后缀），其余正文（表情代码等）
 *  走 EmojiText。 */
function CommentText({ text, aiName }: { text: string; aiName?: string }) {
  const mention = /^@([^\s\uFFFC]+)\uFFFC\s*/.exec(text);
  if (!mention) return <EmojiText text={text} />;
  return (
    <>
      <span className="mr-1 text-sky-400">
        @{mention[1]}
        {mention[1] === aiName && (
          <span className="bg-gradient-to-r from-amber-400 to-pink-500 bg-clip-text font-semibold text-transparent">
            Ai
          </span>
        )}
      </span>
      <EmojiText text={text.slice(mention[0].length)} />
    </>
  );
}
import type { CommentItem, Emoticon, ReplyItem, ReplyPage } from '@/service/schema';

/**
 * 评论附件（官方同款两种形态，2026-10-25 实包锁定）：
 * - 表情图片（第三方 GIF / 自定义表情 / 搜索结果）：服务端下发的表情对象
 *   直接进 `image_data`，附 `business_param.emoji_scene_type`（2 / 4）；
 * - 本地图片：选图后**不立即上传**（官方实测选图无请求），点「发表」才
 *   走 `/reading/reader/upload/picture/`，再把签名 URL 填进 `image_data`。
 */
type CommentAttachment =
  | { kind: 'emoticon'; item: Emoticon; sceneType: number }
  | { kind: 'local'; local: LocalImageAttachment };

interface Props {
  vid: string;
  onClose: () => void;
}

export function CommentPanel({ vid, onClose }: Props) {
  // Esc 关闭（面板盖住互动栏按钮时这是最直接的退出路径）
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      // 输入法组合中的 Esc 是取消拼音，不是关面板
      if (isImeComposing(e)) return;
      if (e.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onClose]);
  const { data: account } = useAccount();
  const loggedIn = !!account;
  const { data, isPending, error, refetch, fetchNextPage, hasNextPage, isFetchingNextPage } =
    useComments(vid);
  // 翻页数据拍平 + 按 commentId 去重（服务端相邻页重叠条目）；total 取
  // 第一页的 need_count 回传（互动栏同源）
  const comments = useMemo(
    () => dedupBy(data?.pages.flatMap((p) => p.items) ?? [], (c) => c.commentId),
    [data],
  );
  const total = data?.pages[0]?.total || comments?.length || 0;
  const send = useSendComment();
  const sendReply = useSendReply();
  // @ 提及（官方交互，2026-10-15 抓 player 实包）：输入 `@` 触发
  // relation/list 候选弹层，选中小果插入提及段，发表时走 rich_text
  // tp=15 + aibot_mention_context（AI bot 会回复剧情介绍）。
  // ⚠️ vid 是「分集vid:seriesId」组合形态——relation/list 的 target 是
  // seriesId（后半段）；取错段查出来恒空，@ 候选就是死的。
  const seriesId = vid.split(':')[1] ?? '';
  const mentionQ = useMentionList(seriesId);
  const aibot = mentionQ.data?.find((m) => m.isAiBot) ?? null;
  /** 点赞（缓存写回型乐观更新，hooks 内失败回滚）：评论与回复分走
   *  两个 hook——回复的乐观值要写进所属评论的回复缓存，key 不同 */
  const commentDigg = useCommentDigg(vid);
  const replyDigg = useReplyDigg(vid);
  const [text, setText] = useState('');
  /** 评论附件（表情图片 / 本地图片；官方两种形态，见 CommentAttachment） */
  const [attachment, setAttachment] = useState<CommentAttachment | null>(null);
  /** 本地图片上传中（官方：选图不请求，点发表才上传） */
  const [uploading, setUploading] = useState(false);
  /** 长按评论附图 → 「添加到表情」菜单（官方同款） */
  const [collectMenu, setCollectMenu] = useState<{
    item: Emoticon;
    point: LongPressPoint;
  } | null>(null);
  const collectEmoticon = useCollectEmoticon();
  /** 评论输入框句柄：@ 提及插入要落在浏览器维护的光标上（脚本拼文本
   *  + 程序化选区会与 Chromium 编辑光标脱节，见 rich-emoji-input） */
  const sendBoxRef = useRef<EmojiSendBoxHandle | null>(null);
  /** 最近一次插入的提及对象（uid 供发送；发完/撤销即清） */
  const [mentionUser, setMentionUser] = useState<{ userId: string; name: string } | null>(null);
  /** @提及评论的「思考中…」占位（评论 id 列表）：轮询到 AI 回复后撤除 */
  const [aiPending, setAiPending] = useState<string[]>([]);
  /** 输入以 `@词` 结尾 = 提及候选弹层开着（官方同款触发：打 @ 即弹；
   *  \uFFFC 是提及段的结尾占位符，含它说明提及已插入，不再触发） */
  const mentionOpen = /(^|\s)@([^\s@￼]*)$/.test(text);
  /** 回复输入框展开在哪条评论上（空 = 无） */
  const [replyTarget, setReplyTarget] = useState<string | null>(null);
  /** 回复「回复」时被回复对象（二级回复；name 供占位文案，id 供
   *  reply_to_reply_id） */
  const [replyToReply, setReplyToReply] = useState<{ id: string; name: string } | null>(null);
  const [replyText, setReplyText] = useState('');
  /** 展开了回复区的评论（受控：回复发送成功后强制展开） */
  const [openReplies, setOpenReplies] = useState<Set<string>>(new Set());
  const setReplySectionOpen = (commentId: string, open: boolean) =>
    setOpenReplies((prev) => {
      const next = new Set(prev);
      if (open) next.add(commentId);
      else next.delete(commentId);
      return next;
    });
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
          // 回复：回复缓存摘除 + total-1，父评论 replyCount-1——与发送
          // 成功的 +1 对称
          removeReply(queryClient, keys.commentReplies(vid, target.parentId), target.id);
          bumpReplyCount(queryClient, keys.comments(vid), target.parentId, -1);
        } else {
          removeComment(queryClient, keys.comments(vid), target.id);
          if (replyTarget === target.id) setReplyTarget(null);
        }
        toast.success(t('common.deleted'));
      })
      .catch((e) => toast.error(String(e)));
  };

  /** @提及评论的 AI 回复轮询：每拍拉一次回复首页，到达即写缓存（去重 +
   * 计数）并撤占位；超时放弃撤占位（服务端偶尔且回复时不再干等）。面板
   * 卸载或换集即停。 */
  useEffect(() => {
    if (aiPending.length === 0 || !vid.includes(':')) return;
    const [groupId = '', bookId = ''] = vid.split(':');
    const startedAt = new Map(aiPending.map((id) => [id, Date.now()]));
    let cancelled = false;
    const timer = setInterval(() => {
      for (const commentId of aiPending) {
        if (Date.now() - (startedAt.get(commentId) ?? 0) > AI_REPLY_WAIT_MS) {
          setAiPending((prev) => prev.filter((x) => x !== commentId));
          continue;
        }
        void danmaku
          .commentReplies(groupId, bookId, commentId, '')
          .then((page) => {
            if (cancelled || page.items.length === 0) return;
            const existing = new Set(
              (
                queryClient
                  .getQueryData<InfiniteData<ReplyPage, string>>(
                    keys.commentReplies(vid, commentId),
                  )
                  ?.pages.flatMap((p) => p.items) ?? []
              ).map((r) => r.replyId),
            );
            const fresh = page.items.filter((r) => !existing.has(r.replyId));
            for (const reply of fresh) {
              appendReply(queryClient, keys.commentReplies(vid, commentId), reply);
            }
            if (fresh.length > 0) {
              // 「展开 N 条」计数 +1（回复缓存 total 由 appendReply 自增；
              // 权威计数随下次重拉对齐服务端）
              bumpReplyCount(queryClient, keys.comments(vid), commentId, 1);
              setAiPending((prev) => prev.filter((x) => x !== commentId));
            }
          })
          .catch(() => undefined);
      }
    }, AI_REPLY_POLL_MS);
    return () => {
      cancelled = true;
      clearInterval(timer);
    };
  }, [aiPending, vid, queryClient]);

  /** 换附件：旧的本地预览 URL 要释放（blob 不释放会漏内存）。 */
  const replaceAttachment = (next: CommentAttachment | null) => {
    setAttachment((prev) => {
      if (prev?.kind === 'local') revokePreview(prev.local.previewUrl);
      return next;
    });
  };

  /** 选中表情图片（第三方 GIF / 自定义表情 / 搜索结果）。 */
  const onPickEmoticon = (item: Emoticon, sceneType: number) => {
    replaceAttachment({ kind: 'emoticon', item, sceneType });
  };

  /** 选本地图片（此刻不请求网络，发表时才上传）。选图走原生文件面板。 */
  const onPickLocalImage = (local: LocalImageAttachment) => {
    replaceAttachment({ kind: 'local', local });
  };

  /** 收藏评论里的图片到「我的表情」（官方：长按附图 → 「添加到表情」）。 */
  const onCollectImage = async (item: Emoticon) => {
    setCollectMenu(null);
    if (!loggedIn) {
      toast.info(t('player.interact.loginRequired'));
      return;
    }
    try {
      await collectEmoticon.mutateAsync({
        imageId: item.id ?? '',
        imageUrl: item.webUri,
        width: item.width,
        height: item.height,
        imageType: item.imageType,
      });
      toast.success(t('player.emoticon.addDone'));
    } catch (e) {
      toast.error(String(e));
    }
  };

  const submit = async () => {
    const content = text.trim();
    if ((!content && !attachment) || send.isPending || uploading) return;
    if (!loggedIn) {
      toast.info(t('player.interact.loginRequired'));
      return;
    }
    // 附图与 @提及是官方两套形态，附件在场时走附图流（正文可空）。
    let image: Emoticon | undefined;
    let emojiSceneType: number | null = null;
    if (attachment?.kind === 'emoticon') {
      image = attachment.item;
      emojiSceneType = attachment.sceneType;
    } else if (attachment?.kind === 'local') {
      setUploading(true);
      try {
        const uploaded = await emoticonCmd.upload(
          attachment.local.bytes,
          attachment.local.filename,
        );
        image = {
          id: null,
          webUri: uploaded.url,
          dynamicUrl: null,
          imageType: 1,
          width: attachment.local.width,
          height: attachment.local.height,
          format: attachment.local.format,
          imageName: null,
          thumbNails: null,
          expandWebUrl: null,
        };
      } catch (e) {
        toast.error(String(e));
        return;
      } finally {
        setUploading(false);
      }
    }
    // @ 提及（草稿含提及段占位符 \uFFFC 时按提及评论发送）：text 传剥掉
    // 提及段后的正文（Rust 侧会重拼 `@名字\uFFFC 正文` + rich_text
    // tp=15/uri=uid + aibot_mention_context=面板最近评论 id——官方实包
    // 1539/1616 传的是评论 id 嵌套数组，不是 vid）
    const withMention = content.includes('\uFFFC') && mentionUser != null && !attachment;
    send.mutate(
      {
        vid,
        text: withMention ? content.replace(/@[^\s\uFFFC]*\uFFFC\s*/, '').trim() : content,
        mentionUserId: withMention ? mentionUser.userId : undefined,
        mentionUserName: withMention ? mentionUser.name : undefined,
        contextCommentIds: withMention
          ? comments.slice(0, MENTION_CONTEXT_LIMIT).map((c) => c.commentId)
          : undefined,
        image,
        emojiSceneType,
      },
      {
        onSuccess: (comment) => {
          setText('');
          setMentionUser(null);
          replaceAttachment(null);
          // @提及评论：官方同款「思考中…」占位——回复区强制展开并轮询，
          // AI 回复到达后由轮询写进缓存并撤占位
          if (withMention) {
            setAiPending((prev) =>
              prev.includes(comment.commentId) ? prev : [...prev, comment.commentId],
            );
            setReplySectionOpen(comment.commentId, true);
          }
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
        onSuccess: (reply) => {
          // 服务端回显的完整回复对象直接进缓存（头像/昵称/uid/时间齐全，
          // 新回复以普通条目出现，replyToName 驱动
          // 「回复 @xxx：」前缀）；同时父评论 replyCount +1
          appendReply(queryClient, keys.commentReplies(vid, parent.commentId), reply);
          bumpReplyCount(queryClient, keys.comments(vid), parent.commentId, 1);
          setReplyText('');
          setReplyTarget(null);
          setReplyToReply(null);
          // 发送成功强制展开回复区，让新回复立刻可见
          setReplySectionOpen(parent.commentId, true);
        },
        onError: (e) => toast.error(String(e)),
      },
    );
  };

  /** 评论 / 回复通用点赞（do_action 8/9，回复同形态传 reply_id）。
   *  乐观写进对应 query 缓存（hooks 内失败回滚 + 调用方 toast）——
   *  显示值直读缓存，面板重开不再回跳旧值（2026-10-10 用户实测）。 */
  const onCommentDigg = (id: string, nextLiked: boolean) => {
    if (!loggedIn) {
      toast.info(t('player.interact.loginRequired'));
      return;
    }
    commentDigg.mutate(
      { commentId: id, digg: nextLiked },
      { onError: (e) => toast.error(String(e)) },
    );
  };
  const onReplyDigg = (parentId: string, id: string, nextLiked: boolean) => {
    if (!loggedIn) {
      toast.info(t('player.interact.loginRequired'));
      return;
    }
    replyDigg.mutate(
      { commentId: parentId, replyId: id, digg: nextLiked },
      { onError: (e) => toast.error(String(e)) },
    );
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
                const isLiked = c.userDigg;
                const diggCount = c.diggCount;
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
                        <CommentText text={c.text} aiName={aibot?.userName} />
                      </p>
                      {/* 附图（本地图片 / 表情图片；纯文字评论无）——长按可收藏进「我的表情」 */}
                      {c.images.length > 0 && (
                        <div className="mt-1.5 flex flex-wrap gap-1.5">
                          {c.images.map((img, index) => (
                            <CommentImage
                              key={`${img.webUri}-${index}`}
                              image={img}
                              onMenu={(target, point) => setCollectMenu({ item: target, point })}
                            />
                          ))}
                        </div>
                      )}
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
                      {/* 回复列表（2026-10-10 起 reply/list 可用）：原地内联
                    展开，其他评论不隐藏；回复行「回复」走二级回复，新回复
                    直接进缓存 */}
                      <ReplySection
                        vid={vid}
                        commentId={c.commentId}
                        replyCount={c.replyCount}
                        myUserId={myUserId}
                        open={openReplies.has(c.commentId)}
                        pendingAi={aiPending.includes(c.commentId)}
                        aiName={aibot?.userName}
                        onToggle={() =>
                          setReplySectionOpen(c.commentId, !openReplies.has(c.commentId))
                        }
                        onDigg={(id, d) => onReplyDigg(c.commentId, id, d)}
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
                          sendClassName="h-8 px-3 text-xs"
                        />
                      )}
                    </div>
                    <button
                      type="button"
                      onClick={() => onCommentDigg(c.commentId, !isLiked)}
                      className={cn(
                        'flex shrink-0 cursor-pointer flex-col items-center gap-0.5 self-start pt-1 text-neutral-400 hover:text-white',
                        isLiked && 'text-red-400',
                      )}
                      title={t('player.interact.like')}
                    >
                      <Heart className={cn('size-4', isLiked && 'fill-red-400 text-red-400')} />
                      {diggCount > 0 && (
                        <span className="text-[10px] tabular-nums">{diggCount}</span>
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

      {/* 底部发评论（relative：表情面板的锚点靠它定位到发送区顶边） */}
      <div className="relative shrink-0 border-t border-white/10 p-3">
        {/* @ 候选条（官方交互：输入 @ 触发 relation/list；官方形态是
            输入行上方的一排大头像候选，AI 头像带橙勾徽标、名字带渐变
            Ai 后缀）。点击候选用 onMouseDown 防输入框失焦；插入走输入框
            句柄（浏览器编辑命令），不经 state 重建 DOM——那会脱节编辑
            光标、吞掉提及段尾随空格 */}
        {mentionOpen && (
          <div className="relative">
            <div className="absolute bottom-2 left-0 z-10 flex w-full scrollbar-none items-center gap-2 overflow-x-auto rounded-lg border border-white/10 bg-neutral-800 p-2 shadow-lg">
              {mentionQ.isPending ? (
                <div className="flex items-center gap-2 px-3 py-2.5 text-xs text-white/60">
                  <Loader2 className="size-3.5 animate-spin" aria-hidden />
                  {t('player.comments.mentionLoading')}
                </div>
              ) : (mentionQ.data?.length ?? 0) > 0 ? (
                mentionQ.data!.map((u) => (
                  <button
                    key={u.userId}
                    type="button"
                    onMouseDown={(e) => {
                      // 焦点纪律：不让输入框失焦（选完还要继续打字）
                      e.preventDefault();
                      setMentionUser({ userId: u.userId, name: u.userName });
                      sendBoxRef.current?.insertMention(u.userName);
                    }}
                    className="flex w-14 shrink-0 flex-col items-center gap-1 rounded-md p-1 transition-colors hover:bg-white/10"
                  >
                    <span className="relative">
                      {u.avatar ? (
                        <img src={u.avatar} alt="" className="size-11 rounded-full object-cover" />
                      ) : (
                        <span className="grid size-11 place-items-center rounded-full bg-neutral-600 text-sm">
                          {u.userName.slice(0, 1)}
                        </span>
                      )}
                      {u.isAiBot && (
                        <span className="absolute -right-0.5 -bottom-0.5 grid size-4 place-items-center rounded-full bg-orange-500 text-white ring-2 ring-neutral-800">
                          <Check className="size-2.5" strokeWidth={3} />
                        </span>
                      )}
                    </span>
                    <span className="flex max-w-full items-baseline gap-px text-[11px] leading-tight text-white/90">
                      <span className="truncate">{u.userName}</span>
                      {u.isAiBot && (
                        <span className="bg-gradient-to-r from-amber-400 to-pink-500 bg-clip-text font-semibold text-transparent">
                          Ai
                        </span>
                      )}
                    </span>
                  </button>
                ))
              ) : (
                <div className="px-3 py-2.5 text-xs text-white/60">
                  {loggedIn
                    ? t('player.comments.mentionUnavailable')
                    : t('player.interact.loginRequired')}
                </div>
              )}
            </div>
          </div>
        )}
        {/* 附件预览（官方同款：输入框上方一排缩略图，带删除钮） */}
        {attachment && (
          <div className="mb-2 flex items-center gap-2">
            <div className="relative">
              <img
                src={
                  attachment.kind === 'local'
                    ? attachment.local.previewUrl
                    : emoticonPreviewUrl(attachment.item)
                }
                alt=""
                className="size-16 rounded-md border border-white/10 object-cover"
              />
              <button
                type="button"
                onClick={() => replaceAttachment(null)}
                aria-label={t('player.emoticon.imageRemove')}
                className="absolute -top-1.5 -right-1.5 grid size-5 cursor-pointer place-items-center rounded-full bg-neutral-800 text-neutral-300 ring-1 ring-white/20 hover:text-white"
              >
                <X className="size-3" />
              </button>
            </div>
          </div>
        )}
        <div className="flex items-center gap-2">
          <div className="min-w-0 flex-1">
            <EmojiSendBox
              ref={sendBoxRef}
              value={text}
              mentionAiName={aibot?.userName}
              onChange={setText}
              onSubmit={() => void submit()}
              onEscape={() => setText((prev) => prev.replace(/(^|\s)@[^\s@￼]*$/, '$1'))}
              placeholder={t('player.comments.placeholder')}
              maxLength={200}
              pending={send.isPending || uploading}
              canSend={text.trim() !== '' || attachment !== null}
              pickerAlign="right"
              pickerAnchorTo="send-area"
              inputClassName="h-9 min-w-0 flex-1 scrollbar-none overflow-x-auto rounded-md bg-neutral-800/80 px-3 text-sm leading-9 whitespace-pre text-white"
              sendClassName="h-9 px-4"
              onPickImage={onPickEmoticon}
              onPickLocalImage={(file) => void onPickLocalImage(file)}
            />
          </div>
        </div>
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

      {/* 长按评论附图 → 「添加到表情」（官方同款：collect_type=1） */}
      {collectMenu && (
        <EmoticonActionsMenu
          x={collectMenu.point.x}
          y={collectMenu.point.y}
          canRemove={false}
          onAdd={() => void onCollectImage(collectMenu.item)}
          onRemove={() => setCollectMenu(null)}
          onClose={() => setCollectMenu(null)}
        />
      )}
    </div>
  );
}
