/** 解析按钮：档案缺失/无分集时的重试出口。 */
import { useQueryClient } from '@tanstack/react-query';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { useResolveSeries } from '@/service/queries';
import { t } from '@/locales';

/** 解析按钮（档案缺失/无分集时的出口）。解析完失效本页两份缓存，就地换挡。 */
export function ResolveButton({ seriesId }: { seriesId: string }) {
  const qc = useQueryClient();
  const resolve = useResolveSeries();
  return (
    <Button
      variant="outline"
      size="sm"
      className="mx-auto"
      disabled={resolve.isPending}
      onClick={() =>
        resolve.mutate(seriesId, {
          onSuccess: () => {
            void qc.invalidateQueries({ queryKey: ['series-episodes', seriesId] });
            void qc.invalidateQueries({ queryKey: ['series-meta', seriesId] });
            toast.success(t('detail.resolved'));
          },
          onError: (e) => toast.error(e.message),
        })
      }
    >
      {t('series.resolveAgain')}
    </Button>
  );
}
