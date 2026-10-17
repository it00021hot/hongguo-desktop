/** 未上线剧集的预约按钮（项目标准 outline 按钮，与收藏/点赞同一语言）。 */
import { Bell, Check } from 'lucide-react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Spinner } from '@/components/ui/spinner';
import { useReservationsFillAll, useReserveSeries } from '@/service/queries';
import { t } from '@/locales';

export function ReserveButton({ seriesId }: { seriesId: string }) {
  const reserve = useReserveSeries();
  // 初始态对号预约列表（两个 tab 都查：预约态跟剧走；mutation 成功会
  // 失效列表缓存，这里随后跟上服务端真值）。成员判定要全集，FillAll 在
  // 后台把剩余页补齐（与预约页共享缓存，不重复请求）
  const offlineReservations = useReservationsFillAll(false);
  const onlineReservations = useReservationsFillAll(true);
  const reserved =
    offlineReservations.items.some((i) => i.seriesId === seriesId) ||
    onlineReservations.items.some((i) => i.seriesId === seriesId);
  return (
    <Button
      size="sm"
      variant="outline"
      className="gap-1"
      disabled={reserve.isPending}
      onClick={(e) => {
        e.stopPropagation(); // 别触发整卡跳详情
        const next = !reserved;
        reserve.mutate(
          { seriesId, reserve: next },
          {
            onSuccess: () =>
              toast.success(t(next ? 'player.interact.reserved' : 'player.interact.unreserved')),
          },
        );
      }}
    >
      {reserve.isPending ? (
        <Spinner aria-hidden />
      ) : reserved ? (
        <Check className="size-4" aria-hidden />
      ) : (
        <Bell className="size-4" aria-hidden />
      )}
      {t(reserved ? 'player.reserved' : 'player.reserve')}
    </Button>
  );
}
