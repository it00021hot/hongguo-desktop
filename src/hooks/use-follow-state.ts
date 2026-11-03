import { useState } from 'react';
import { toast } from 'sonner';
import { useAccount, useFollowToggle, useIsFollowing } from '@/service/queries';
import { t } from '@/locales';

/**
 * 追更状态共享逻辑（详情页 FollowButton 与播放页互动栏同源）：乐观翻转失败回滚并弹服务端原文；
 * 未登录时 `set()` 弹提示并返回 false（登录墙，对齐官方客户端）。
 */
export function useFollowState(seriesId: string) {
  const { data: account } = useAccount();
  // staleTime 0 的查询按 seriesId 缓存：翻集/翻剧时 key 变化自然重查
  const { data: following } = useIsFollowing(seriesId);
  const toggle = useFollowToggle();
  const [optimistic, setOptimistic] = useState<{ seriesId: string; value: boolean } | null>(null);

  const isOn =
    (optimistic && optimistic.seriesId === seriesId ? optimistic.value : null) ??
    following ??
    false;

  const set = (next: boolean): boolean => {
    if (!account) {
      toast.error(t('follow.loginRequired'));
      return false;
    }
    setOptimistic({ seriesId, value: next });
    toggle.mutate(
      { seriesId, follow: next },
      {
        onSuccess: () => toast.success(t(next ? 'follow.followed' : 'follow.unfollowed')),
        onError: (e: Error) => {
          setOptimistic(null);
          toast.error(e.message);
        },
      },
    );
    return true;
  };

  return { isOn, pending: toggle.isPending, set };
}
