import { useEffect, useMemo, useRef, useState } from 'react';
import { CalendarDays, Flame, Loader2, Star, Tv } from 'lucide-react';
import { toast } from 'sonner';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Skeleton } from '@/components/ui/skeleton';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';
import {
  SeriesDetailSheet,
  type SeriesRef,
} from '@/features/series/components/series-detail-sheet';
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
    <div className="flex flex-col gap-4">
      <Tabs defaultValue="recommend">
        <TabsList>
          <TabsTrigger value="recommend">{t('newDrama.tabs.recommend')}</TabsTrigger>
          <TabsTrigger value="calendar">
            <CalendarDays className="mr-1 size-4" aria-hidden />
            {t('newDrama.tabs.calendar')}
          </TabsTrigger>
        </TabsList>

        <TabsContent value="recommend" className="mt-3">
          <NewDramaRecommends onSelect={handleSelect} />
        </TabsContent>

        <TabsContent value="calendar" className="mt-3">
          <NewCalendarView onSelect={handleSelect} />
        </TabsContent>
      </Tabs>

      <SeriesDetailSheet
        card={detail?.card ?? null}
        selected={detail?.selected ?? []}
        onSelectedChange={(next) => setDetail((d) => (d ? { ...d, selected: next } : d))}
        onOpenChange={(open) => !open && setDetail(null)}
      />

      {resolving && (
        <p className="text-muted-foreground bg-card fixed bottom-4 left-1/2 flex -translate-x-1/2 items-center gap-2 rounded-full border px-4 py-2 text-sm shadow-lg">
          <Loader2 className="size-4 animate-spin" aria-hidden />
          {t('browse.resolving')}
        </p>
      )}
    </div>
  );
}

/** 新剧推荐：频道筛选 + 封面网格 + 滚动翻页。 */
function NewDramaRecommends({ onSelect }: { onSelect: (item: RankItem) => void }) {
  const [gender, setGender] = useState(2);
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

  // 挂载即拉第一页（与 useFeed 相同的免循环依赖手法）
  const feedRef = useRef(feed);
  useEffect(() => {
    feedRef.current = feed;
  });
  useEffect(() => {
    void feedRef.current.refresh();
  }, [gender]);

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
      <div className="flex items-center gap-2">
        {GENDERS.map(({ value, labelKey }) => (
          <Button
            key={value}
            size="sm"
            variant={gender === value ? 'default' : 'outline'}
            onClick={() => setGender(value)}
          >
            {t(labelKey)}
          </Button>
        ))}
      </div>

      {feed.isLoading ? (
        <div className="grid grid-cols-2 gap-4 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 2xl:grid-cols-8">
          {Array.from({ length: 9 }, (_, i) => (
            <div key={i} className="flex flex-col gap-2">
              <Skeleton className="aspect-[3/4] w-full rounded-xl" />
              <Skeleton className="h-4 w-3/4" />
            </div>
          ))}
        </div>
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
      <div className="flex flex-col gap-1 p-3">
        <p className="truncate text-sm font-semibold" title={item.title}>
          {item.title}
        </p>
        <div className="text-muted-foreground flex items-center gap-2 text-xs">
          {item.score > 0 && (
            <span className="flex items-center gap-0.5">
              <Star className="size-3 text-amber-400" aria-hidden />
              {item.score.toFixed(1)}
            </span>
          )}
          {item.subTitle !== '' && <span className="truncate">{item.subTitle}</span>}
        </div>
      </div>
    </article>
  );
}

/** 上新日历：日期条 + 当日上新列表（含未上线）。 */
function NewCalendarView({ onSelect }: { onSelect: (item: CalendarItem) => void }) {
  const [date, setDate] = useState('');
  const { data, isLoading, error, refetch } = useNewCalendar(date);
  // 首次拿到日期列表后选中默认日（空串 = 默认日，这里显式化便于高亮）
  const active = date === '' ? (data?.defaultDate ?? '') : date;
  const dates = data?.dates ?? [];

  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-wrap gap-2">
        {dates.map((d) => (
          <Button
            key={d}
            size="sm"
            variant={active === d ? 'default' : 'outline'}
            className="tabular-nums"
            onClick={() => setDate(d)}
          >
            {formatDateChip(d)}
          </Button>
        ))}
      </div>

      {isLoading ? (
        <div className="flex flex-col gap-3">
          {Array.from({ length: 6 }, (_, i) => (
            <Skeleton key={i} className="h-20 rounded-xl" />
          ))}
        </div>
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
    </div>
  );
}

/** 日历一行：封面 | 标题/分类/简介 | 上线状态与热度。 */
function CalendarRow({
  item,
  onSelect,
}: {
  item: CalendarItem;
  onSelect: (item: CalendarItem) => void;
}) {
  const { data: webCover } = useWebCover(item.seriesId, item.cover);
  const sourceRenderable = isRenderableCover(item.cover);
  const cover = webCover ?? (sourceRenderable ? item.cover : '');
  const [brokenFor, setBrokenFor] = useState('');
  const imgBroken = brokenFor !== '' && brokenFor === cover;
  const showImg = cover !== '' && !imgBroken;
  const heat = item.recTags[0] ?? '';

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
        <div className="text-muted-foreground flex items-center gap-2 text-xs">
          {item.category !== '' && <span>{item.category}</span>}
          {item.score > 0 && (
            <span className="flex items-center gap-0.5">
              <Star className="size-3 text-amber-400" aria-hidden />
              {item.score.toFixed(1)}
            </span>
          )}
          {item.episodeCnt > 0 && (
            <span>{tf('common.episodeCount', { count: item.episodeCnt })}</span>
          )}
        </div>
        {item.description !== '' && (
          <p className="text-muted-foreground/80 line-clamp-2 text-xs leading-relaxed">
            {item.description}
          </p>
        )}
      </div>

      <div className="flex shrink-0 flex-col items-end gap-1">
        {heat !== '' && (
          <Badge variant="secondary" className="gap-1">
            <Flame className="size-3 text-orange-400" aria-hidden />
            {heat}
          </Badge>
        )}
        {item.publishTime > 0 && (
          <span className="text-muted-foreground text-xs tabular-nums">
            {formatPublishTime(item.publishTime)}
          </span>
        )}
      </div>
    </article>
  );
}

/** "20261003" → "10/3"。 */
function formatDateChip(d: string): string {
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
