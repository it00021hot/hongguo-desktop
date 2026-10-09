//! 「收藏」页：书架列表（bookshelf/video/list，2026-10-06 抓包锁定）。
//!
//! 服务端条目只有 series_id + 收藏时间——标题封面走本地档案缓存，
//! 未收录的自动回落 resolve_series；卡片同「历史」页形态，点击进播放器。
//! 支持就地取消收藏（bookshelf update operate_type=1）。
//! 标题搜索框对齐参考端 v1.1.6（客户端过滤，标题异步到位后登记）。

import { useCallback, useEffect, useState } from 'react';
import { useNavigate } from '@tanstack/react-router';
import { Loader2, Play, Star, StarOff } from 'lucide-react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { ListSearch } from '@/components/list-search';
import { matchListQuery } from '@/utils/list-filter';
import { Skeleton } from '@/components/ui/skeleton';
import {
  useAuthRefresh,
  useBookshelf,
  useSeriesCollect,
  useSeriesMeta,
  useWebCover,
} from '@/service/queries';
import { isRenderableCover } from '@/utils/cover';
import { usePlayerStore } from '@/stores/player';
import { t, tf } from '@/locales';
import type { BookshelfEntry } from '@/service/schema';

export function CollectionPage() {
  const navigate = useNavigate();
  const setTarget = usePlayerStore((s) => s.setTarget);
  const refreshAuth = useAuthRefresh();
  const { data: entries, isLoading, error, refetch } = useBookshelf();
  const collect = useSeriesCollect();

  const [query, setQuery] = useState('');
  // 书架条目本身无标题：卡片里 resolve 到位后登记上来供过滤用
  const [titles, setTitles] = useState<Record<string, string>>({});
  const registerTitle = useCallback((id: string, title: string) => {
    setTitles((prev) => (prev[id] === title || title === '' ? prev : { ...prev, [id]: title }));
  }, []);

  const items = entries ?? [];
  const shown = items.filter((entry) =>
    matchListQuery(query, titles[entry.seriesId] ?? '', entry.seriesId),
  );

  const open = (seriesId: string) => {
    setTarget(seriesId, 1);
    void navigate({ to: '/player' });
  };

  if (isLoading) {
    return (
      <div className="grid grid-cols-2 gap-3 p-4 sm:grid-cols-3 lg:grid-cols-4 xl:grid-cols-5">
        {Array.from({ length: 10 }, (_, i) => (
          <Skeleton key={i} className="aspect-[3/4] rounded-xl" />
        ))}
      </div>
    );
  }
  if (error) {
    const notLoggedIn = error.message.includes('未登录');
    if (notLoggedIn) {
      return (
        <div className="text-muted-foreground flex flex-col items-center gap-3 p-4 py-16">
          <Star className="size-8 opacity-40" aria-hidden />
          <p className="text-sm">{t('collections.loginRequired')}</p>
          <Button variant="outline" size="sm" onClick={() => void refetch()}>
            <Loader2 className="mr-1 size-4 animate-spin" aria-hidden />
            {t('feed.retry')}
          </Button>
        </div>
      );
    }
    return (
      <div className="text-muted-foreground flex flex-col items-center gap-3 p-4 py-16">
        <p>{t('collections.loadFailed')}</p>
        <p className="text-destructive text-xs">{error.message}</p>
        <Button variant="outline" size="sm" onClick={() => void refetch()}>
          {t('feed.retry')}
        </Button>
      </div>
    );
  }

  if (items.length === 0) {
    return (
      <div className="text-muted-foreground flex flex-col items-center gap-2 p-4 py-16">
        <Star className="size-8 opacity-40" aria-hidden />
        <p className="text-sm">{t('collections.empty')}</p>
        <p className="text-xs opacity-70">{t('collections.emptyHint')}</p>
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-3 p-4">
      <div className="flex justify-end">
        <ListSearch
          value={query}
          onChange={setQuery}
          placeholder={t('collections.searchPlaceholder')}
        />
      </div>
      <div className="grid grid-cols-2 gap-3 sm:grid-cols-3 lg:grid-cols-4 xl:grid-cols-5">
        {shown.map((entry) => (
          <CollectionCard
            key={entry.seriesId}
            entry={entry}
            onOpen={() => open(entry.seriesId)}
            onTitle={registerTitle}
            onRemove={() => {
              collect.mutate(
                { seriesId: entry.seriesId, collect: false },
                {
                  onSuccess: () => {
                    toast.success(t('collections.removed'));
                    refreshAuth();
                  },
                  onError: (e) => toast.error(String(e)),
                },
              );
            }}
          />
        ))}
      </div>
    </div>
  );
}

/** 收藏卡：封面 3:4 + 标题 + 收藏时间；悬浮出「取消收藏」。 */
function CollectionCard({
  entry,
  onOpen,
  onRemove,
  onTitle,
}: {
  entry: BookshelfEntry;
  onOpen: () => void;
  onRemove: () => void;
  onTitle: (id: string, title: string) => void;
}) {
  const { data: meta } = useSeriesMeta(entry.seriesId);
  const rawCover = meta?.cover ?? '';
  const { data: webCover } = useWebCover(rawCover);
  const cover = webCover ?? (isRenderableCover(rawCover) ? rawCover : '');
  const [brokenFor, setBrokenFor] = useState('');
  const imgBroken = brokenFor !== '' && brokenFor === cover;
  const showImg = cover !== '' && !imgBroken;
  const title = meta?.title ?? '';

  // 标题到位就登记给页面级过滤（本地无档案、靠 resolve 补的条目）
  useEffect(() => {
    if (title !== '') onTitle(entry.seriesId, title);
  }, [entry.seriesId, title, onTitle]);

  return (
    <div className="group relative">
      <article
        role="button"
        tabIndex={0}
        aria-label={meta?.title ?? entry.seriesId}
        onClick={onOpen}
        onKeyDown={(e) => {
          if (e.key !== 'Enter' && e.key !== ' ') return;
          e.preventDefault();
          onOpen();
        }}
        className="bg-card hover:border-foreground/30 focus-visible:border-foreground/30 cursor-pointer overflow-hidden rounded-xl border text-left transition-colors hover:shadow-md focus-visible:outline-none"
      >
        <div className="bg-muted relative aspect-[3/4]">
          {showImg ? (
            <img
              src={cover}
              alt={meta?.title ?? ''}
              loading="lazy"
              className="size-full object-cover"
              onError={() => setBrokenFor(cover)}
            />
          ) : (
            <div className="text-muted-foreground grid size-full place-items-center">
              <Star className="size-5" />
            </div>
          )}
          <div className="absolute inset-0 bg-black/0 transition-colors group-hover:bg-black/30" />
          <div className="absolute inset-0 hidden place-items-center group-hover:grid">
            <span className="bg-primary text-primary-foreground inline-flex items-center gap-1.5 rounded-full px-3 py-1.5 text-xs font-medium">
              <Play className="size-3.5" aria-hidden />
              {t('history.continue')}
            </span>
          </div>
        </div>
        <div className="flex flex-col gap-1 p-2.5">
          <p className="truncate text-sm font-semibold" title={meta?.title}>
            {meta?.title !== '' && meta?.title != null
              ? meta.title
              : tf('collections.unresolved', { id: entry.seriesId.slice(-6) })}
          </p>
          {meta?.episodeCount ? (
            <p className="text-muted-foreground text-xs">
              {tf('collections.episodeCount', { n: meta.episodeCount })}
            </p>
          ) : (
            <p className="text-muted-foreground text-xs">{t('collections.resolvingHint')}</p>
          )}
        </div>
      </article>
      <button
        type="button"
        onClick={onRemove}
        title={t('collections.remove')}
        className="absolute top-2 right-2 hidden size-7 place-items-center rounded-full bg-black/60 text-white group-hover:grid hover:bg-black/80"
      >
        <StarOff className="size-3.5" />
      </button>
    </div>
  );
}
