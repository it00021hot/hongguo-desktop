import { useState } from 'react';
import { useNavigate } from '@tanstack/react-router';
import { Play, Download } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import { Separator } from '@/components/ui/separator';
import { Skeleton } from '@/components/ui/skeleton';
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
} from '@/components/ui/sheet';
import { EpisodePicker } from './episode-picker';
import { useDownloadActions, useResolveSeries, useSeriesEpisodes } from '@/lib/queries';
import { usePlayerStore } from '@/lib/stores/player';
import { toast } from 'sonner';
import { t, tf } from '@/i18n';
import type { SeriesCard } from '@/lib/schema';

interface Props {
  card: SeriesCard | null;
  onOpenChange: (open: boolean) => void;
}

/** 剧集详情抽屉：解析全集 → 选集 → 播放或下载。 */
export function SeriesDetailSheet({ card, onOpenChange }: Props) {
  const navigate = useNavigate();
  const [selected, setSelected] = useState<number[]>([]);
  const setTarget = usePlayerStore((s) => s.setTarget);
  const { data: series, isPending } = useSeriesEpisodes(card?.seriesId ?? null);
  const { mutate: resolve } = useResolveSeries();
  const { start } = useDownloadActions();

  const open = card !== null;

  const handleResolve = () => {
    if (!card) return;
    resolve(card.seriesId, {
      onSuccess: (s) => {
        setTarget(s.seriesId, 1);
        onOpenChange(false);
        void navigate({ to: '/player' });
      },
      onError: (e) => toast.error(e.message),
    });
  };

  const handleDownload = () => {
    if (!card || selected.length === 0) return;
    start.mutate(
      { seriesId: card.seriesId, vids: selected },
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
    <Sheet open={open} onOpenChange={onOpenChange}>
      <SheetContent side="right" className="w-full sm:max-w-lg">
        <SheetHeader>
          <SheetTitle>{card?.seriesTitle ?? ''}</SheetTitle>
          <SheetDescription>
            {card && card.episodeCount > 0 ? `${card.episodeCount} 集` : ''}
          </SheetDescription>
        </SheetHeader>

        {card && card.tags.length > 0 && (
          <div className="flex flex-wrap gap-1">
            {card.tags.map((tag) => (
              <Badge key={tag} variant="outline">
                {tag}
              </Badge>
            ))}
          </div>
        )}

        {isPending && <Skeleton className="h-40" />}

        {series && series.episodes.length > 0 && (
          <>
            <Separator />
            <EpisodePicker
              episodes={series.episodes}
              selected={selected}
              onChange={setSelected}
            />
          </>
        )}

        <div className="mt-auto flex gap-2">
          <Button className="flex-1" onClick={handleResolve}>
            <Play className="size-4" />
            {t('download.playNow')}
          </Button>
          <Button
            className="flex-1"
            variant="secondary"
            disabled={selected.length === 0}
            onClick={handleDownload}
          >
            <Download className="size-4" />
            {t('download.submit')}
          </Button>
        </div>
      </SheetContent>
    </Sheet>
  );
}
