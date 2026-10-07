import { useEffect, useMemo, useRef, useState } from 'react';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { BellRing, Flame, Loader2, Star, Tv } from 'lucide-react';
import { toast } from 'sonner';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Skeleton } from '@/components/ui/skeleton';
import { RefreshShade } from '@/components/refresh-shade';
import { ResolvingPill } from '@/components/resolving-pill';
import { SkeletonCardGrid, SkeletonRows } from '@/components/skeletons';
import { TopBarTab, TopBarTabsPortal } from '@/components/layout/top-bar-tabs';
import {
  SeriesDetailSheet,
  type SeriesRef,
} from '@/features/series/components/series-detail-sheet';
import { rank as rankApi } from '@/lib/ipc/commands';
import {
  isRenderableCover,
  useDownloadTasks,
  useNewCalendar,
  useNewDrama,
  useResolveSeries,
  useWebCover,
} from '@/lib/queries';
import { t, tf } from '@/i18n';
import type { CalendarItem, RankItem } from '@/lib/schema';

/**
 * 新剧页：新剧推荐 + 上新日历两个视图。
 *
 * 推荐 = `firstonlinetime_new` 按上架时间倒序的翻页流；
 * 日历 = 官方按日期排布的上新表（前后各一周，含未上线条目）。
 */

const GENDERS: { value: number; labelKey: string }[] = [
  { value: 2, labelKey: 'newDrama.gender.all' },
  { value: 1, labelKey: 'newDrama.gender.male' },
  { value: 0, labelKey: 'newDrama.gender.female' },
];

export function NewDramaPage() {
  const { mutate: resolve, isPending: resolving } = useResolveSeries();
  const [detail, setDetail] = useState<{ card: SeriesRef; selected: number[] } | null>(null);
  // 频道筛选在页面层：官方把它放在标题行右侧，对推荐/日历两个视图都可见
  const [gender, setGender] = useState(2);
  // 视图 tab（推荐/日历）：状态自持——tab 胶囊 portal 进 AppShell 顶栏，
  // 不能再依赖 Radix Tabs 的组件树上下文（Trigger 必须长在 Tabs 里）
  const [view, setView] = useState<'recommend' | 'calendar'>('recommend');

  const handleSelect = (item: {
    seriesId: string;
    title: string;
    cover: string;
    episodeCnt: number;
  }) => {
    resolve(item.seriesId, {
      onSuccess: (series) =>
        setDetail({
          card: {
            seriesId: series.seriesId,
            seriesTitle: series.title,
            cover: series.cover || item.cover,
            episodeCount: series.episodeCount || item.episodeCnt,
            tags: series.tags,
          },
          selected: [],
        }),
      onError: (e) =>
        toast.error(t('common.resolveFailed'), { description: String(e.message ?? e) }),
    });
  };

  return (
    // 视图 tab 已上移 AppShell 顶栏（TopBarTabsPortal，见下）
    <div className="flex min-h-full flex-col">
      <TopBarTabsPortal>
        <TopBarTab active={view === 'recommend'} onClick={() => setView('recommend')}>
          {t('newDrama.tabs.recommend')}
        </TopBarTab>
        <TopBarTab active={view === 'calendar'} onClick={() => setView('calendar')}>
          {t('newDrama.tabs.calendar')}
        </TopBarTab>
      </TopBarTabsPortal>

      <div className="flex min-h-0 flex-1 flex-col gap-3">
        {/* 顶行只留频道胶囊；页标题由侧栏高亮表达，不重复 */}
        <div className="mt-3 flex flex-wrap items-center justify-end gap-2">
          {GENDERS.map(({ value, labelKey }) => (
            <Button
              key={value}
              size="sm"
              variant={gender === value ? 'default' : 'outline'}
              className="rounded-full px-4"
              onClick={() => setGender(value)}
            >
              {t(labelKey)}
            </Button>
          ))}
        </div>

        {view === 'recommend' ? (
          <NewDramaRecommends gender={gender} onSelect={handleSelect} />
        ) : (
          <NewCalendarView onSelect={handleSelect} />
        )}
      </div>

      <SeriesDetailSheet
        card={detail?.card ?? null}
        selected={detail?.selected ?? []}
        onSelectedChange={(next) => setDetail((d) => (d ? { ...d, selected: next } : d))}
        onOpenChange={(open) => !open && setDetail(null)}
      />

      {resolving && <ResolvingPill />}
    </div>
  );
}

