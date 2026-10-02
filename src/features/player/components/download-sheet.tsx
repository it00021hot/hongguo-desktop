import { useState } from 'react';
import { Download } from 'lucide-react';
import { useNavigate } from '@tanstack/react-router';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Sheet, SheetContent, SheetHeader, SheetTitle } from '@/components/ui/sheet';
import { EpisodePicker } from '@/features/series/components/episode-picker';
import { useDownloadActions } from '@/lib/queries';
import { t, tf } from '@/i18n';
import type { Episode } from '@/lib/schema';

interface Props {
  seriesId: string;
  episodes: Episode[];
  /** 打开时默认勾选当前正在播的那一集 */
  defaultSelected: number[];
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

/**
 * 播放器里的「下载到本地」。
 *
 * 与下载页共用同一个选集器：几百集的剧不可能手点，
 * 「全选 / 区间语法 / 后 30 集」这些能力必须在这里也在。
 */
export function DownloadSheet({ seriesId, episodes, defaultSelected, open, onOpenChange }: Props) {
  const navigate = useNavigate();
  const [selected, setSelected] = useState<number[]>(defaultSelected);
  const { start } = useDownloadActions();

  const handleOpen = (next: boolean) => {
    // 每次打开都回到「当前这一集」，否则会带上一次勾选的残留
    if (next) setSelected(defaultSelected);
    onOpenChange(next);
  };

  const submit = () => {
    if (selected.length === 0) return;
    start.mutate(
      { seriesId, vids: selected },
      {
        onSuccess: () => {
          toast.success(tf('download.submitting', { count: selected.length }));
          onOpenChange(false);
          void navigate({ to: '/tasks' });
        },
        onError: (e) => toast.error(e.message),
      },
    );
  };

  return (
    <Sheet open={open} onOpenChange={handleOpen}>
      <SheetContent side="bottom" className="max-h-[80vh] overflow-y-auto">
        <SheetHeader>
          <SheetTitle>{t('player.downloadToLocal')}</SheetTitle>
        </SheetHeader>
        <div className="py-3">
          <EpisodePicker episodes={episodes} selected={selected} onChange={setSelected} />
        </div>
        <div className="flex justify-end gap-2 pb-2">
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            {t('common.cancel')}
          </Button>
          <Button disabled={selected.length === 0 || start.isPending} onClick={submit}>
            <Download className="size-4" />
            {t('download.submit')}
          </Button>
        </div>
      </SheetContent>
    </Sheet>
  );
}
