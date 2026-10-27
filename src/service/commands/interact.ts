import { call } from '../tauri/invoke';
import type { Emoticon } from '../schema';

// ---------------------------------------------------------------- 互动（点赞 / 收藏 / 发弹幕，官方 App API）

/** 互动操作（官方端点；全部要求登录态，匿名被服务端静默拒）。 */
export const interact = {
  /** 发一条弹幕（offsetMs = 视频内位置毫秒），返回服务端 comment_id。 */
  sendDanmaku: (groupId: string, bookId: string, text: string, offsetMs: number) =>
    call('danmaku_send', { groupId, bookId, text, offsetMs }),
  /** 发一条播放页评论（单集维度流，与面板同源），返回**服务端回显的
   *  完整评论对象**（直接插列表顶部）。
   *  mention 在场 = @提及评论（rich_text tp=15 +
   *  aibot_mention_context=最近评论 id；userName 是被提及者昵称，Rust 侧
   *  重拼 `@昵称\uFFFC` 前缀与段文本）。⚠️ 键名必须显式对住 registry
   *  的 mentionUserId——call 不校验入参，键名写错会被 Tauri 静默丢弃、
   *  提及降级成普通评论，勿改回展开写法）。
   *  image 在场 = 附图评论（body 顶层多 image_data；
   *  emojiSceneType 表情类才有——第三方 2 / 自定义 4，本地图片不带）。 */
  sendComment: (
    groupId: string,
    bookId: string,
    text: string,
    mention?: { userId: string; userName: string; contextCommentIds: string[] },
    image?: { image: Emoticon; emojiSceneType?: number | null },
  ) =>
    call('comment_send', {
      groupId,
      bookId,
      text,
      mentionUserId: mention?.userId,
      mentionUserName: mention?.userName,
      contextCommentIds: mention?.contextCommentIds,
      // 附图键只在有图时带（无图时不塞 null——纯文字评论的入参形态不变，
      // 回归用例锁的就是这个形状）
      ...(image ? { image: image.image, emojiSceneType: image.emojiSceneType ?? null } : {}),
    }),
  /** 评论 @ 提及候选（含官方 AI bot「小果」，isAiBot=true）。 */
  mentionList: (seriesId: string) => call('mention_list', { seriesId }),
  /** 回复一条评论（reply/add 独立端点；回复流与面板同单集维度）。
   *  回复「回复」时传 replyToReplyId。返回**服务端回显的完整回复对象**
   *  （直接插回复区）。 */
  sendReply: (
    groupId: string,
    bookId: string,
    replyToCommentId: string,
    replyToReplyId: string | null,
    text: string,
  ) =>
    call('comment_reply', {
      groupId,
      bookId,
      replyToCommentId,
      replyToReplyId: replyToReplyId ?? null,
      text,
    }),
  /** 点赞 / 取消点赞一集（vid = 分集 id）。 */
  videoDigg: (vid: string, seriesId: string, digg: boolean) =>
    call('video_digg', { vid, seriesId, digg }),
  /** 点赞 / 取消点赞一条评论（object_type=8；**回复的点赞同款形态**，
   *  object_id 传 reply_id）。 */
  commentDigg: (commentId: string, digg: boolean) => call('comment_digg', { commentId, digg }),
  /** 点赞 / 取消点赞一条剧评（object_type=2 / comment_type=2 / 空埋点，
   *  与评论点赞三处不同）。 */
  reviewDigg: (reviewId: string, digg: boolean) => call('review_digg', { reviewId, digg }),
  /** 回复一条剧评（reply/add 剧评形态 commit_source=13）；
   *  回复「回复」时传 replyToReplyId。返回服务端回显的完整回复对象。 */
  reviewReplySend: (
    seriesId: string,
    replyToCommentId: string,
    replyToReplyId: string | null,
    text: string,
  ) =>
    call('review_reply_send', {
      seriesId,
      replyToCommentId,
      replyToReplyId: replyToReplyId ?? null,
      text,
    }),
  /** 删除自己的评论/剧评/回复（serviceId：2 = 剧评，4 = 评论/回复）。 */
  deleteComment: (commentId: string, serviceId: 2 | 4) =>
    call('comment_delete', { commentId, serviceId }),
  /** 收藏（追剧）/ 取消收藏一部剧。 */
  seriesCollect: (seriesId: string, collect: boolean) =>
    call('series_collect', { seriesId, collect }),
  /** 批量收藏/取消收藏（一请求多条目）。 */
  seriesCollectBatch: (entries: { seriesId: string; collect: boolean }[]) =>
    call('series_collect_batch', { entries }),
  /** 最近互动列表（点赞过的 vid + 收藏的剧），回显是 best-effort 匹配。 */
  state: () => call('interaction_state'),
  /** 书架（我的收藏）列表，需要登录。 */
  bookshelf: () => call('bookshelf_list'),
};