/** 新剧推荐：封面网格 + 滚动翻页（频道筛选在页面标题行）。 */
function NewDramaRecommends({
  gender,
  onSelect,
}: {
  gender: number;
  onSelect: (item: RankItem) => void;
}) {
  const feed = useNewDrama(gender);
  const { data: tasks } = useDownloadTasks();

  const downloadedMap = useMemo(() => {
    const map: Record<string, number> = {};
    for (const task of tasks ?? []) {
      if (task.status !== 'completed') continue;
      map[task.seriesId] = (map[task.seriesId] ?? 0) + 1;
    }
    return map;
  }, [tasks]);

  // 挂载与换频道的首拉由 useInfiniteQuery 随 queryKey 自动驱动；
  // 这里只留哨兵回调用的最新状态镜像（feed 是每次渲染的新对象）
  const feedRef = useRef(feed);
  useEffect(() => {
    feedRef.current = feed;
  });

  const sentinelRef = useRef<HTMLDivElement | null>(null);
  useEffect(() => {
    const el = sentinelRef.current;
    if (!el) return;
    const io = new IntersectionObserver(
      (entries) => {
        const f = feedRef.current;
        if (entries[0]?.isIntersecting && f.items.length > 0 && !f.isLoading && !f.isFetchingMore) {
          void f.loadMore();
        }
      },
      { rootMargin: '400px' },
    );
    io.observe(el);
    return () => io.disconnect();
  }, []);

  return (
    <div className="flex flex-col gap-4">
      <RefreshShade refreshing={feed.isRefreshing}>
        {feed.isLoading ? (
          <SkeletonCardGrid count={9} />
        ) : feed.error ? (
          <div className="text-muted-foreground flex flex-col items-center gap-3 py-16">
            <p>{t('newDrama.loadFailed')}</p>
            <p className="text-destructive text-xs">{feed.error}</p>
            <Button variant="outline" size="sm" onClick={() => void feed.refresh()}>
              <Loader2 className="mr-1 size-4 animate-spin" aria-hidden />
              {t('feed.retry')}
            </Button>
          </div>
        ) : (
          <div className="grid grid-cols-2 gap-4 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 2xl:grid-cols-8">
            {feed.items.map((item) => (
              <NewDramaCard
                key={item.seriesId}
                item={item}
                downloaded={downloadedMap[item.seriesId] ?? 0}
                onSelect={onSelect}
              />
            ))}
          </div>
        )}
      </RefreshShade>

      <div ref={sentinelRef} className="h-px" aria-hidden />
      {feed.isFetchingMore && (
        <p className="text-muted-foreground flex items-center justify-center gap-2 py-2 text-sm">
          <Loader2 className="size-4 animate-spin" aria-hidden />
          {t('feed.loadingMore')}
        </p>
      )}
      {!feed.isLoading && !feed.error && feed.items.length > 0 && !feed.isFetchingMore && (
        <p className="text-muted-foreground py-2 text-center text-sm">{t('feed.end')}</p>
      )}
    </div>
  );
}

/** 新剧卡：字段比信息流少（无 tags/commentCount），subTitle 直接展示。 */
function NewDramaCard({
  item,
  downloaded,
  onSelect,
}: {
  item: RankItem;
  downloaded: number;
  onSelect: (item: RankItem) => void;
}) {
  const { data: webCover } = useWebCover(item.seriesId, item.cover);
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
      onClick={() => onSelect(item)}
      onKeyDown={(e) => {
        if (e.key !== 'Enter' && e.key !== ' ') return;
        e.preventDefault();
        onSelect(item);
      }}
      className="group bg-card hover:border-foreground/30 focus-visible:border-foreground/30 flex w-full cursor-pointer flex-col overflow-hidden rounded-xl border text-left transition-colors hover:shadow-md focus-visible:outline-none"
    >
      <div className="bg-muted relative aspect-[3/4] w-full overflow-hidden">
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
            <Tv className="size-8" />
          </div>
        )}
        {/* 官方同款封面角标：左下热度（黄）、右下总集数 */}
        {item.recText !== '' && (
          <span className="absolute bottom-1 left-1 flex items-center gap-0.5 text-[10px] font-semibold text-amber-300 drop-shadow-[0_1px_2px_rgba(0,0,0,0.9)]">
            <Flame className="size-3" aria-hidden />
            {item.recText}
          </span>
        )}
        {item.episodeCnt > 0 && (
          <span className="absolute right-1 bottom-1 rounded bg-black/60 px-1 py-0.5 text-[10px] text-white">
            {tf('newDrama.episodeTotal', { count: item.episodeCnt })}
          </span>
        )}
        {downloaded > 0 && (
          <Badge
            variant="success"
            className="absolute top-2 right-2 shadow-sm"
            title={tf('browse.downloadedBadge', { count: downloaded })}
          >
            {downloaded}
          </Badge>
        )}
      </div>
      {/* 官方同款信息排布：标题 / 分类标签 / 分数 */}
      <div className="flex flex-col gap-1 p-2">
        <p className="truncate text-sm font-semibold" title={item.title}>
          {item.title}
        </p>
        <p className="text-muted-foreground truncate text-xs">
          {[...item.tags, item.subTitle !== '' ? item.subTitle.split('·')[0] : '']
            .filter((x) => x !== '')
            .slice(0, 4)
            .join('·')}
        </p>
        {item.score > 0 && (
          <p className="flex items-center gap-1 text-xs font-semibold">
            <Star className="size-3.5 text-amber-400" aria-hidden />
            {item.score.toFixed(1)}
            {t('newDrama.scoreSuffix')}
          </p>
        )}
      </div>
    </article>
  );
}

