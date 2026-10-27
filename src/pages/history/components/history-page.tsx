import { useEffect, useRef, useState } from 'react';
import { History, Play } from 'lucide-react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { ErrorState } from '@/components/common/error-state';
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group';
import {
  Empty,
  EmptyDescription,
  EmptyHeader,
  EmptyMedia,
  EmptyTitle,
} from '@/components/ui/empty';
import { ListSearch } from '@/components/common/list-search';
import { matchListQuery } from '@/utils/list-filter';
import { Skeleton } from '@/components/ui/skeleton';
import { cn } from '@/lib/utils';
import {
  useAccount,
  useLocalHistory,
  useLocalHistoryDelete,
  useResolveSeries,
  useSeriesMeta,
  useWatchHistory,
  useWatchHistoryDelete,
} from '@/service/queries';
import { BatchBar, ManageToggle, PickDot } from '@/components/common/batch-manage';
import { useBatchSelect } from '@/hooks/use-batch-select';
import { useScrollRestore, useSessionState } from '@/hooks/use-scroll-restore';
import { enhancedCover, isRenderableCover } from '@/utils/cover';
import { usePlaySeries } from '@/hooks/use-play-series';
import { formatDuration } from '@/utils/format';
import { SENTINEL_ROOT_MARGIN_PX } from '@/utils/infinite-scroll';
import { t, tf, type MessageKey } from '@/locales';
import type { WatchHistoryItem } from '@/service/schema';

/**
 * 「历史」——云端观看记录（官方 App「历史」同源 read_history/list，
 * book_type=2 短剧过滤；各端看的记录都在）。
 *
 * 点卡片跳播放器续播；封面是 HEIC 签名 URL，经 enhancedCover 转码代理。
 * 顶部「全部/未看完/已看完」筛选与标题搜索对齐官方客户端：
 * 已看完 = episode_cnt>0 且已看到最后一集（官方客户端同款判定，
 * 不看片内进度）。
 */

type HistoryTab = 'all' | 'unfinished' | 'finished';

/** 官方同款「已看完」判定：看到最后一集即算，与片内进度无关。 */
function isFinished(item: WatchHistoryItem): boolean {
  return item.episodeCnt > 0 && Math.max(1, item.vidIndex) >= item.episodeCnt;
}

const TABS: { key: HistoryTab; labelKey: MessageKey }[] = [
  { key: 'all', labelKey: 'history.tabAll' },
  { key: 'unfinished', labelKey: 'history.tabUnfinished' },
  { key: 'finished', labelKey: 'history.tabFinished' },
];

