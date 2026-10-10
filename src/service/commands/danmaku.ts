import { call } from '../tauri/invoke';
import {
  commentPageSchema,
  danmakuSchema,
  seriesReviewPageSchema,
  type CommentPage,
  type Danmaku,
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
  /** 剧级评论（详情页「剧评」：group_type=1 形态；响应 extra 带评分摘要）。 */
  seriesComments: (seriesId: string, cursor = '') =>
    call<SeriesReviewPage>(
      'series_comment_list',
      { seriesId, cursor: cursor || undefined },
      seriesReviewPageSchema,
    ),
  /**
   * 发剧评（整剧维度 comment/add；需登录态）。score 为十分制评分
   * （5 星 ×2，hgplayer 1.1.6 抓包：评分随发送走 business_param.score）。
   * 返回新评论 id。
   */
  seriesReviewSend: (seriesId: string, text: string, score: number) =>
    call<string>('series_review_send', { seriesId, text, score }),
};