/** 上新日历：日期条 + 当日上新列表（含未上线）。 */
function NewCalendarView({ onSelect }: { onSelect: (item: CalendarItem) => void }) {
  const [date, setDate] = useState('');
  const { data, isLoading, error, isFetching, refetch } = useNewCalendar(date);
  // 首次拿到日期列表后选中默认日（空串 = 默认日，这里显式化便于高亮）
  const active = date === '' ? (data?.defaultDate ?? '') : date;
  const dates = data?.dates ?? [];

  return (
    <div className="flex flex-col gap-4">
      {/* 日期条常驻不参与 loading：切日期只换下方列表（keepPreviousData
          平滑过渡）；首屏日期未到时骨架占位，不留一条空行。
          按钮官方同款：均匀铺满一行，星期小字 + 日期大字，选中主色底 */}
      <div className="flex min-h-14 flex-wrap items-center gap-2">
        {isLoading && dates.length === 0
          ? Array.from({ length: 8 }, (_, i) => (
              <Skeleton key={i} className="h-14 min-w-16 flex-1 rounded-lg" />
            ))
          : dates.map((d) => (
              <Button
                key={d}
                size="sm"
                variant={active === d ? 'default' : 'outline'}
                className="h-14 min-w-14 flex-1 flex-col gap-0 rounded-lg px-1 py-1.5 sm:max-w-20"
                onClick={() => setDate(d)}
              >
                <span className="text-[11px] leading-tight opacity-80">{formatWeekday(d)}</span>
                <span className="text-sm leading-tight font-semibold tabular-nums">
                  {formatDayMonth(d)}
                </span>
              </Button>
            ))}
      </div>

      <RefreshShade refreshing={isFetching && !isLoading}>
        {isLoading ? (
          <SkeletonRows count={6} height="h-20 rounded-xl" />
        ) : error ? (
          <div className="text-muted-foreground flex flex-col items-center gap-3 py-16">
            <p>{t('newDrama.loadFailed')}</p>
            <p className="text-destructive text-xs">{error.message}</p>
            <Button variant="outline" size="sm" onClick={() => void refetch()}>
              <Loader2 className="mr-1 size-4 animate-spin" aria-hidden />
              {t('feed.retry')}
            </Button>
          </div>
        ) : (data?.items.length ?? 0) === 0 ? (
          <p className="text-muted-foreground py-16 text-center text-sm">{t('newDrama.empty')}</p>
        ) : (
          <div className="flex flex-col gap-2">
            {(data?.items ?? []).map((item) => (
              <CalendarRow key={item.seriesId} item={item} onSelect={onSelect} />
            ))}
          </div>
        )}
      </RefreshShade>
    </div>
  );
}

