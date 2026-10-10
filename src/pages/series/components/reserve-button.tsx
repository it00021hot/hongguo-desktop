/** 未上线剧集的预约按钮（项目标准 outline 按钮，与收藏/点赞同一语言）。 */
import { Bell, Check, Loader2 } from 'lucide-react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { useReservations, useReserveSeries } from '@/service/queries';
import { t } from '@/locales';

export function ReserveButton({ seriesId }: { seriesId: string }) {
  const reserve = useReserveSeries();
  // 初始态对号预约列表（两个 tab 都查：预约态跟剧走；mutation 成功会
  // 失效列表缓存，这里随后跟上服务端真值）
  const { data: offlineReservations } = useReservations(false);
  const { data: onlineReservations } = useReservations(true);
  const reserved =
    (offlineReservations?.items.some((i) => i.seriesId === seriesId) ?? false) ||
    (onlineReservations?.items.some((i) => i.seriesId === seriesId) ?? false);
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
            onError: (err) => toast.error(String(err)),
          },
        );
      }}
    >
      {reserve.isPending ? (
        <Loader2 className="size-4 animate-spin" aria-hidden />
      ) : reserved ? (
        <Check className="size-4" aria-hidden />
      ) : (
        <Bell className="size-4" aria-hidden />
      )}
      {t(reserved ? 'player.reserved' : 'player.reserve')}
    </Button>
  );
}
