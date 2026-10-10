import { call } from '../tauri/invoke';
import {
  commentItemSchema,
  commentPageSchema,
  danmakuSchema,
  replyPageSchema,
  seriesReviewPageSchema,
  type CommentItem,
  type CommentPage,
  type Danmaku,
  type ReplyPage,
  type SeriesReviewPage,
} from '../schema';

// ---------------------------------------------------------------- 弹幕

export const danmaku = {
  list: (groupId: string, bookId: string) =>
    call<Danmaku[]>('danmaku_list', { groupId: groupId, bookId }, danmakuSchema.array()),
  /** 评论区（ct=4/src=4 形态；一窗 20 条，cursor 翻页，返回列表+总数）。 */
  comments: (groupId: string, bookId: string, cursor = '') =>
    call<CommentPage>(
      'comment_list',
      { groupId, bookId, cursor: cursor || undefined },
      commentPageSchema,
    ),
  /**
   * 一条单集评论的回复列表（reply/list 独立端点，评论维度 src=504/ch=18，
   * 2026-10-10 抓 hgplayer 1.1.8 锁定）。cursor 翻页（数字串原样回传）。
   */
  commentReplies: (groupId: string, bookId: string, commentId: string, cursor = '') =>
    call<ReplyPage>(
      'comment_replies',
      { groupId, bookId, commentId, cursor: cursor || undefined },
      replyPageSchema,
    ),
  /** 一条剧评的回复列表（剧评维度 src=501/ch=34，同上）。 */
  reviewReplies: (seriesId: string, commentId: string, cursor = '') =>
    call<ReplyPage>(
      'review_replies',
      { seriesId, commentId, cursor: cursor || undefined },
      replyPageSchema,
    ),
  /** 剧级评论（详情页「剧评」：group_type=1 形态；响应 extra 带评分摘要）。 */
  seriesComments: (seriesId: string, cursor = '') =>
    call<SeriesReviewPage>(
      'series_comment_list',
      { seriesId, cursor: cursor || undefined },
      seriesReviewPageSchema,
    ),
  /**
   * 发剧评（整剧维度 comment/add；需登录态）。score 为十分制评分
   * （5 星 ×2）。返回**服务端回显的完整剧评对象**（含 expand.score
   * 回显与头像昵称）——前端直接置顶插入。
   */
  seriesReviewSend: (seriesId: string, text: string, score: number) =>
    call<CommentItem>('series_review_send', { seriesId, text, score }, commentItemSchema),
};
