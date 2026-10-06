//! 「点赞」页：我赞过的视频（ugc/action/mget 最近互动列表过滤 user_digg）。
//!
//! 服务端没有「点赞列表」专用接口（2026-10-06 抓包确认 hgplayer 同样用
//! mget 承载），条目带剧标题与点赞计数；超过 100 条的旧互动不在此列表
//! （mget 上限，与 hgplayer 口径一致）。点击进播放器，支持就地取消赞。

import { useState } from 'react';
import { useNavigate } from '@tanstack/react-router';
import { Heart, HeartOff, Loader2 } from 'lucide-react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Skeleton } from '@/components/ui/skeleton';
import {
  isRenderableCover,
  useInteractionState,
  useSeriesMeta,
  useVideoDigg,
  useWebCover,
} from '@/lib/queries';
import { usePlayerStore } from '@/lib/stores/player';
import { t, tf } from '@/i18n';
import type { InteractionItem } from '@/lib/schema';

export function LikedPage() {
  const navigate = useNavigate();
  const setTarget = usePlayerStore((s) => s.setTarget);
  const { data: state, isLoading, error, refetch } = useInteractionState();
  const digg = useVideoDigg();

  const liked = (state?.items ?? []).filter((i) => i.userDigg);

  const open = (item: InteractionItem) => {
    setTarget(item.seriesId, 1);
    void navigate({ to: '/player' });
  };

  if (isLoading) {
    return (
      <div className="flex flex-col gap-3">
        {Array.from({ length: 6 }, (_, i) => (
          <Skeleton key={i} className="h-20 rounded-xl" />
        ))}
      </div>
    );
  }
  if (error) {
    return (
      <div className="text-muted-foreground flex flex-col items-center gap-3 py-16">
        <p>{t('liked.loadFailed')}</p>
        <p className="text-destructive text-xs">{error.message}</p>
        <Button variant="outline" size="sm" onClick={() => void refetch()}>
          <Loader2 className="mr-1 size-4 animate-spin" aria-hidden />
          {t('feed.retry')}
        </Button>
      </div>
    );
  }
  if (liked.length === 0) {
    return (
      <div className="text-muted-foreground flex flex-col items-center gap-2 py-16">
        <Heart className="size-8 opacity-40" aria-hidden />
        <p className="text-sm">{t('liked.empty')}</p>
        <p className="text-xs opacity-70">{t('liked.emptyHint')}</p>
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-2">
      <p className="text-muted-foreground text-xs">{t('liked.recentOnly')}</p>
      {liked.map((item) => (
        <LikedRow
          key={`${item.vid}:${item.seriesId}`}
          item={item}
          onOpen={() => open(item)}
          onUndo={() =>
            digg.mutate(
              { vid: item.vid, seriesId: item.seriesId, digg: false },
              { onError: (e) => toast.error(String(e)) },
            )
          }
        />
      ))}
    </div>
  );
}

/** 点赞行：同「历史」页横排形态，标题/封面来自剧集档案（缺失时回落解析）。 */
function LikedRow({
  item,
  onOpen,
  onUndo,
}: {
  item: InteractionItem;
  onOpen: () => void;
  onUndo: () => void;
}) {
  const { data: meta } = useSeriesMeta(item.seriesId);
  const rawCover = meta?.cover ?? '';
  const { data: webCover } = useWebCover(item.seriesId, rawCover);
  const cover = webCover ?? (isRenderableCover(rawCover) ? rawCover : '');
  const [brokenFor, setBrokenFor] = useState('');
  const imgBroken = brokenFor !== '' && brokenFor === cover;
  const showImg = cover !== '' && !imgBroken;
  const title = item.seriesTitle || meta?.title || '';

  return (
    <article
      role="button"
      tabIndex={0}
      aria-label={title}
      onClick={onOpen}
      onKeyDown={(e) => {
        if (e.key !== 'Enter' && e.key !== ' ') return;
        e.preventDefault();
        onOpen();
      }}
      className="bg-card hover:border-foreground/30 focus-visible:border-foreground/30 flex cursor-pointer items-center gap-4 rounded-xl border p-3 text-left transition-colors hover:shadow-md focus-visible:outline-none"
    >
      <div className="bg-muted relative aspect-[3/4] w-[56px] shrink-0 overflow-hidden rounded-lg">
        {showImg ? (
          <img
            src={cover}
            alt={title}
            loading="lazy"
            className="size-full object-cover"
            onError={() => setBrokenFor(cover)}
          />
        ) : (
          <div className="text-muted-foreground grid size-full place-items-center">
            <Heart className="size-4" />
          </div>
        )}
      </div>
      <div className="flex min-w-0 flex-1 flex-col gap-1">
        <p className="truncate text-sm font-semibold" title={title}>
          {title !== '' ? title : tf('collections.unresolved', { id: item.seriesId.slice(-6) })}
        </p>
        <div className="text-muted-foreground flex items-center gap-2 text-xs">
          <Heart className="size-3.5 fill-red-400 text-red-400" aria-hidden />
          <span className="tabular-nums">{item.diggedCount > 0 ? item.diggedCount : ''}</span>
        </div>
      </div>
      <Button
        size="sm"
        variant="ghost"
        className="shrink-0"
        onClick={(e) => {
          e.stopPropagation();
          onUndo();
        }}
      >
        <HeartOff className="size-4" aria-hidden />
        {t('liked.undo')}
      </Button>
    </article>
  );
}
