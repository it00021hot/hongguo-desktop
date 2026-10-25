import { useInfiniteQuery, useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import type { InfiniteData } from '@tanstack/react-query';
import { danmaku as danmakuCmd, interact as interactCmd } from '../commands';
import type {
  CommentPage,
  CreatorProfile,
  CreatorWorksPage,
  Danmaku,
  ReplyPage,
  SeriesReviewPage,
} from '../schema';
import { keys } from './common';
import { useAccount } from './interact';

// ---------------------------------------------------------------- 弹幕与评论
// 社交态数据一律不缓存（staleTime 0，每次挂载重拉——2026-10-10 定规：
// 全站用户在产生数据，缓存 1 秒都是旧值；缓存只保留封面/视频流/登录
// 信息这类不变或重资产）。乐观写回只解决「自己操作后到重拉之间」的
// 即时反馈，数据新鲜度全靠每次打开重拉。

/** 播放键 `{groupId}:{bookId}` → 弹幕/互动接口的两个参数。 */
function parseVid(vid: string): { groupId: string; bookId: string } {
  const [groupId = '', bookId = ''] = vid.split(':');
  return { groupId, bookId };
}

/** 乐观点赞的通用缓存手术：id 命中的条目翻 userDigg 并把计数 ±1（夹 0）。
 *  评论/回复两条数据线 × 单集/剧评两侧共四份同构，差异只有缓存 key 与
 *  id 字段，2026-10-13 收敛成这一份。 */
function diggInPages<P extends { items: Array<{ userDigg: boolean; diggCount: number }> }>(
  draft: InfiniteData<P, string>,
  idOf: (item: P['items'][number]) => string,
  id: string,
  digg: boolean,
): InfiniteData<P, string> {
  return {
    ...draft,
    pages: draft.pages.map((p) => ({
      ...p,
      items: p.items.map((it) =>
        idOf(it) === id
          ? { ...it, userDigg: digg, diggCount: Math.max(0, it.diggCount + (digg ? 1 : -1)) }
          : it,
      ),
    })),
  };
}

/** 一窗弹幕（command 响应的形状；与 registry 的 danmaku_window 契约同源）。 */
export interface DanmakuWindow {
  items: Danmaku[];
  hasMore: boolean;
  nextCursor: string;
  nextStartMs: number;
}

/** 弹幕滚窗翻页参数。 */
interface DanmakuWindowParam {
  startMs: number;
  cursor: string;
}

/**
 * 一集的弹幕（按 vid，**随播放进度滚窗**）：官方 30 秒滚窗协议在 medusa
 * 风控降级下每窗 `hasMore=false`，不存在一次拉全——这里用无限查询按窗
 * 铺页（首页 startMs=当前播放位置，之后 `nextStartMs`/`nextCursor` 链式
 * 推进），由播放器在逼近已拉边界时 `fetchNextPage` 预取（官方播到哪拉
 * 到哪）。startSecs = 播放器 metadata 就绪后采样的当前播放位置（官方
 * 「播到哪从哪拉」，续播不再从 0 补拉废窗），null = 视频未就绪（查询
 * 禁用）。durationSecs 传该集时长（秒）。
 */
export function useDanmaku(vid: string, durationSecs = 0, startSecs: number | null = null) {
  const { groupId, bookId } = parseVid(vid);
  return useInfiniteQuery({
    queryKey: keys.danmaku(vid),
    queryFn: ({ pageParam }: { pageParam: DanmakuWindowParam }) =>
      danmakuCmd.window(groupId, bookId, pageParam.startMs, durationSecs, pageParam.cursor),
    initialPageParam: {
      startMs: Math.max(0, Math.floor((startSecs ?? 0) * 1000)),
      cursor: '',
    } satisfies DanmakuWindowParam,
    // 降级形态 hasMore 恒 false——推进链只由 nextCursor/nextStartMs 是否
    // 还在决定（GPUI advance_danmaku_window 同口径，hasMore 仅参考）
    getNextPageParam: (last): DanmakuWindowParam | undefined =>
      last.nextCursor && last.nextStartMs > 0
        ? { startMs: last.nextStartMs, cursor: last.nextCursor }
        : undefined,
    enabled: vid.includes(':') && startSecs !== null,
    // 播放器已有带上下文的定制 toast（「不影响播放」），全局不再重复弹
    meta: { silentOnError: true },
    staleTime: 0,
  });
}

/**
 * 发弹幕：成功后把新弹幕乐观追加进该集弹幕缓存的**最后一窗**（服务端
 * 列表有延迟，不追的话用户发完看不见自己的弹幕）。同一 vid 会因首窗
 * 起点不同带不同的 key 尾参，按 key[1]（vid）谓词匹配全部窗页缓存。
 */
export function useSendDanmaku() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: { vid: string; text: string; offsetMs: number }) => {
      const { groupId, bookId } = parseVid(input.vid);
      return interactCmd.sendDanmaku(groupId, bookId, input.text, input.offsetMs);
    },
    onSuccess: (commentId, input) => {
      queryClient.setQueriesData<InfiniteData<DanmakuWindow, DanmakuWindowParam>>(
        { predicate: (q) => q.queryKey[0] === 'danmaku' && q.queryKey[1] === input.vid },
        (prev) => {
          if (!prev) return prev;
          const pages = prev.pages.map((p, i) => {
            if (i !== prev.pages.length - 1) return p;
            return {
              ...p,
              items: [
                ...p.items,
                { commentId, text: input.text, offsetMs: input.offsetMs, diggCount: 0 },
              ].sort((a, b) => a.offsetMs - b.offsetMs),
            };
          });
          return { ...prev, pages };
        },
      );
    },
  });
}

