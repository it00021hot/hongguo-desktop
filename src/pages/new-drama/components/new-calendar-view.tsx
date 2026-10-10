/** 上新日历视图：日期条（今天/周几换算）+ 当日上新列表。 */
import { useEffect, useRef, useState } from 'react';
import { Loader2 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Skeleton } from '@/components/ui/skeleton';
import { RefreshShade } from '@/components/refresh-shade';
import { SkeletonRows } from '@/components/skeletons';
import { useNewCalendar } from '@/service/queries';
import { useSessionState } from '@/hooks/use-scroll-restore';
import { t } from '@/locales';
import { CalendarRow } from './calendar-row';

/** 上新日历：日期条 + 当日上新列表（含未上线）。 */
export function NewCalendarView() {
  // 选中日期会话级保留：进详情再回来还停在离开的那天
  const [date, setDate] = useSessionState('hongguo.new.calDate', '');
  const { data, isLoading, error, isFetching, refetch } = useNewCalendar(date);
  // 首次拿到日期列表后选中默认日（空串 = 默认日，这里显式化便于高亮）
  const active = date === '' ? (data?.defaultDate ?? '') : date;
  const dates = data?.dates ?? [];

  // 渐进渲染（历史页同款）：热门日子几十条一次性挂载照样卡，首批 12 行
  // 秒出，滚近底部续载；切日期重置批量（渲染期调整 state 的官方模式）
  const items = data?.items ?? [];
  const [visibleCount, setVisibleCount] = useState(12);
  const [prevDate, setPrevDate] = useState('');
  if (prevDate !== date) {
    setPrevDate(date);
    setVisibleCount(12);
  }
  const sentinelRef = useRef<HTMLDivElement | null>(null);
  useEffect(() => {
    const el = sentinelRef.current;
    if (!el) return;
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((e) => e.isIntersecting)) {
          setVisibleCount((n) => (n < items.length ? n + 24 : n));
        }
      },
      { rootMargin: '600px' },
    );
    observer.observe(el);
    return () => observer.disconnect();
  });

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-4">
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

      {/* 列表滚动区（日期条下方唯一会滚的地方）：切日期用 key 重挂载
          归零滚动——新的一天从第一条看起，不停在旧日期的滚动位置 */}
      <div key={date} className="min-h-0 flex-1 scrollbar-thin overflow-y-auto">
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
              {items.slice(0, visibleCount).map((item) => (
                <CalendarRow key={item.seriesId} item={item} />
              ))}
              {/* 续载哨兵：滚近底部（600px 提前量）继续渲染下一批 */}
              {visibleCount < items.length && <div ref={sentinelRef} className="h-px" />}
              {/* 当日条目穷尽：hgplayer 同款「没有更多了」尾标 */}
              {!isLoading && items.length > 0 && visibleCount >= items.length && (
                <p className="text-muted-foreground py-2 text-center text-sm">{t('feed.end')}</p>
              )}
            </div>
          )}
        </RefreshShade>
      </div>
    </div>
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