export function HistoryPage() {
  const playSeries = usePlaySeries();
  // 双模式（issue #10）：登录 = 云端历史（官方 App 同源，跨设备）；
  // 未登录 = 本地观看记录（播放时已落库，删除只动本机）
  const { data: account } = useAccount();
  const loggedIn = !!account;
  const cloud = useWatchHistory(loggedIn);
  const local = useLocalHistory(!loggedIn);
  const data = loggedIn ? cloud.data : local.data ? { items: local.data } : undefined;
  const isLoading = loggedIn ? cloud.isLoading : local.isLoading;
  const error = loggedIn ? cloud.error : local.error;
  const refetch = loggedIn ? cloud.refetch : local.refetch;
  const { mutate: resolve, isPending: resolving } = useResolveSeries();

  // tab/搜索词会话级保留（进详情再返回停在离开时的 tab）
  const [tab, setTab] = useSessionState<HistoryTab>('hongguo.history.tab', 'all');
  const [query, setQuery] = useSessionState('hongguo.history.query', '');
  // 批量管理：多选后批量删除云端历史
  const batch = useBatchSelect();
  const deleteMutation = useWatchHistoryDelete();
  const localDeleteMutation = useLocalHistoryDelete();

  const items = data?.items ?? [];
  const shown = items.filter((item) => {
    if (tab === 'finished' && !isFinished(item)) return false;
    if (tab === 'unfinished' && isFinished(item)) return false;
    return matchListQuery(query, item.title, item.seriesId);
  });
  // 返回保留浏览位置（页面滚在 app-shell 的 #content）
  useScrollRestore(
    'hongguo.history.scroll',
    () => document.getElementById('content'),
    !isLoading && shown.length > 0,
  );

  // 渐进渲染：几百行一次性挂载是菜单点击卡顿的来源（实测 489 行 ~300ms
  // 主线程阻塞）。首批 20 行秒出，滚动到底部由哨兵续载，语义不变
  // （筛选/搜索仍作用于全量 shown）。
  const [visibleCount, setVisibleCount] = useState(20);
  // tab/搜索变化时重置批量（渲染期调整 state 的官方模式，避免 effect 级联）
  const [prevFilterKey, setPrevFilterKey] = useState('');
  const filterKey = `${tab}:${query}`;
  if (prevFilterKey !== filterKey) {
    setPrevFilterKey(filterKey);
    setVisibleCount(20);
  }
  const sentinelRef = useRef<HTMLDivElement | null>(null);
  useEffect(() => {
    const el = sentinelRef.current;
    if (!el) return;
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((e) => e.isIntersecting)) {
          setVisibleCount((n) => (n < shown.length ? n + 40 : n));
        }
      },
      { rootMargin: `${SENTINEL_ROOT_MARGIN_PX}px` },
    );
    observer.observe(el);
    return () => observer.disconnect();
  });

  const open = (item: WatchHistoryItem) => {
    resolve(item.seriesId, {
      onSuccess: () => {
        // 云端 vid_index 1 起（云端行的 vid 与该集 vid
        // 一一对应，双魂共生 vid_index=18 即第 18/18 集；历史数据可能混有
        // 旧客户端写入的 0 基——钳到第 1 集）。播放器
        // store 同为 1 起（0=无目标）
        playSeries(item.seriesId, Math.max(1, item.vidIndex));
      },
    });
  };

  if (isLoading) {
    return (
      <div className="flex flex-col gap-3 p-4">
        {Array.from({ length: 6 }, (_, i) => (
          <Skeleton key={i} className="h-28 rounded-xl" />
        ))}
      </div>
    );
  }
  if (error) {
    return (
      <ErrorState
        message={t('history.loadFailed')}
        detail={error.message}
        onRetry={() => void refetch()}
      />
    );
  }
  if (items.length === 0) {
    return (
      <Empty className="p-4 py-16">
        <EmptyHeader>
          <EmptyMedia>
            <History className="size-8 opacity-40" aria-hidden />
          </EmptyMedia>
          <EmptyTitle className="text-muted-foreground text-sm font-normal">
            {t('history.empty')}
          </EmptyTitle>
          <EmptyDescription className="text-xs opacity-70">
            {t('history.emptyHint')}
          </EmptyDescription>
        </EmptyHeader>
      </Empty>
    );
  }

  return (
    <div className="flex flex-col gap-3 p-4">
      {/* 未登录提示（issue #10）：本地模式可记可管，数据不上云 */}
      {!loggedIn && (
        <p className="text-muted-foreground px-1 text-xs">{t('history.localModeHint')}</p>
      )}
      <div className="flex flex-wrap items-center justify-between gap-2">
        <ToggleGroup
          type="single"
          spacing={1}
          value={tab}
          onValueChange={(v) => v !== '' && setTab(v as HistoryTab)}
          className="flex items-center gap-2"
        >
          {TABS.map(({ key, labelKey }) => (
            <ToggleGroupItem
              key={key}
              value={key}
              className="text-muted-foreground hover:bg-accent hover:text-accent-foreground data-[state=on]:bg-primary data-[state=on]:text-primary-foreground h-auto min-w-0 rounded-full px-3 py-1 text-xs transition-colors data-[state=on]:font-medium"
            >
              {t(labelKey)}
            </ToggleGroupItem>
          ))}
        </ToggleGroup>
        <ListSearch
          value={query}
          onChange={setQuery}
          placeholder={t('history.searchPlaceholder')}
        />
        <ManageToggle managing={batch.managing} onEnter={batch.enter} onExit={batch.exit} />
      </div>

      {shown.length === 0 ? (
        // 列表非空但被 tab/搜索滤空：给一行轻提示（文案区分场景）
        <Empty className="py-16">
          <EmptyHeader>
            <EmptyMedia>
              <History className="size-8 opacity-40" aria-hidden />
            </EmptyMedia>
            <EmptyTitle className="text-muted-foreground text-sm font-normal">
              {query.trim() !== ''
                ? t('common.searchNoMatch')
                : tab === 'finished'
                  ? t('history.noMatchFinished')
                  : t('history.noMatchUnfinished')}
            </EmptyTitle>
          </EmptyHeader>
        </Empty>
      ) : (
        <>
          <div className="flex flex-col gap-2">
            {shown.slice(0, visibleCount).map((item) => (
              <HistoryRow
                key={`${item.seriesId}:${item.updatedAtMs}`}
                item={item}
                onOpen={() => open(item)}
                pick={
                  batch.managing
                    ? {
                        checked: batch.selected.has(item.seriesId),
                        onToggle: () => batch.toggle(item.seriesId),
                      }
                    : undefined
                }
              />
            ))}
            {/* 续载哨兵：滚近底部（400px 提前量）继续渲染下一批 */}
            {visibleCount < shown.length && <div ref={sentinelRef} className="h-px" />}
            {resolving && <p className="text-muted-foreground text-sm">{t('common.resolving')}</p>}
          </div>
          {batch.managing && (
            <BatchBar
              count={batch.selected.size}
              total={shown.length}
              deleting={deleteMutation.isPending}
              deleteLabel={t('batch.deleteSelected')}
              confirmTitle={t('history.deleteConfirmTitle')}
              onToggleAll={() => batch.toggleAll(shown.map((i) => i.seriesId))}
              onDelete={() => {
                const count = batch.selected.size;
                if (loggedIn) {
                  deleteMutation.mutate(
                    items
                      .filter((i) => batch.selected.has(i.seriesId))
                      .map((i) => ({ seriesId: i.seriesId, vid: i.vid, vidIndex: i.vidIndex })),
                    {
                      onSuccess: () => {
                        toast.success(tf('batch.done', { count }));
                        batch.exit();
                        void refetch();
                      },
                      onError: (e: Error) =>
                        toast.error(tf('batch.partialFail', { error: e.message })),
                    },
                  );
                } else {
                  localDeleteMutation.mutate(
                    items.filter((i) => batch.selected.has(i.seriesId)).map((i) => i.seriesId),
                    {
                      onSuccess: () => {
                        toast.success(tf('batch.done', { count }));
                        batch.exit();
                        void refetch();
                      },
                      onError: (e: Error) =>
                        toast.error(tf('batch.partialFail', { error: e.message })),
                    },
                  );
                }
              }}
            />
          )}
        </>
      )}
    </div>
  );
}

