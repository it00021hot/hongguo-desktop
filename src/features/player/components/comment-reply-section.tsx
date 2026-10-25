//! 沉浸流评论面板的回复区：一组件一文件，从 comment-panel 拆出。

import { ReplyThread } from '@/components/common/reply-thread';
import { useCommentReplies } from '@/service/queries';
import { dedupBy } from '@/service/queries/common';
import type { ReplyItem } from '@/service/schema';

/** 播放器皮肤的回复线程：解包评论回复查询，交给共享 ReplyThread。
 *  皮肤与 i18n 键位差异收敛在 `components/common/reply-thread`（阶段 3
 *  「双实现收敛」；页侧剧评同源）。原地内联展开，展开才
 *  拉首页；开合受控于父组件——回复发送成功后强制展开，让本地追加的那条
 *  立刻可见（折叠时发回复 = 「没效果」的根源）。 */
export function ReplySection({
  vid,
  commentId,
  replyCount,
  myUserId,
  open,
  onToggle,
  onDigg,
  onReplyTo,
  onDeleteReply,
  pendingAi = false,
  aiName,
}: {
  vid: string;
  commentId: string;
  replyCount: number;
  myUserId: string;
  open: boolean;
  onToggle: () => void;
  onDigg: (id: string, digg: boolean) => void;
  onReplyTo: (reply: ReplyItem) => void;
  onDeleteReply: (reply: ReplyItem) => void;
  /** @小果AI 提及评论的「思考中…」占位（发送成功即显示，回复到达后撤除） */
  pendingAi?: boolean;
  /** AI bot 用户名（回复作者的渐变 Ai 后缀 + AI 生成标记） */
  aiName?: string;
}) {
  const q = useCommentReplies(vid, commentId, open);
  const replies = dedupBy(q.data?.pages.flatMap((p) => p.items) ?? [], (r) => r.replyId);
  return (
    <ReplyThread
      variant="player"
      replyCount={replyCount}
      myUserId={myUserId}
      open={open}
      onToggle={onToggle}
      replies={replies}
      isPending={q.isPending}
      error={q.error}
      refetch={() => void q.refetch()}
      hasNextPage={q.hasNextPage}
      isFetchingNextPage={q.isFetchingNextPage}
      fetchNextPage={() => void q.fetchNextPage()}
      pendingAi={pendingAi}
      aiName={aiName}
      onDigg={onDigg}
      onReplyTo={onReplyTo}
      onDeleteReply={onDeleteReply}
    />
  );
}
