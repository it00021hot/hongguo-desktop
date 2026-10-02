import { useState } from 'react';
import { useNavigate } from '@tanstack/react-router';
import { Link2, Download, Play } from 'lucide-react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Card } from '@/components/ui/card';
import { Skeleton } from '@/components/ui/skeleton';
import { EpisodePicker } from './episode-picker';
import { useDownloadActions, useResolveSeries } from '@/lib/queries';
import { usePlayerStore } from '@/lib/stores/player';
import { t, tf } from '@/i18n';
import type { Series } from '@/lib/schema';

/**
 * 下载页：粘贴链接 → 解析全集 → 选集 → 提交队列。
 *
 * 这是「剧集解析」的主入口，与浏览/搜索的卡片点选共用同一条解析链路。
 */
export function DownloadPage() {
  const navigate = useNavigate();
  const [input, setInput] = useState('');
  const [resolved, setResolved] = useState<Series | null>(null);
  const [selected, setSelected] = useState<number[]>([]);
  const setTarget = usePlayerStore((s) => s.setTarget);

  const { mutate: resolve, isPending } = useResolveSeries();
  const { start } = useDownloadActions();
  const isStarting = start.isPending;

  const handleResolve = () => {
    const value = input.trim();
    if (!value) return;
    setResolved(null);
    setSelected([]);
    resolve(value, {
      onSuccess: setResolved,
      onError: (e) => toast.error(e.message),
    });
  };

  const handleDownload = () => {
    if (!resolved || selected.length === 0) return;
    start.mutate(
      { seriesId: resolved.seriesId, vids: selected },
      {
        onSuccess: (n) => {
          toast.success(tf('download.submitting', { count: n }));
          void navigate({ to: '/tasks' });
        },
        onError: (e) => toast.error(e.message),
      },
    );
  };

  const handlePlayNow = () => {
    if (!resolved) return;
    setTarget(resolved.seriesId, selected[0] ?? 1);
    void navigate({ to: '/player' });
  };

  return (
    <div className="mx-auto flex max-w-4xl flex-col gap-6 p-6">
      <form
        className="flex flex-col gap-2"
        onSubmit={(e) => {
          e.preventDefault();
          handleResolve();
        }}
      >
        <Label htmlFor="series-input">{t('download.inputLabel')}</Label>
        <div className="flex gap-2">
          <Input
            id="series-input"
            value={input}
            onChange={(e) => setInput(e.target.value)}
            placeholder={t('download.inputPlaceholder')}
          />
          <Button type="submit" disabled={isPending || !input.trim()}>
            <Link2 className="size-4" />
            {t('download.resolve')}
          </Button>
        </div>
      </form>

      {isPending && <Skeleton className="h-64" />}

      {resolved && (
        <Card className="gap-4">
          <div>
            <h2 className="text-lg font-semibold">{resolved.title}</h2>
            <p className="text-muted-foreground text-sm">
              {tf('common.episodeCount', { count: resolved.episodes.length })}
            </p>
          </div>

          <EpisodePicker episodes={resolved.episodes} selected={selected} onChange={setSelected} />

          <div className="flex gap-2">
            <Button
              className="flex-1"
              disabled={selected.length === 0 || isStarting}
              onClick={handleDownload}
            >
              <Download className="size-4" />
              {t('download.submit')}
            </Button>
            <Button variant="secondary" className="flex-1" onClick={handlePlayNow}>
              <Play className="size-4" />
              {t('download.playNow')}
            </Button>
          </div>
        </Card>
      )}
    </div>
  );
}
