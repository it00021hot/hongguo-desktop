import { useState } from 'react';
import { useNavigate } from '@tanstack/react-router';
import { History, Loader2, Play } from 'lucide-react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Skeleton } from '@/components/ui/skeleton';
import { isRenderableCover, useResolveSeries, useWatchHistory, useWebCover } from '@/lib/queries';
import { usePlayerStore } from '@/lib/stores/player';
import { formatDuration } from '@/lib/format';
import { t, tf } from '@/i18n';
import type { WatchHistoryItem } from '@/lib/schema';

/**
 * 「历史」——云端观看记录（官方 App「历史」同源 read_history/list，
 * book_type=2 短剧过滤；官方/第三方客户端看的记录都在，2026-10-05 实证
 * 469 条带名字带进度）。
 *
 * 点卡片跳播放器续播；封面是 HEIC 签名 URL，走 useWebCover 转码代理。
 * 本应用内观看的云端上报（read_history/update）尚未接线，暂只记本地。
 */

export function HistoryPage() {
  const navigate = useNavigate();
  const setTarget = usePlayerStore((s) => s.setTarget);
  const { data, isLoading, error, refetch } = useWatchHistory();
  const { mutate: resolve, isPending: resolving } = useResolveSeries();

  const open = (item: WatchHistoryItem) => {
    resolve(item.seriesId, {
      onSuccess: () => {
        // 云端 vid_index 0 起（第一集=0），播放器 store 是 1 起（0=无目标）
        setTarget(item.seriesId, item.vidIndex + 1);
        void navigate({ to: '/player' });
      },
      onError: (e) => toast.error(t('common.resolveFailed'), { description: e.message }),
    });
  };

  if (isLoading) {
    return (
      <div className="flex flex-col gap-3">
        {Array.from({ length: 6 }, (_, i) => (
          <Skeleton key={i} className="h-28 rounded-xl" />
        ))}
      </div>
    );
  }
  if (error) {
    return (
      <div className="text-muted-foreground flex flex-col items-center gap-3 py-16">
        <p>{t('history.loadFailed')}</p>
        <p className="text-destructive text-xs">{error.message}</p>
        <Button variant="outline" size="sm" onClick={() => void refetch()}>
          <Loader2 className="mr-1 size-4 animate-spin" aria-hidden />
          {t('feed.retry')}
        </Button>
      </div>
    );
  }
  const items = data?.items ?? [];
  if (items.length === 0) {
    return (
      <div className="text-muted-foreground flex flex-col items-center gap-2 py-16">
        <History className="size-8 opacity-40" aria-hidden />
        <p className="text-sm">{t('history.empty')}</p>
        <p className="text-xs opacity-70">{t('history.emptyHint')}</p>
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-2">
      {items.map((item) => (
        <HistoryRow
          key={`${item.seriesId}:${item.updatedAtMs}`}
          item={item}
          onOpen={() => open(item)}
        />
      ))}
      {resolving && <p className="text-muted-foreground text-sm">{t('common.resolving')}</p>}
    </div>
  );
}

/** 历史行：官方同款「看到第N集/共M集 时长」进度徽标 + 继续播放。 */
function HistoryRow({ item, onOpen }: { item: WatchHistoryItem; onOpen: () => void }) {
  const { data: webCover } = useWebCover(item.cover);
  const sourceRenderable = isRenderableCover(item.cover);
  const cover = webCover ?? (sourceRenderable ? item.cover : '');
  const [brokenFor, setBrokenFor] = useState('');
  const imgBroken = brokenFor !== '' && brokenFor === cover;
  const showImg = cover !== '' && !imgBroken;

  return (
    <article
      role="button"
      tabIndex={0}
      aria-label={item.title}
      onClick={onOpen}
      onKeyDown={(e) => {
        if (e.key !== 'Enter' && e.key !== ' ') return;
        e.preventDefault();
        onOpen();
      }}
      className="bg-card hover:border-foreground/30 focus-visible:border-foreground/30 flex cursor-pointer items-center gap-4 rounded-xl border p-3 text-left transition-colors hover:shadow-md focus-visible:outline-none"
    >
      <div className="bg-muted relative aspect-[3/4] w-[72px] shrink-0 overflow-hidden rounded-lg">
        {showImg ? (
          <img
            src={cover}
            alt={item.title}
            loading="lazy"
            className="size-full object-cover"
            onError={() => setBrokenFor(cover)}
          />
        ) : (
          <div className="text-muted-foreground grid size-full place-items-center">
            <History className="size-5" />
          </div>
        )}
      </div>
      <div className="flex min-w-0 flex-1 flex-col gap-1.5">
        <p className="truncate text-sm font-semibold" title={item.title}>
          {item.title !== '' ? item.title : t('history.unknownTitle')}
        </p>
        <div className="flex flex-wrap items-center gap-2 text-xs">
          <span className="bg-primary/10 text-primary rounded px-1.5 py-0.5 font-medium">
            {item.durationMs > 0
              ? tf('history.watching', {
                  index: item.vidIndex + 1,
                  total: item.episodeCnt > 0 ? item.episodeCnt : '–',
                  time: formatDuration(item.positionMs / 1000),
                })
              : tf('player.epShort', { index: item.vidIndex + 1 })}
          </span>
          <span className="text-muted-foreground">{formatTimeAgo(item.updatedAtMs)}</span>
        </div>
      </div>
      <Button size="sm" variant="outline" className="shrink-0" onClick={onOpen}>
        <Play className="size-4" aria-hidden />
        {t('history.continue')}
      </Button>
    </article>
  );
}

/** unix 毫秒 → 相对时间（今天/昨天/N天前，随界面语言）。 */
function formatTimeAgo(ms: number): string {
  if (ms <= 0) return '';
  const diffDays = Math.floor((Date.now() - ms) / 86_400_000);
  if (diffDays <= 0) return t('history.today');
  if (diffDays === 1) return t('history.yesterday');
  return tf('history.daysAgo', { count: diffDays });
}
