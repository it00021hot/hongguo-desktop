import { useInfiniteQuery, useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import type { InfiniteData } from '@tanstack/react-query';
import { danmaku as danmakuCmd, interact as interactCmd } from '../commands';
import type { CommentPage, Danmaku } from '../schema';
import { keys } from './common';

// ---------------------------------------------------------------- 弹幕与评论（缓存管理域）

/** 一集的弹幕（按 vid 缓存；后端已按时间轴排序并拉全窗口）。 */
export function useDanmaku(vid: string) {
  const groupId = vid.split(':')[0] ?? '';
  const bookId = vid.split(':')[1] ?? '';
  return useQuery({
    queryKey: keys.danmaku(vid),
    queryFn: () => danmakuCmd.list(groupId, bookId),
    enabled: vid.includes(':'),
    staleTime: 10 * 60_000,
  });
}

/**
 * 发弹幕：成功后把新弹幕乐观追加进该集的弹幕缓存（服务端列表有延迟，
 * 不追的话用户发完看不见自己的弹幕）。
 */
export function useSendDanmaku() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: { vid: string; text: string; offsetMs: number }) => {
      const [groupId, bookId] = input.vid.split(':');
      return interactCmd.sendDanmaku(groupId ?? '', bookId ?? '', input.text, input.offsetMs);
    },
    onSuccess: (commentId, input) => {
      queryClient.setQueryData<Danmaku[]>(keys.danmaku(input.vid), (prev) => {
        const next = prev ?? [];
        return [
          ...next,
          { commentId, text: input.text, offsetMs: input.offsetMs, diggCount: 0 },
        ].sort((a, b) => a.offsetMs - b.offsetMs);
      });
    },
  });
}

/**
 * 一集的评论区（无限翻页：第一页秒回，面板底部「加载更多」续拉；
 * total 取自第一页的 need_count 回传，失败时报错由面板显示不打断播放）。
 */
export function useComments(vid: string) {
  const groupId = vid.split(':')[0] ?? '';
  const bookId = vid.split(':')[1] ?? '';
  return useInfiniteQuery({
    queryKey: keys.comments(vid),
    queryFn: ({ pageParam }) => danmakuCmd.comments(groupId, bookId, pageParam),
    initialPageParam: '',
    getNextPageParam: (last) => (last.hasMore && last.nextCursor ? last.nextCursor : undefined),
    enabled: vid.includes(':'),
    staleTime: 60_000,
  });
}

/** 发评论：成功后乐观插入第一页顶部，评论总数 +1。 */
export function useSendComment() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: { vid: string; text: string }) => {
      const [groupId, bookId] = input.vid.split(':');
      return interactCmd.sendComment(groupId ?? '', bookId ?? '', input.text);
    },
    onSuccess: (commentId, input) => {
      // 乐观条目带上登录 uid：删除入口按 userId 判「自己的评论」
      const userId = queryClient.getQueryData<{ userId: string }>(keys.account)?.userId ?? '';
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
                items: [
                  {
                    commentId,
                    userId,
                    userName: '我',
                    avatar: '',
                    text: input.text,
                    createTime: Math.floor(Date.now() / 1000),
                    diggCount: 0,
                    replyCount: 0,
                    userDigg: false,
                    score: '',
                    scoreSuffixText: '',
                  },
                  ...first.items,
                ],
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
 * 回复一条评论（reply/add）。
 *
 * 回复**列表**自 2026-10-10 起可用（hgplayer 1.1.8 抓包锁定 reply/list
 * 独立端点，见 [`useCommentReplies`]）——发送成功后失效该评论的回复
 * 缓存，下次展开即含新回复；面板侧同时本地追加一份（服务端列表有
 * 延迟，不等重取就能看见自己那条）。
 */
export function useSendReply() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: {
      vid: string;
      replyToCommentId: string;
      replyToReplyId?: string;
      text: string;
    }) => {
      const [groupId, bookId] = input.vid.split(':');
      return interactCmd.sendReply(
        groupId ?? '',
        bookId ?? '',
        input.replyToCommentId,
        input.replyToReplyId ?? null,
        input.text,
      );
    },
    onSuccess: (_replyId, input) => {
      void queryClient.invalidateQueries({
        queryKey: keys.commentReplies(input.vid, input.replyToCommentId),
      });
    },
  });
}

/**
 * 一条单集评论的回复列表（按需拉取：`enabled` 传「是否展开」）。
 * 一页 10 条，面板内「展开更多回复」续拉（cursor 数字串原样回传）。
 */
export function useCommentReplies(vid: string, commentId: string, enabled: boolean) {
  const groupId = vid.split(':')[0] ?? '';
  const bookId = vid.split(':')[1] ?? '';
  return useInfiniteQuery({
    queryKey: keys.commentReplies(vid, commentId),
    queryFn: ({ pageParam }) => danmakuCmd.commentReplies(groupId, bookId, commentId, pageParam),
    initialPageParam: '',
    getNextPageParam: (last) => (last.hasMore && last.nextCursor ? last.nextCursor : undefined),
    enabled: enabled && vid.includes(':') && !!commentId,
    staleTime: 60_000,
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
    staleTime: 60_000,
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
    staleTime: 60_000,
  });
}

/**
 * 发剧评（详情页「剧评」评论框）。
 *
 * 成功后失效剧评缓存：第一页重取，新评论按时间排序自然置顶。
 */
export function useSendSeriesReview(seriesId: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: { text: string; score: number }) =>
      danmakuCmd.seriesReviewSend(seriesId, input.text, input.score),
    onSuccess: () =>
      void queryClient.invalidateQueries({ queryKey: keys.seriesComments(seriesId) }),
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
          (draft) =>
            draft && {
              ...draft,
              pages: draft.pages.map((p) => ({
                ...p,
                items: p.items.map((c) =>
                  c.commentId === input.reviewId
                    ? {
                        ...c,
                        userDigg: input.digg,
                        diggCount: Math.max(0, c.diggCount + (input.digg ? 1 : -1)),
                      }
                    : c,
                ),
              })),
            },
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
 * 回复一条剧评（或剧评的回复，reply/add 剧评形态 commit_source=13）。
 *
 * 成功后失效该剧评的回复缓存 + 剧评列表（replyCount 计数）。
 */
export function useSendReviewReply(seriesId: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: { replyToCommentId: string; replyToReplyId?: string; text: string }) =>
      interactCmd.reviewReplySend(
        seriesId,
        input.replyToCommentId,
        input.replyToReplyId ?? null,
        input.text,
      ),
    onSuccess: (_replyId, input) => {
      void queryClient.invalidateQueries({
        queryKey: keys.reviewReplies(seriesId, input.replyToCommentId),
      });
      void queryClient.invalidateQueries({ queryKey: keys.seriesComments(seriesId) });
    },
  });
}
