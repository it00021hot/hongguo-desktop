/** 日历行卡：封面/信息/上线状态，未上线给红色预约胶囊。 */
import { useState } from 'react';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { BellRing, Loader2, Star, Tv } from 'lucide-react';
import { toast } from 'sonner';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { rank as rankApi } from '@/service/commands';
import { useWebCover } from '@/service/queries';
import { isRenderableCover } from '@/utils/cover';
import { t, tf } from '@/locales';
import type { CalendarItem } from '@/service/schema';

/** 日历一行：封面 | 标题/分类/简介 | 上线状态与热度 | 预约。 */
export function CalendarRow({
  item,
  onSelect,
}: {
  item: CalendarItem;
  onSelect: (item: CalendarItem) => void;
}) {
  const qc = useQueryClient();
  const { data: webCover } = useWebCover(item.cover);
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

/** unix 秒 → "MM-DD HH:mm"（本地时区）。 */
function formatPublishTime(sec: number): string {
  const dt = new Date(sec * 1000);
  if (Number.isNaN(dt.getTime())) return '';
  const pad = (n: number) => String(n).padStart(2, '0');
  return `${pad(dt.getMonth() + 1)}-${pad(dt.getDate())} ${pad(dt.getHours())}:${pad(dt.getMinutes())}`;
}
