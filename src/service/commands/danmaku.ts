import { call } from '../tauri/invoke';

export const danmaku = {
  /** 一集弹幕的一窗（官方 30 秒滚窗协议，随进度逐窗推进）；cursor 传上一窗 nextCursor（首页空串） */
  window: (
    groupId: string,
    bookId: string,
    startMs: number,
    durationSecs: number,
    cursor: string,
  ) => call('danmaku_window', { groupId: groupId, bookId, startMs, durationSecs, cursor }),
  /** 播放页单集维度评论区；一窗 10 条，cursor 翻页，返回列表 + total（与互动栏角标同源） */
  comments: (groupId: string, bookId: string, cursor = '') =>
    call('comment_list', { groupId, bookId, cursor: cursor || undefined }),
  /** 单集评论的回复列表（回复流同单集维度），cursor 原样回传翻页 */
  commentReplies: (groupId: string, bookId: string, commentId: string, cursor = '') =>
    call('comment_replies', { groupId, bookId, commentId, cursor: cursor || undefined }),
  /** 一条剧评的回复列表（剧评维度 src=501/ch=34，同上）。 */
  reviewReplies: (seriesId: string, commentId: string, cursor = '') =>
    call('review_replies', { seriesId, commentId, cursor: cursor || undefined }),
  /** 剧级评论（详情页「剧评」：group_type=1 形态；响应 extra 带评分摘要）。 */
  seriesComments: (seriesId: string, cursor = '') =>
    call('series_comment_list', { seriesId, cursor: cursor || undefined }),
  /** 发剧评（需登录态）；score 十分制（5 星 ×2）。返回服务端回显的完整剧评对象，前端直接置顶插入 */
  seriesReviewSend: (seriesId: string, text: string, score: number) =>
    call('series_review_send', { seriesId, text, score }),

  // 创作者主页

  creatorProfile: (userId: string) => call('creator_profile', { userId }),
  /** 主页作品列表一页（cursor 传上一页返回的原样游标，首页空串）。 */
  creatorWorks: (userId: string, cursor = '') =>
    call('creator_works', { userId, cursor: cursor || undefined }),
};
