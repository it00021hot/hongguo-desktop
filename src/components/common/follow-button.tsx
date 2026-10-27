import { Tv } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { useFollowState } from '@/hooks/use-follow-state';
import { t } from '@/locales';
import { cn } from '@/lib/utils';

/**
 * 追更按钮（对齐官方客户端的追更按钮语义）：详情页/列表挂同款。
 * 状态与乐观逻辑收在 use-follow-state，播放页互动栏是同一份。
 */
export function FollowButton({
  seriesId,
  size = 'sm',
  className,
}: {
  seriesId: string;
  size?: 'sm' | 'default';
  className?: string;
}) {
  const { isOn, pending, set } = useFollowState(seriesId);
  return (
    <Button
      variant="outline"
      size={size}
      onClick={() => set(!isOn)}
      disabled={pending}
      title={isOn ? t('follow.unfollowTip') : t('follow.followTip')}
      className={cn(isOn && 'border-primary text-primary', className)}
    >
      <Tv className="size-4" />
      {isOn ? t('follow.following') : t('follow.follow')}
    </Button>
  );
}
