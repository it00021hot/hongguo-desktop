import { call } from '../tauri/invoke';
import {
  bookshelfEntrySchema,
  interactionStateSchema,
  type BookshelfEntry,
  type InteractionState,
} from '../schema';

// ---------------------------------------------------------------- 互动（点赞 / 收藏 / 发弹幕，官方 App API）

/** 互动操作（2026-10-05/06 抓包端点；全部要求登录态，匿名被服务端静默拒）。 */
export const interact = {
  /** 发一条弹幕（offsetMs = 视频内位置毫秒），返回服务端 comment_id。 */
  sendDanmaku: (groupId: string, bookId: string, text: string, offsetMs: number) =>
    call<string>('danmaku_send', { groupId, bookId, text, offsetMs }),
  /** 发一条评论，返回 comment_id。 */
  sendComment: (groupId: string, bookId: string, text: string) =>
    call<string>('comment_send', { groupId, bookId, text }),
  /** 回复一条评论（reply/add 独立端点）；回复「回复」时传 replyToReplyId。 */
  sendReply: (
    groupId: string,
    bookId: string,
    replyToCommentId: string,
    replyToReplyId: string | null,
    text: string,
  ) =>
    call<string>('comment_reply', {
      groupId,
      bookId,
      replyToCommentId,
      replyToReplyId: replyToReplyId ?? null,
      text,
    }),
  /** 点赞 / 取消点赞一集（vid = 分集 id）。 */
  videoDigg: (vid: string, seriesId: string, digg: boolean) =>
    call<void>('video_digg', { vid, seriesId, digg }),
  /** 点赞 / 取消点赞一条评论。 */
  commentDigg: (commentId: string, digg: boolean) =>
    call<void>('comment_digg', { commentId, digg }),
  /** 收藏（追剧）/ 取消收藏一部剧。 */
  seriesCollect: (seriesId: string, collect: boolean) =>
    call<void>('series_collect', { seriesId, collect }),
  /** 最近互动列表（点赞过的 vid + 收藏的剧），回显是 best-effort 匹配。 */
  state: () => call<InteractionState>('interaction_state', undefined, interactionStateSchema),
  /** 书架（我的收藏）列表，需要登录。 */
  bookshelf: () =>
    call<BookshelfEntry[]>('bookshelf_list', undefined, bookshelfEntrySchema.array()),
};