/** 日历一行：封面 | 标题/分类/简介 | 上线状态与热度 | 预约。 */
function CalendarRow({
  item,
  onSelect,
}: {
  item: CalendarItem;
  onSelect: (item: CalendarItem) => void;
}) {
  const qc = useQueryClient();
  const { data: webCover } = useWebCover(item.seriesId, item.cover);
  const sourceRenderable = isRenderableCover(item.cover);
  const cover = webCover ?? (sourceRenderable ? item.cover : '');
  const [brokenFor, setBrokenFor] = useState('');
  const imgBroken = brokenFor !== '' && brokenFor === cover;
  const showImg = cover !== '' && !imgBroken;
  const heat = item.recTags[0] ?? '';

  // 日历形态响应不带预约态（has_subscribed 恒 false），本地记已点过的剧
  const [reserved, setReserved] = useState(item.hasSubscribed);
  const reserve = useMutation({
    mutationFn: () => rankApi.reserve(item.seriesId, true),
    onSuccess: () => {
      setReserved(true);
      toast.success(t('reservation.done'));
      void qc.invalidateQueries({ queryKey: ['reservations'] });
    },
    onError: (e: Error) => toast.error(e.message),
  });

  return (
    <article
      role="button"
      tabIndex={0}
      aria-label={item.title}
      onClick={() => onSelect(item)}
      onKeyDown={(e) => {
        if (e.key !== 'Enter' && e.key !== ' ') return;
        e.preventDefault();
        onSelect(item);
      }}
      className="bg-card hover:border-foreground/30 focus-visible:border-foreground/30 flex cursor-pointer items-center gap-4 rounded-xl border p-3 text-left transition-colors hover:shadow-md focus-visible:outline-none"
    >
      <div className="bg-muted relative aspect-[3/4] w-14 shrink-0 overflow-hidden rounded-lg">
        {showImg ? (
          <img
            src={cover}
            alt=""
            loading="lazy"
            className="size-full object-cover"
            onError={() => setBrokenFor(cover)}
          />
        ) : (
          <div className="text-muted-foreground grid size-full place-items-center">
            <Tv className="size-5" />
          </div>
        )}
      </div>

      <div className="flex min-w-0 flex-1 flex-col gap-1">
        <div className="flex items-center gap-2">
          <p className="truncate text-sm font-semibold" title={item.title}>
            {item.title}
          </p>
          {item.isOnline ? (
            <Badge variant="success" className="shrink-0 text-[10px]">
              {t('newDrama.online')}
            </Badge>
          ) : (
            <Badge variant="secondary" className="shrink-0 text-[10px]">
              {t('newDrama.upcoming')}
            </Badge>
          )}
        </div>
        {/* 官方同款：预约人数红色醒目，后跟分类/评分/集数 */}
        <div className="flex flex-wrap items-center gap-2 text-xs">
          {heat !== '' && <span className="font-semibold text-red-500">{heat}</span>}
          {item.category !== '' && <span className="text-muted-foreground">{item.category}</span>}
          {item.score > 0 && (
            <span className="text-muted-foreground flex items-center gap-0.5">
              <Star className="size-3 text-amber-400" aria-hidden />
              {item.score.toFixed(1)}
            </span>
          )}
          {item.episodeCnt > 0 && (
            <span className="text-muted-foreground">
              {tf('common.episodeCount', { count: item.episodeCnt })}
            </span>
          )}
        </div>
        {item.description !== '' && (
          <p className="text-muted-foreground/80 line-clamp-2 text-xs leading-relaxed">
            {item.description}
          </p>
        )}
      </div>

      <div className="flex shrink-0 flex-col items-end gap-1">
        {item.publishTime > 0 && (
          <span className="text-muted-foreground text-xs tabular-nums">
            {formatPublishTime(item.publishTime)}
          </span>
        )}
        {!item.isOnline && (
          // 官方同款红色实心胶囊（品牌红，白字铃铛）
          <Button
            size="sm"
            variant={reserved ? 'secondary' : 'default'}
            disabled={reserved || reserve.isPending}
            className={
              reserved ? 'rounded-full' : 'rounded-full bg-red-500 text-white hover:bg-red-500/90'
            }
            onClick={() => reserve.mutate()}
          >
            {reserve.isPending ? (
              <Loader2 className="size-4 animate-spin" aria-hidden />
            ) : (
              <BellRing className="size-4" aria-hidden />
            )}
            {reserved ? t('reservation.reserved') : t('reservation.action')}
          </Button>
        )}
      </div>
    </article>
  );
}

/** "20261003" → 今天/周几（hgplayer 日期条同款：今天显示「今天」，其余
 *  显示星期；文案走 i18n，中英文各自成串）。 */
function formatWeekday(d: string): string {
  if (d.length !== 8) return '';
  const today = new Date();
  const pad = (n: number) => String(n).padStart(2, '0');
  const todayKey = `${today.getFullYear()}${pad(today.getMonth() + 1)}${pad(today.getDate())}`;
  if (d === todayKey) return t('newDrama.today');
  const dt = new Date(Number(d.slice(0, 4)), Number(d.slice(4, 6)) - 1, Number(d.slice(6, 8)));
  if (Number.isNaN(dt.getTime())) return '';
  return t(`newDrama.weekday.${dt.getDay()}`);
}

/** "20261003" → "10/3"。 */
function formatDayMonth(d: string): string {
  if (d.length !== 8) return d;
  const month = Number(d.slice(4, 6));
  const day = Number(d.slice(6, 8));
  return `${month}/${day}`;
}

/** unix 秒 → "MM-DD HH:mm"（本地时区）。 */
function formatPublishTime(sec: number): string {
  const dt = new Date(sec * 1000);
  if (Number.isNaN(dt.getTime())) return '';
  const pad = (n: number) => String(n).padStart(2, '0');
  return `${pad(dt.getMonth() + 1)}-${pad(dt.getDate())} ${pad(dt.getHours())}:${pad(dt.getMinutes())}`;
}
