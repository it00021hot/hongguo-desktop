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
                    userName: '我',
                    avatar: '',
                    text: input.text,
                    createTime: Math.floor(Date.now() / 1000),
                    diggCount: 0,
                    replyCount: 0,
                    userDigg: false,
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
 * 回复一条评论（reply/add）。回复**列表**服务端暂无拉取接口
 * （2026-10-06 probe 实证：reply/list 对 aid 8662 无 handler，hgplayer
 * 同样不拉）——自己发的回复由评论面板本地追加展示。
 */
export function useSendReply() {
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
    mutationFn: (text: string) => danmakuCmd.seriesReviewSend(seriesId, text),
    onSuccess: () =>
      void queryClient.invalidateQueries({ queryKey: keys.seriesComments(seriesId) }),
  });
}
