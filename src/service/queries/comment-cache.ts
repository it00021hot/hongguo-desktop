/**
 * 评论/回复缓存的乐观手术（单一实现）：发送与删除在播放器评论区与
 * 详情页剧评区两处逐行同构——只有缓存 key 不同，2026-10-13 收敛到这里。
 * 计数与摘除必须成对（发送 +1 / 删除 -1），此前各写一份时漏过计数，
 * 「展开 N 条」与实际条数对不上。
 */
import type { InfiniteData, QueryClient } from '@tanstack/react-query';
import type { CommentPage, ReplyItem, ReplyPage } from '@/service/schema';

/** 服务端回显的完整回复对象进回复缓存（头像/昵称/uid/时间齐全：
 *  新回复以普通条目追加首页并去重，total +1；缓存未建时给单页初形
 *  ——面板没展开过就先回复的路径）。 */
export function appendReply(
  queryClient: QueryClient,
  replyKey: readonly unknown[],
  reply: ReplyItem,
): void {
  queryClient.setQueryData<InfiniteData<ReplyPage, string>>(replyKey, (prev) => {
    if (!prev) {
      return {
        pages: [{ items: [reply], total: 1, hasMore: false, nextCursor: '' }],
        pageParams: [''],
      };
    }
    const [first, ...rest] = prev.pages;
    if (!first) return prev;
    return {
      ...prev,
      pages: [
        {
          ...first,
          items: first.items.some((r) => r.replyId === reply.replyId)
            ? first.items
            : [...first.items, reply],
          total: first.total + 1,
        },
        ...rest,
      ],
    };
  });
}

/** 回复摘除（本地删除免重取）：首页 total - 1 + 条目过滤。 */
export function removeReply(
  queryClient: QueryClient,
  replyKey: readonly unknown[],
  replyId: string,
): void {
  queryClient.setQueryData<InfiniteData<ReplyPage, string>>(
    replyKey,
    (prev) =>
      prev && {
        ...prev,
        pages: prev.pages.map((p, i) => ({
          ...p,
          total: i === 0 ? Math.max(0, p.total - 1) : p.total,
          items: p.items.filter((r) => r.replyId !== replyId),
        })),
      },
  );
}

/** 父评论/剧评的回复计数 ±1（发送 +1 / 删除 -1，夹 0）。 */
export function bumpReplyCount(
  queryClient: QueryClient,
  commentsKey: readonly unknown[],
  commentId: string,
  delta: 1 | -1,
): void {
  queryClient.setQueryData<InfiniteData<CommentPage, string>>(
    commentsKey,
    (prev) =>
      prev && {
        ...prev,
        pages: prev.pages.map((p) => ({
          ...p,
          items: p.items.map((c) =>
            c.commentId === commentId ? { ...c, replyCount: Math.max(0, c.replyCount + delta) } : c,
          ),
        })),
      },
  );
}

/** 评论/剧评本体摘除：首页 total - 1 + 条目过滤。 */
export function removeComment(
  queryClient: QueryClient,
  commentsKey: readonly unknown[],
  commentId: string,
): void {
  queryClient.setQueryData<InfiniteData<CommentPage, string>>(
    commentsKey,
    (prev) =>
      prev && {
        ...prev,
        pages: prev.pages.map((p, i) => ({
          ...p,
          total: i === 0 ? Math.max(0, p.total - 1) : p.total,
          items: p.items.filter((c) => c.commentId !== commentId),
        })),
      },
  );
}
