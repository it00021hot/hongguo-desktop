//! 「点赞」页：我赞过的视频（ugc/action/mget 最近互动列表过滤 user_digg）。
//!
//! 服务端没有「点赞列表」专用接口（2026-10-06 抓包确认 hgplayer 同样用
//! mget 承载），条目带剧标题与点赞计数；超过 100 条的旧互动不在此列表
//! （mget 上限，与 hgplayer 口径一致）。卡片同「收藏」页网格形态
//! （封面 3:4 + 悬浮续播/取消赞）；标题搜索框对齐参考端 v1.1.6。

import { useCallback, useEffect, useState } from 'react';
import { useNavigate } from '@tanstack/react-router';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { Heart, HeartOff, Loader2, Play } from 'lucide-react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { ListSearch } from '@/components/list-search';
import { matchListQuery } from '@/utils/list-filter';
import { Skeleton } from '@/components/ui/skeleton';
import { cn } from '@/lib/utils';
import { BatchBar, ManageToggle, PickDot } from '@/components/batch-manage';
import { useBatchSelect } from '@/hooks/use-batch-select';
import { interact as interactCmd } from '@/service/commands';
import { keys } from '@/service/queries/common';
import { useInteractionState, useSeriesMeta, useVideoDigg, useWebCover } from '@/service/queries';
import { isRenderableCover } from '@/utils/cover';
import { usePlayerStore } from '@/stores/player';
import { t, tf } from '@/locales';
import type { InteractionItem } from '@/service/schema';

