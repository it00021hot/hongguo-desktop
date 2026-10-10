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
import { cn } from '@/lib/utils';
import { BatchBar, ManageToggle, PickDot } from '@/components/batch-manage';
import { useBatchSelect } from '@/hooks/use-batch-select';
import {
  useAuthRefresh,
  useBookshelf,
  useSeriesCollect,
  useSeriesCollectBatch,
  useSeriesMeta,
  useWebCover,
} from '@/service/queries';
import { isRenderableCover } from '@/utils/cover';
import { usePlayerStore } from '@/stores/player';
import { t, tf } from '@/locales';
import type { BookshelfEntry } from '@/service/schema';

/** 内容类型筛选（对齐 hgplayer v1.1.7 收藏筛选；依据 bookshelf 条目的
 *  content_type：1=真人 1004=漫剧，0=未知不归入任何一类）。 */
type TypeTab = 'all' | 'real' | 'comic';

const TYPE_TABS: { key: TypeTab; labelKey: string }[] = [
  { key: 'all', labelKey: 'collections.filterAll' },
  { key: 'real', labelKey: 'collections.filterReal' },
  { key: 'comic', labelKey: 'collections.filterComic' },
];

function matchTypeTab(tab: TypeTab, entry: BookshelfEntry): boolean {
  if (tab === 'real') return entry.contentType === 1;
  if (tab === 'comic') return entry.contentType === 1004;
  return true;
}

export function CollectionPage() {
  const navigate = useNavigate();
  const setTarget = usePlayerStore((s) => s.setTarget);
  const refreshAuth = useAuthRefresh();
  const { data: entries, isLoading, error, refetch } = useBookshelf();
  const collect = useSeriesCollect();
  // 批量管理（hgplayer v1.1.6 同款）：多选后一请求批量取消收藏
  const batch = useBatchSelect();
  const batchCollect = useSeriesCollectBatch();

  const [query, setQuery] = useState('');
  const [typeTab, setTypeTab] = useState<TypeTab>('all');
  // 书架条目本身无标题：卡片里 resolve 到位后登记上来供过滤用
  const [titles, setTitles] = useState<Record<string, string>>({});
  const registerTitle = useCallback((id: string, title: string) => {
    setTitles((prev) => (prev[id] === title || title === '' ? prev : { ...prev, [id]: title }));
  }, []);

  const items = entries ?? [];
  const shown = items.filter(
    (entry) =>
      matchTypeTab(typeTab, entry) &&
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
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="flex items-center gap-2">
          {TYPE_TABS.map(({ key, labelKey }) => (
            <button
              key={key}
              type="button"
              onClick={() => setTypeTab(key)}
              className={cn(
                'rounded-full px-4 py-1.5 text-sm transition-colors',
                typeTab === key
                  ? 'bg-primary text-primary-foreground font-medium'
                  : 'bg-muted text-muted-foreground hover:text-foreground',
              )}
            >
              {t(labelKey)}
            </button>
          ))}
        </div>
        <ListSearch
          value={query}
          onChange={setQuery}
          placeholder={t('collections.searchPlaceholder')}
        />
        <ManageToggle managing={batch.managing} onEnter={batch.enter} onExit={batch.exit} />
      </div>
      <div className="grid grid-cols-2 gap-3 sm:grid-cols-3 lg:grid-cols-4 xl:grid-cols-5">
        {shown.map((entry) => (
          <CollectionCard
            key={entry.seriesId}
            entry={entry}
            managing={batch.managing}
            picked={batch.selected.has(entry.seriesId)}
            onPick={() => batch.toggle(entry.seriesId)}
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
      {batch.managing && (
        <BatchBar
          count={batch.selected.size}
          total={shown.length}
          deleting={batchCollect.isPending}
          deleteLabel={t('collections.remove')}
          confirmTitle={t('collections.batchRemoveTitle')}
          onToggleAll={() => batch.toggleAll(shown.map((e) => e.seriesId))}
          onDelete={() => {
            const count = batch.selected.size;
            batchCollect.mutate(
              shown
                .filter((e) => batch.selected.has(e.seriesId))
                .map((e) => ({ seriesId: e.seriesId, collect: false })),
              {
                onSuccess: () => {
                  toast.success(tf('batch.done', { count }));
                  batch.exit();
                  refreshAuth();
                },
                onError: (e: Error) => toast.error(tf('batch.partialFail', { error: e.message })),
              },
            );
          }}
        />
      )}
    </div>
  );
}

/** 收藏卡：封面 3:4 + 标题 + 收藏时间；悬浮出「取消收藏」。
 *  管理模式下 pick 在场：角标多选，点击卡=切换选中。 */
function CollectionCard({
  entry,
  managing,
  picked,
  onPick,
  onOpen,
  onRemove,
  onTitle,
}: {
  entry: BookshelfEntry;
  managing: boolean;
  picked: boolean;
  onPick: () => void;
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

  const open = () => (managing ? onPick() : onOpen());

  return (
    <div className="group relative">
      <article
        role="button"
        tabIndex={0}
        aria-label={meta?.title ?? entry.seriesId}
        onClick={open}
        onKeyDown={(e) => {
          if (e.key !== 'Enter' && e.key !== ' ') return;
          e.preventDefault();
          open();
        }}
        className={cn(
          'bg-card hover:border-foreground/30 focus-visible:border-foreground/30 cursor-pointer overflow-hidden rounded-xl border text-left transition-colors hover:shadow-md focus-visible:outline-none',
          picked && 'border-primary/60 bg-primary/5',
        )}
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
          {managing && (
            <div className="absolute top-2 left-2">
              <PickDot checked={picked} onToggle={onPick} />
            </div>
          )}
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
      {!managing && (
        <button
          type="button"
          onClick={onRemove}
          title={t('collections.remove')}
          className="absolute top-2 right-2 hidden size-7 place-items-center rounded-full bg-black/60 text-white group-hover:grid hover:bg-black/80"
        >
          <StarOff className="size-3.5" />
        </button>
      )}
    </div>
  );
}