/**
 * 播放页评论区（单集维度流：group=vid/group_type=30/channel=18/
 * sort=video_innerflow，2026-10-16 官方实包——面板整页与互动栏角标
 * 同源；剧集维度 channel=27 是沉浸流信息流面板的另一条流，只有流内
 * 评论）。无限翻页，面板底部「加载更多」续拉。
 */
export function useComments(vid: string) {
  const { groupId, bookId } = parseVid(vid);
  return useInfiniteQuery({
    queryKey: keys.comments(vid),
    queryFn: ({ pageParam }) => danmakuCmd.comments(groupId, bookId, pageParam),
    initialPageParam: '',
    getNextPageParam: (last) => (last.hasMore && last.nextCursor ? last.nextCursor : undefined),
    enabled: vid.includes(':'),
    staleTime: 0,
  });
}

/** 发评论：成功后把**服务端回显的完整评论对象**置顶插入第一页
 * （含 userId/头像/昵称——手拼条目缺这些，删除按钮出不来、样式两样），
 * 评论总数 +1。
 */
export function useSendComment() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: {
      vid: string;
      text: string;
      mentionUserId?: string;
      mentionUserName?: string;
      contextCommentIds?: string[];
    }) => {
      const { groupId, bookId } = parseVid(input.vid);
      return interactCmd.sendComment(groupId, bookId, input.text, {
        userId: input.mentionUserId ?? '',
        userName: input.mentionUserName ?? '',
        contextCommentIds: input.contextCommentIds ?? [],
      });
    },
    onSuccess: (comment, input) => {
      queryClient.setQueryData<InfiniteData<CommentPage, string>>(
        keys.comments(input.vid),
        (prev) => {
          if (!prev) return prev;
          const [first, ...rest] = prev.pages;
          if (!first) return prev;
          return {
            ...prev,
            pages: [
              {
                ...first,
                total: first.total + 1,
                items: first.items.some((c) => c.commentId === comment.commentId)
                  ? first.items
                  : [comment, ...first.items],
              },
              ...rest,
            ],
          };
        },
      );
    },
  });
}

/**
 * 回复一条评论（reply/add）。返回**服务端回显的完整回复对象**——
 * 调用方直接插进回复缓存（头像/昵称/uid/时间齐全，删除按钮立即可用）。
 */
export function useSendReply() {
  return useMutation({
    mutationFn: (input: {
      vid: string;
      replyToCommentId: string;
      replyToReplyId?: string;
      text: string;
    }) => {
      const { groupId, bookId } = parseVid(input.vid);
      return interactCmd.sendReply(
        groupId,
        bookId,
        input.replyToCommentId,
        input.replyToReplyId ?? null,
        input.text,
      );
    },
  });
}

/**
 * 一条评论的回复列表（回复流同单集维度：group=vid/group_type=30；
 * 按需拉取：`enabled` 传「是否展开」）。面板内「展开更多回复」续拉。
 */
export function useCommentReplies(vid: string, commentId: string, enabled: boolean) {
  const { groupId, bookId } = parseVid(vid);
  return useInfiniteQuery({
    queryKey: keys.commentReplies(vid, commentId),
    queryFn: ({ pageParam }) => danmakuCmd.commentReplies(groupId, bookId, commentId, pageParam),
    initialPageParam: '',
    getNextPageParam: (last) => (last.hasMore && last.nextCursor ? last.nextCursor : undefined),
    enabled: enabled && vid.includes(':') && !!commentId,
    staleTime: 0,
  });
}