export function LikedPage() {
  const navigate = useNavigate();
  const setTarget = usePlayerStore((s) => s.setTarget);
  const queryClient = useQueryClient();
  const { data: state, isLoading, error, refetch } = useInteractionState();
  const digg = useVideoDigg();
  // 批量管理（hgplayer v1.1.6 同款）：多选后逐条取消赞（官方无批量点赞
  // 端点，do_action 单条语义；全跑完统一回显 + 汇总失败数）
  const batch = useBatchSelect();
  const batchUndo = useMutation({
    mutationFn: async (items: InteractionItem[]) => {
      const results = await Promise.allSettled(
        items.map((i) => interactCmd.videoDigg(i.vid, i.seriesId, false)),
      );
      const failed = results.filter((r) => r.status === 'rejected').length;
      if (failed > 0) throw new Error(`${failed}/${items.length}`);
      return items.length;
    },
    onSuccess: (n) => {
      toast.success(tf('batch.done', { count: n }));
      batch.exit();
      void queryClient.invalidateQueries({ queryKey: keys.interactState });
    },
    onError: (e: Error) => toast.error(tf('batch.partialFail', { error: e.message })),
  });

  const [query, setQuery] = useState('');
  // mget 自带 seriesTitle，但缺失时卡片会回落 resolve——标题异步到位后
  // 登记上来供过滤用（卡片卸载后登记值保留，不影响筛选）
  const [titles, setTitles] = useState<Record<string, string>>({});
  const registerTitle = useCallback((id: string, title: string) => {
    setTitles((prev) => (prev[id] === title || title === '' ? prev : { ...prev, [id]: title }));
  }, []);

  const liked = (state?.items ?? []).filter((i) => i.userDigg);
  const shown = liked.filter((i) =>
    matchListQuery(query, i.seriesTitle || titles[i.seriesId] || '', i.seriesId),
  );

  const open = (item: InteractionItem) => {
    setTarget(item.seriesId, 1);
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
    return (
      <div className="text-muted-foreground flex flex-col items-center gap-3 p-4 py-16">
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
      <div className="text-muted-foreground flex flex-col items-center gap-2 p-4 py-16">
        <Heart className="size-8 opacity-40" aria-hidden />
        <p className="text-sm">{t('liked.empty')}</p>
        <p className="text-xs opacity-70">{t('liked.emptyHint')}</p>
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-3 p-4">
      <div className="flex justify-end gap-2">
        <ListSearch value={query} onChange={setQuery} placeholder={t('liked.searchPlaceholder')} />
        <ManageToggle managing={batch.managing} onEnter={batch.enter} onExit={batch.exit} />
      </div>
      <div className="grid grid-cols-2 gap-3 sm:grid-cols-3 lg:grid-cols-4 xl:grid-cols-5">
        {shown.map((item) => (
          <LikedCard
            key={`${item.vid}:${item.seriesId}`}
            item={item}
            managing={batch.managing}
            picked={batch.selected.has(item.vid)}
            onPick={() => batch.toggle(item.vid)}
            onOpen={() => open(item)}
            onTitle={registerTitle}
            onUndo={() =>
              digg.mutate(
                { vid: item.vid, seriesId: item.seriesId, digg: false },
                {
                  onSuccess: () => toast.success(t('player.interact.undone')),
                  onError: (e) => toast.error(String(e)),
                },
              )
            }
          />
        ))}
      </div>
      {batch.managing && (
        <BatchBar
          count={batch.selected.size}
          total={shown.length}
          deleting={batchUndo.isPending}
          deleteLabel={t('liked.undo')}
          confirmTitle={t('liked.batchUndoTitle')}
          onToggleAll={() => batch.toggleAll(shown.map((i) => i.vid))}
          onDelete={() => batchUndo.mutate(shown.filter((i) => batch.selected.has(i.vid)))}
        />
      )}
    </div>
  );
}

/** 点赞卡：形态同「收藏」卡——封面 3:4 + 标题，悬浮出续播与「取消赞」；
 * 次行保留本页特有信息：红心 + 该集点赞计数。管理模式下角标多选。 */
function LikedCard({
  item,
  managing,
  picked,
  onPick,
  onOpen,
  onUndo,
  onTitle,
}: {
  item: InteractionItem;
  managing: boolean;
  picked: boolean;
  onPick: () => void;
  onOpen: () => void;
  onUndo: () => void;
  onTitle: (id: string, title: string) => void;
}) {
  const { data: meta } = useSeriesMeta(item.seriesId);
  const rawCover = meta?.cover ?? '';
  const { data: webCover } = useWebCover(rawCover);
  const cover = webCover ?? (isRenderableCover(rawCover) ? rawCover : '');
  const [brokenFor, setBrokenFor] = useState('');
  const imgBroken = brokenFor !== '' && brokenFor === cover;
  const showImg = cover !== '' && !imgBroken;
  const title = item.seriesTitle || meta?.title || '';

  // 标题到位就登记给页面级过滤（mget 缺标题、靠 resolve 补的条目）
  useEffect(() => {
    if (title !== '') onTitle(item.seriesId, title);
  }, [item.seriesId, title, onTitle]);

  const open = () => (managing ? onPick() : onOpen());

  return (
    <div className="group relative">
      <article
        role="button"
        tabIndex={0}
        aria-label={title}
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
              alt={title}
              loading="lazy"
              className="size-full object-cover"
              onError={() => setBrokenFor(cover)}
            />
          ) : (
            <div className="text-muted-foreground grid size-full place-items-center">
              <Heart className="size-5" />
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
          <p className="truncate text-sm font-semibold" title={title}>
            {title !== '' ? title : tf('collections.unresolved', { id: item.seriesId.slice(-6) })}
          </p>
          <div className="text-muted-foreground flex items-center gap-1 text-xs">
            <Heart className="size-3.5 fill-red-400 text-red-400" aria-hidden />
            <span className="tabular-nums">{item.diggedCount > 0 ? item.diggedCount : ''}</span>
          </div>
        </div>
      </article>
      {!managing && (
        <button
          type="button"
          onClick={onUndo}
          title={t('liked.undo')}
          className="absolute top-2 right-2 hidden size-7 place-items-center rounded-full bg-black/60 text-white group-hover:grid hover:bg-black/80"
        >
          <HeartOff className="size-3.5" />
        </button>
      )}
    </div>
  );
}
