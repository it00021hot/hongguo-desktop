import { call } from '../tauri/invoke';
import {
  bookshelfEntrySchema,
  commentItemSchema,
  interactionStateSchema,
  replyItemSchema,
  type BookshelfEntry,
  type CommentItem,
  type InteractionState,
  type ReplyItem,
} from '../schema';

// ---------------------------------------------------------------- 互动（点赞 / 收藏 / 发弹幕，官方 App API）

/** 互动操作（2026-10-05/06 抓包端点；全部要求登录态，匿名被服务端静默拒）。 */
export const interact = {
  /** 发一条弹幕（offsetMs = 视频内位置毫秒），返回服务端 comment_id。 */
  sendDanmaku: (groupId: string, bookId: string, text: string, offsetMs: number) =>
    call<string>('danmaku_send', { groupId, bookId, text, offsetMs }),
  /** 发一条评论，返回**服务端回显的完整评论对象**（直接插列表顶部）。 */
  sendComment: (groupId: string, bookId: string, text: string) =>
    call<CommentItem>('comment_send', { groupId, bookId, text }, commentItemSchema),
  /** 回复一条评论（reply/add 独立端点）；回复「回复」时传 replyToReplyId。
   *  返回**服务端回显的完整回复对象**（直接插回复区，hgplayer 同款）。 */
  sendReply: (
    groupId: string,
    bookId: string,
    replyToCommentId: string,
    replyToReplyId: string | null,
    text: string,
  ) =>
    call<ReplyItem>(
      'comment_reply',
      {
        groupId,
        bookId,
        replyToCommentId,
        replyToReplyId: replyToReplyId ?? null,
        text,
      },
      replyItemSchema,
    ),
  /** 点赞 / 取消点赞一集（vid = 分集 id）。 */
  videoDigg: (vid: string, seriesId: string, digg: boolean) =>
    call<void>('video_digg', { vid, seriesId, digg }),
  /** 点赞 / 取消点赞一条评论（object_type=8；**回复的点赞同款形态**，
   *  object_id 传 reply_id——2026-10-10 抓包实锤）。 */
  commentDigg: (commentId: string, digg: boolean) =>
    call<void>('comment_digg', { commentId, digg }),
  /** 点赞 / 取消点赞一条剧评（object_type=2 / comment_type=2 / 空埋点，
   *  与评论点赞三处不同，2026-10-10 抓包锁定）。 */
  reviewDigg: (reviewId: string, digg: boolean) => call<void>('review_digg', { reviewId, digg }),
  /** 回复一条剧评（reply/add 剧评形态 commit_source=13）；
   *  回复「回复」时传 replyToReplyId。返回服务端回显的完整回复对象。 */
  reviewReplySend: (
    seriesId: string,
    replyToCommentId: string,
    replyToReplyId: string | null,
    text: string,
  ) =>
    call<ReplyItem>(
      'review_reply_send',
      {
        seriesId,
        replyToCommentId,
        replyToReplyId: replyToReplyId ?? null,
        text,
      },
      replyItemSchema,
    ),
  /** 删除自己的评论/剧评/回复（serviceId：2 = 剧评，4 = 评论/回复）。 */
  deleteComment: (commentId: string, serviceId: 2 | 4) =>
    call<void>('comment_delete', { commentId, serviceId }),
  /** 收藏（追剧）/ 取消收藏一部剧。 */
  seriesCollect: (seriesId: string, collect: boolean) =>
    call<void>('series_collect', { seriesId, collect }),
  /** 最近互动列表（点赞过的 vid + 收藏的剧），回显是 best-effort 匹配。 */
  state: () => call<InteractionState>('interaction_state', undefined, interactionStateSchema),
  /** 书架（我的收藏）列表，需要登录。 */
  bookshelf: () =>
    call<BookshelfEntry[]>('bookshelf_list', undefined, bookshelfEntrySchema.array()),
};