/** 历史行：官方同款「看到第N集/共M集 时长」进度徽标 + 继续播放。
 *  管理模式下 pick 在场：行首圆钮多选，点击行=切换选中。 */
function HistoryRow({
  item: rawItem,
  onOpen,
  pick,
}: {
  item: WatchHistoryItem;
  onOpen: () => void;
  pick?: { checked: boolean; onToggle: () => void };
}) {
  // v0.5.1 的 record 契约错配（meta 平铺被 serde 丢弃）落过一批无标题行：
  // 快照回填展示字段（剧名/封面/总集数），只读不改库。传空串即禁用查询。
  const backfill = useSeriesMeta(rawItem.title === '' ? rawItem.seriesId : '');
  const item =
    rawItem.title === '' && backfill.data
      ? {
          ...rawItem,
          title: backfill.data.title,
          cover: backfill.data.cover,
          episodeCnt: backfill.data.episodeCount,
        }
      : rawItem;
  const webCover = enhancedCover(item.cover);
  const sourceRenderable = isRenderableCover(item.cover);
  const cover = webCover ?? (sourceRenderable ? item.cover : '');
  const [brokenFor, setBrokenFor] = useState('');
  const imgBroken = brokenFor !== '' && brokenFor === cover;
  const showImg = cover !== '' && !imgBroken;
  const open = () => (pick ? pick.onToggle() : onOpen());

  return (
    <article
      role="button"
      tabIndex={0}
      aria-label={item.title}
      onClick={open}
      onKeyDown={(e) => {
        if (e.key !== 'Enter' && e.key !== ' ') return;
        e.preventDefault();
        open();
      }}
      className={cn(
        'bg-card hover:border-foreground/30 focus-visible:border-foreground/30 flex cursor-pointer items-center gap-4 rounded-xl border p-3 text-left transition-colors [contain-intrinsic-size:auto_104px] [content-visibility:auto] hover:shadow-md focus-visible:outline-none',
        pick?.checked && 'border-primary/60 bg-primary/5',
      )}
    >
      {pick && <PickDot checked={pick.checked} onToggle={pick.onToggle} />}
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
                  index: Math.max(1, item.vidIndex),
                  total: item.episodeCnt > 0 ? item.episodeCnt : '–',
                  time: formatDuration(item.positionMs / 1000),
                })
              : tf('player.epShort', { index: Math.max(1, item.vidIndex) })}
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
