import { useInfiniteQuery, useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import type { InfiniteData } from '@tanstack/react-query';
import { danmaku as danmakuCmd, interact as interactCmd } from '../commands';
import type {
  CommentPage,
  CreatorProfile,
  CreatorWorksPage,
  Danmaku,
  Emoticon,
  ReplyPage,
  SeriesReviewPage,
} from '../schema';
import { keys } from './common';
import { useAccount } from './interact';

// 社交态数据一律不缓存（staleTime 0，全站用户在产生数据，缓存 1 秒都是旧值）。

/** 播放键 `{groupId}:{bookId}` → 弹幕/互动接口的两个参数。 */
function parseVid(vid: string): { groupId: string; bookId: string } {
  const [groupId = '', bookId = ''] = vid.split(':');
  return { groupId, bookId };
}

/** 乐观点赞的通用缓存手术：id 命中的条目翻 userDigg 并把计数 ±1（夹 0）。 */
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

export interface DanmakuWindow {
  items: Danmaku[];
  hasMore: boolean;
  nextCursor: string;
  nextStartMs: number;
}

interface DanmakuWindowParam {
  startMs: number;
  cursor: string;
}

/**
 * 一集弹幕按窗铺页（官方 30 秒滚窗协议，medusa 降级下每窗 hasMore=false，不存在一次拉全），
 * 播放器逼近已拉边界时 fetchNextPage 预取。startSecs = metadata 就绪后的播放位置，null = 未就绪（查询禁用）。
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
    // 降级形态 hasMore 恒 false——推进链只由 nextCursor/nextStartMs 是否还在决定
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
 * 发弹幕：成功后把新弹幕乐观追加进该集弹幕缓存的最后一窗（服务端列表有延迟，
 * 不追的话用户发完看不见自己的弹幕）；同一 vid 因首窗起点不同带不同 key 尾参，按 key[1]（vid）谓词匹配全部窗页缓存。
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
 * 播放页评论区（单集维度流），面板整页与互动栏角标同源；无限翻页，面板底部「加载更多」续拉。
 * 剧集维度 channel=27 是沉浸流信息流面板的另一条流，只有流内评论。
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

/** 发评论：成功后把服务端回显的完整评论对象置顶插入第一页（手拼条目缺头像/昵称，删除按钮出不来），评论总数 +1。 */
export function useSendComment() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: {
      vid: string;
      text: string;
      mentionUserId?: string;
      mentionUserName?: string;
      contextCommentIds?: string[];
      /** 附图（本地图片 / 表情图片） */
      image?: Emoticon;
      /** `business_param.emoji_scene_type`（第三方 2 / 自定义 4；本地图片不带） */
      emojiSceneType?: number | null;
    }) => {
      const { groupId, bookId } = parseVid(input.vid);
      return interactCmd.sendComment(
        groupId,
        bookId,
        input.text,
        {
          userId: input.mentionUserId ?? '',
          userName: input.mentionUserName ?? '',
          contextCommentIds: input.contextCommentIds ?? [],
        },
        input.image
          ? { image: input.image, emojiSceneType: input.emojiSceneType ?? null }
          : undefined,
      );
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

/** 回复一条评论（reply/add）。返回服务端回显的完整回复对象——调用方直接插进回复缓存。 */
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

/** 一条评论的回复列表（回复流同单集维度）；enabled 传「是否展开」，面板「展开更多回复」续拉。 */
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
 * 单集评论点赞 / 取消（do_action object_type=8 形态）：乐观写评论列表缓存
 * （diggCount ±1 / userDigg），失败回滚——服务端读写即时同步。
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
 * 单集评论的回复点赞 / 取消——与评论点赞同端点同形态，object_id 传 reply_id；
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

/** 剧级评论（详情页「剧评」tab，group_type=1 形态）；total 就是 tab 上的「剧评 579」计数，与单集评论区分库。 */
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
 * 发剧评：成功后把服务端回显的完整剧评对象（expand.score 回显评分）置顶插入第一页——
 * 服务端列表有索引延迟，重取会让用户找不到刚发的；不做立即 invalidate，staleTime 0 下次进入即对齐。
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

/** 剧评点赞 / 取消（object_type=2 形态）：乐观更新剧评缓存的 diggCount / userDigg，失败回滚并由调用方 toast。 */
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
 * 剧评的回复点赞 / 取消——不是剧评形态：与评论点赞同端点（object_type=8 / comment_type=4），
 * object_id 传 reply_id（误走 review_digg 是错端点）；乐观写回复缓存，失败回滚。
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

/** 回复一条剧评（或剧评的回复，commit_source=13）；返回服务端回显的完整回复对象，调用方直接插回复缓存。 */
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

/** 评论 @ 提及候选（relation/list，登录态），含官方 AI bot「小果」（isAiBot=true）——@小果AI 快捷图标的数据源。 */
export function useMentionList(seriesId: string) {
  const { data: account } = useAccount();
  return useQuery({
    queryKey: keys.mentionList(seriesId),
    queryFn: () => interactCmd.mentionList(seriesId),
    enabled: !!account && !!seriesId,
    staleTime: 0,
  });
}

// 创作者主页

/** 创作者主页资料头；失败交页面降级（头部行不渲染，作品列表照常）。 */
export function useCreatorProfile(userId: string) {
  return useQuery({
    queryKey: keys.creatorProfile(userId),
    queryFn: (): Promise<CreatorProfile> => danmakuCmd.creatorProfile(userId),
    enabled: !!userId,
    staleTime: 0,
  });
}

/** 创作者主页作品列表（一页 18 条，cursor 原样回传续拉）；作品 seriesId 即播放目标，可直连 resolve。 */
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
