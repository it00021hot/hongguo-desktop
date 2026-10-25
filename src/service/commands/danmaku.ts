import { call } from '../tauri/invoke';

// ---------------------------------------------------------------- 弹幕

export const danmaku = {
  /**
   * 一集弹幕的**一窗**（官方 30 秒滚窗协议，随播放进度逐窗推进；medusa
   * 风控降级下没有全量拉取路线）。durationSecs 该集时长（秒，分集表
   * duration）；cursor 传上一窗响应的 nextCursor（首页空串）。 */
  window: (
    groupId: string,
    bookId: string,
    startMs: number,
    durationSecs: number,
    cursor: string,
  ) => call('danmaku_window', { groupId: groupId, bookId, startMs, durationSecs, cursor }),
  /**
   * 播放页评论区（单集维度流：group=vid/group_type=30/channel=18/
   * sort=video_innerflow，2026-10-16 官方实包形态）。一窗 10 条，
   * cursor 翻页，返回列表+总数（total 与互动栏角标同源）。 */
  comments: (groupId: string, bookId: string, cursor = '') =>
    call('comment_list', { groupId, bookId, cursor: cursor || undefined }),
  /**
   * 一条单集评论的回复列表（回复流同单集维度 src=504/ch=18；2026-10-16
   * 官方实包）。cursor 翻页（原样回传）。
   */
  commentReplies: (groupId: string, bookId: string, commentId: string, cursor = '') =>
    call('comment_replies', { groupId, bookId, commentId, cursor: cursor || undefined }),
  /** 一条剧评的回复列表（剧评维度 src=501/ch=34，同上）。 */
  reviewReplies: (seriesId: string, commentId: string, cursor = '') =>
    call('review_replies', { seriesId, commentId, cursor: cursor || undefined }),
  /** 剧级评论（详情页「剧评」：group_type=1 形态；响应 extra 带评分摘要）。 */
  seriesComments: (seriesId: string, cursor = '') =>
    call('series_comment_list', { seriesId, cursor: cursor || undefined }),
  /**
   * 发剧评（整剧维度 comment/add；需登录态）。score 为十分制评分
   * （5 星 ×2）。返回**服务端回显的完整剧评对象**（含 expand.score
   * 回显与头像昵称）——前端直接置顶插入。
   */
  seriesReviewSend: (seriesId: string, text: string, score: number) =>
    call('series_review_send', { seriesId, text, score }),

  // ------------------------------------------------------------ 创作者主页

  /** 主页资料头（详情页点创作者 → 主页；2026-10-15 官方实包）。 */
  creatorProfile: (userId: string) => call('creator_profile', { userId }),
  /** 主页作品列表一页（cursor 传上一页返回的原样游标，首页空串）。 */
  creatorWorks: (userId: string, cursor = '') =>
    call('creator_works', { userId, cursor: cursor || undefined }),
};
