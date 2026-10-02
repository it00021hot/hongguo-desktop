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

/**
 * 抽屉只消费「身份 + 展示」这几项。
 *
 * 不直接用 SeriesCard：分享链接/ID 解析出来的剧没有嗅探链接，
 * 为了满足类型去编一个空 url 只会让调用方以为它有意义。
 */
export type SeriesRef = Pick<
  SeriesCard,
  'seriesId' | 'seriesTitle' | 'cover' | 'episodeCount' | 'tags'
>;

interface Props {
  card: SeriesRef | null;
  /**
   * 勾选集号由调用方持有，不放组件内部。
   *
   * 原因：换剧和关闭抽屉都要求「选择被清空」。放组件里就得用 effect 监听
   * card 变化去重置，而本项目的 eslint 把 `set-state-in-effect` 设成了 error。
   * 状态跟着「当前打开的是哪部剧」这个对象一起换掉，两个场景一次解决。
   */
  selected: number[];
  onSelectedChange: (next: number[]) => void;
  onOpenChange: (open: boolean) => void;
}

/** 剧集详情抽屉：解析全集 → 选集 → 播放或下载。 */
export function SeriesDetailSheet({ card, selected, onSelectedChange, onOpenChange }: Props) {
  const navigate = useNavigate();
  const setTarget = usePlayerStore((s) => s.setTarget);
  const { data: series, isPending, isError, error } = useSeriesEpisodes(card?.seriesId ?? null);
  const { mutate: resolve } = useResolveSeries();
  const { start } = useDownloadActions();

  const open = card !== null;
  const episodes = series?.episodes ?? [];

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
          {/* 嗅探结果常常不带集数，为空就不渲染 description 元素 */}
          {card && card.episodeCount > 0 && (
            <SheetDescription>
              {tf('common.episodeCount', { count: card.episodeCount })}
            </SheetDescription>
          )}
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

        {/* 搜索出来的剧本地可能还没 resolve 过，取分集会失败。
            不给出口的话抽屉就是一片空白，用户以为功能坏了。 */}
        {isError && (
          <div className="grid gap-3 py-6 text-center">
            <p className="text-destructive text-sm">
              {t('series.loadFailed')}
              {error instanceof Error && `: ${error.message}`}
            </p>
            <Button variant="outline" className="mx-auto" onClick={handleResolve}>
              {t('series.resolveAgain')}
            </Button>
          </div>
        )}

        {!isPending && !isError && episodes.length === 0 && (
          <div className="grid gap-3 py-6 text-center">
            <p className="text-muted-foreground text-sm">{t('series.noEpisodes')}</p>
            <Button variant="outline" className="mx-auto" onClick={handleResolve}>
              {t('series.resolveAgain')}
            </Button>
          </div>
        )}

        {episodes.length > 0 && (
          <>
            <Separator />
            <EpisodePicker episodes={episodes} selected={selected} onChange={onSelectedChange} />
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
