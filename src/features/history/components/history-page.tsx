import { useEffect, useRef, useState } from 'react';
import { useNavigate } from '@tanstack/react-router';
import { History, Loader2, Play } from 'lucide-react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { ListSearch } from '@/components/list-search';
import { matchListQuery } from '@/lib/list-filter';
import { Skeleton } from '@/components/ui/skeleton';
import { cn } from '@/lib/utils';
import { useResolveSeries, useWatchHistory, useWebCover } from '@/service/queries';
import { isRenderableCover } from '@/utils/cover';
import { usePlayerStore } from '@/lib/stores/player';
import { formatDuration } from '@/lib/format';
import { t, tf } from '@/i18n';
import type { WatchHistoryItem } from '@/service/schema';

/**
 * 「历史」——云端观看记录（官方 App「历史」同源 read_history/list，
 * book_type=2 短剧过滤；官方/第三方客户端看的记录都在，2026-10-05 实证
 * 469 条带名字带进度）。
 *
 * 点卡片跳播放器续播；封面是 HEIC 签名 URL，走 useWebCover 转码代理。
 * 顶部「全部/未看完/已看完」筛选与标题搜索对齐参考端 v1.1.6：
 * 已看完 = episode_cnt>0 且已看到最后一集（参考端 History 页 p() 同款判定，
 * 不看片内进度）。
 */

type HistoryTab = 'all' | 'unfinished' | 'finished';

/** 参考端同款「已看完」判定：看到最后一集即算，与片内进度无关。 */
function isFinished(item: WatchHistoryItem): boolean {
  return item.episodeCnt > 0 && Math.max(1, item.vidIndex) >= item.episodeCnt;
}

const TABS: { key: HistoryTab; labelKey: string }[] = [
  { key: 'all', labelKey: 'history.tabAll' },
  { key: 'unfinished', labelKey: 'history.tabUnfinished' },
  { key: 'finished', labelKey: 'history.tabFinished' },
];

export function HistoryPage() {
  const navigate = useNavigate();
  const setTarget = usePlayerStore((s) => s.setTarget);
  const { data, isLoading, error, refetch } = useWatchHistory();
  const { mutate: resolve, isPending: resolving } = useResolveSeries();

  const [tab, setTab] = useState<HistoryTab>('all');
  const [query, setQuery] = useState('');

  const items = data?.items ?? [];
  const shown = items.filter((item) => {
    if (tab === 'finished' && !isFinished(item)) return false;
    if (tab === 'unfinished' && isFinished(item)) return false;
    return matchListQuery(query, item.title, item.seriesId);
  });

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
      { rootMargin: '600px' },
    );
    observer.observe(el);
    return () => observer.disconnect();
  });

  const open = (item: WatchHistoryItem) => {
    resolve(item.seriesId, {
      onSuccess: () => {
        // 云端 vid_index 1 起（2026-10-09 实证：云端行的 vid 与该集 vid
        // 一一对应，双魂共生 vid_index=18 即第 18/18 集；第三方 hgplayer
        // 写入的是 0 基，历史数据可能混有 0——钳到第 1 集）。播放器
        // store 同为 1 起（0=无目标）
        setTarget(item.seriesId, Math.max(1, item.vidIndex));
        void navigate({ to: '/player' });
      },
      onError: (e) => toast.error(t('common.resolveFailed'), { description: e.message }),
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
      <div className="text-muted-foreground flex flex-col items-center gap-3 p-4 py-16">
        <p>{t('history.loadFailed')}</p>
        <p className="text-destructive text-xs">{error.message}</p>
        <Button variant="outline" size="sm" onClick={() => void refetch()}>
          <Loader2 className="mr-1 size-4 animate-spin" aria-hidden />
          {t('feed.retry')}
        </Button>
      </div>
    );
  }
  if (items.length === 0) {
    return (
      <div className="text-muted-foreground flex flex-col items-center gap-2 p-4 py-16">
        <History className="size-8 opacity-40" aria-hidden />
        <p className="text-sm">{t('history.empty')}</p>
        <p className="text-xs opacity-70">{t('history.emptyHint')}</p>
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-3 p-4">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="flex items-center gap-2">
          {TABS.map(({ key, labelKey }) => (
            <button
              key={key}
              type="button"
              onClick={() => setTab(key)}
              className={cn(
                'rounded-full px-4 py-1.5 text-sm transition-colors',
                tab === key
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
          placeholder={t('history.searchPlaceholder')}
        />
      </div>

      {shown.length === 0 ? (
        // 列表非空但被 tab/搜索滤空：给一行轻提示（文案区分场景）
        <div className="text-muted-foreground flex flex-col items-center gap-2 py-16">
          <History className="size-8 opacity-40" aria-hidden />
          <p className="text-sm">
            {query.trim() !== ''
              ? t('common.searchNoMatch')
              : tab === 'finished'
                ? t('history.noMatchFinished')
                : t('history.noMatchUnfinished')}
          </p>
        </div>
      ) : (
        <div className="flex flex-col gap-2">
          {shown.slice(0, visibleCount).map((item) => (
            <HistoryRow
              key={`${item.seriesId}:${item.updatedAtMs}`}
              item={item}
              onOpen={() => open(item)}
            />
          ))}
          {/* 续载哨兵：滚近底部（600px 提前量）继续渲染下一批 */}
          {visibleCount < shown.length && <div ref={sentinelRef} className="h-px" />}
          {resolving && <p className="text-muted-foreground text-sm">{t('common.resolving')}</p>}
        </div>
      )}
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
      className="bg-card hover:border-foreground/30 focus-visible:border-foreground/30 flex cursor-pointer items-center gap-4 rounded-xl border p-3 text-left transition-colors hover:shadow-md focus-visible:outline-none [content-visibility:auto] [contain-intrinsic-size:auto_104px]"
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