/** 一条剧评的回复列表（详情页剧评区；参数语义同 [`useCommentReplies`]）。 */
export function useReviewReplies(seriesId: string, commentId: string, enabled: boolean) {
  return useInfiniteQuery({
    queryKey: keys.reviewReplies(seriesId, commentId),
    queryFn: ({ pageParam }) => danmakuCmd.reviewReplies(seriesId, commentId, pageParam),
    initialPageParam: '',
    getNextPageParam: (last) => (last.hasMore && last.nextCursor ? last.nextCursor : undefined),
    enabled: enabled && !!seriesId && !!commentId,
    staleTime: 0,
  });
}

/**
 * 单集评论点赞 / 取消（do_action object_type=8 形态）。
 *
 * 乐观写**评论列表缓存**（diggCount ±1 / userDigg），失败回滚——服务端
 * 读写即时同步（2026-10-10 CDP 实测），此前「旁路 state ±1」面板一关
 * 就丢，重开在 staleTime 内拿旧缓存会回跳（194→195 的根因）。
 */
export function useCommentDigg(vid: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: { commentId: string; digg: boolean }) =>
      interactCmd.commentDigg(input.commentId, input.digg),
    onMutate: async (input) => {
      await queryClient.cancelQueries({ queryKey: keys.comments(vid) });
      const prev = queryClient.getQueryData<InfiniteData<CommentPage, string>>(keys.comments(vid));
      if (prev) {
        queryClient.setQueryData<InfiniteData<CommentPage, string>>(
          keys.comments(vid),
          (draft) => draft && diggInPages(draft, (c) => c.commentId, input.commentId, input.digg),
        );
      }
      return { prev };
    },
    onError: (_e, _input, ctx) => {
      if (ctx?.prev) queryClient.setQueryData(keys.comments(vid), ctx.prev);
    },
  });
}

/**
 * 单集评论的**回复**点赞 / 取消——与评论点赞同端点同形态，
 * `object_id` 传 reply_id（2026-10-10 抓包实锤；CDP 实测读写即时同步）。
 *
 * 乐观写该评论的回复缓存，失败回滚。
 */
export function useReplyDigg(vid: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: { commentId: string; replyId: string; digg: boolean }) =>
      interactCmd.commentDigg(input.replyId, input.digg),
    onMutate: async (input) => {
      const key = keys.commentReplies(vid, input.commentId);
      await queryClient.cancelQueries({ queryKey: key });
      const prev = queryClient.getQueryData<InfiniteData<ReplyPage, string>>(key);
      if (prev) {
        queryClient.setQueryData<InfiniteData<ReplyPage, string>>(
          key,
          (draft) => draft && diggInPages(draft, (r) => r.replyId, input.replyId, input.digg),
        );
      }
      return { prev, key };
    },
    onError: (_e, _input, ctx) => {
      if (ctx?.prev) queryClient.setQueryData(ctx.key, ctx.prev);
    },
  });
}

/**
 * 剧级评论（详情页「剧评」tab：整部剧一条线，group_type=1 形态；
 * total 就是 tab 上的「剧评 579」计数）。与单集评论区（播放器 💬）分库。
 */
export function useSeriesComments(seriesId: string) {
  return useInfiniteQuery({
    queryKey: keys.seriesComments(seriesId),
    queryFn: ({ pageParam }) => danmakuCmd.seriesComments(seriesId, pageParam),
    initialPageParam: '',
    getNextPageParam: (last) => (last.hasMore && last.nextCursor ? last.nextCursor : undefined),
    enabled: !!seriesId,
    staleTime: 0,
  });
}

/**
 * 发剧评（详情页「剧评」评论框）。
 *
 * 成功后把**服务端回显的完整剧评对象**（expand.score 回显评分）置顶
 * 插入第一页（unshift 置顶）：服务端列表有索引延迟且排序
 * 未必把新条目放回第一页，重取会让用户「找不到自己刚发的」
 * （2026-10-10 实测反馈）。不做立即 invalidate——本地那条一直在列表
 * 头，staleTime 0 下次进入即重拉与服务器对齐。
 */
export function useSendSeriesReview(seriesId: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: { text: string; score: number }) =>
      danmakuCmd.seriesReviewSend(seriesId, input.text, input.score),
    onSuccess: (comment) => {
      queryClient.setQueryData<InfiniteData<SeriesReviewPage, string>>(
        keys.seriesComments(seriesId),
        (prev) => {
          if (!prev) return prev;
          const [first, ...rest] = prev.pages;
          if (!first) return prev;
          return {
            ...prev,
            pages: [
              {
                ...first,
                total: first.total + 1,
                items: first.items.some((c) => c.commentId === comment.commentId)
                  ? first.items
                  : [comment, ...first.items],
              },
              ...rest,
            ],
          };
        },
      );
    },
  });
}

/**
 * 剧评点赞 / 取消（comment/do_action object_type=2 形态）。
 *
 * 乐观更新剧评缓存里的 diggCount / userDigg，失败回滚并由调用方 toast。
 */
export function useReviewDigg(seriesId: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: { reviewId: string; digg: boolean }) =>
      interactCmd.reviewDigg(input.reviewId, input.digg),
    onMutate: async (input) => {
      await queryClient.cancelQueries({ queryKey: keys.seriesComments(seriesId) });
      const prev = queryClient.getQueryData<InfiniteData<CommentPage, string>>(
        keys.seriesComments(seriesId),
      );
      if (prev) {
        queryClient.setQueryData<InfiniteData<CommentPage, string>>(
          keys.seriesComments(seriesId),
          (draft) => draft && diggInPages(draft, (c) => c.commentId, input.reviewId, input.digg),
        );
      }
      return { prev };
    },
    onError: (_e, _input, ctx) => {
      if (ctx?.prev) queryClient.setQueryData(keys.seriesComments(seriesId), ctx.prev);
    },
  });
}

/**
 * 剧评的**回复**点赞 / 取消——注意**不是**剧评形态：回复点赞与评论点赞
 * 同端点（object_type=8 / comment_type=4），`object_id` 传 reply_id
 * （2026-10-10 CDP 实测对剧评回复即时生效；此前误走 review_digg 是错端点）。
 *
 * 乐观写该剧评的回复缓存，失败回滚。
 */
export function useReviewReplyDigg(seriesId: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: { commentId: string; replyId: string; digg: boolean }) =>
      interactCmd.commentDigg(input.replyId, input.digg),
    onMutate: async (input) => {
      const key = keys.reviewReplies(seriesId, input.commentId);
      await queryClient.cancelQueries({ queryKey: key });
      const prev = queryClient.getQueryData<InfiniteData<ReplyPage, string>>(key);
      if (prev) {
        queryClient.setQueryData<InfiniteData<ReplyPage, string>>(
          key,
          (draft) => draft && diggInPages(draft, (r) => r.replyId, input.replyId, input.digg),
        );
      }
      return { prev, key };
    },
    onError: (_e, _input, ctx) => {
      if (ctx?.prev) queryClient.setQueryData(ctx.key, ctx.prev);
    },
  });
}

/**
 * 回复一条剧评（或剧评的回复，reply/add 剧评形态 commit_source=13）。
 * 返回**服务端回显的完整回复对象**——调用方直接插进该剧评的回复缓存。
 */
export function useSendReviewReply(seriesId: string) {
  return useMutation({
    mutationFn: (input: { replyToCommentId: string; replyToReplyId?: string; text: string }) =>
      interactCmd.reviewReplySend(
        seriesId,
        input.replyToCommentId,
        input.replyToReplyId ?? null,
        input.text,
      ),
  });
}

/**
 * 评论 @ 提及候选（relation/list，登录态）。列表含官方 AI bot「小果」
 * （isAiBot=true）——@小果AI 快捷图标的数据源；社交态数据每次重拉。
 */
export function useMentionList(seriesId: string) {
  const { data: account } = useAccount();
  return useQuery({
    queryKey: keys.mentionList(seriesId),
    queryFn: () => interactCmd.mentionList(seriesId),
    enabled: !!account && !!seriesId,
    staleTime: 0,
  });
}

// ---------------------------------------------------------------- 创作者主页

/**
 * 创作者主页资料头（详情页点创作者 → 主页，2026-10-15 官方实包对齐）。
 * 失败交页面降级（头部行不渲染，作品列表照常）。
 */
export function useCreatorProfile(userId: string) {
  return useQuery({
    queryKey: keys.creatorProfile(userId),
    queryFn: (): Promise<CreatorProfile> => danmakuCmd.creatorProfile(userId),
    enabled: !!userId,
    staleTime: 0,
  });
}

/**
 * 创作者主页作品列表（无限翻页；一页 18 条，cursor 原样回传续拉）。
 * 作品的 seriesId 即播放目标（与追更列表同构，实测可直连 resolve）。
 */
export function useCreatorWorks(userId: string) {
  return useInfiniteQuery({
    queryKey: keys.creatorWorks(userId),
    queryFn: ({ pageParam }): Promise<CreatorWorksPage> =>
      danmakuCmd.creatorWorks(userId, pageParam),
    initialPageParam: '',
    getNextPageParam: (last) => (last.hasMore && last.cursor ? last.cursor : undefined),
    enabled: !!userId,
    staleTime: 0,
  });
}
