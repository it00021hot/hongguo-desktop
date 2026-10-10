/** 日历行卡：与排行榜行同骨架（SeriesRowCard）——卡片一律进详情，
 *  行尾动作列按上线状态分：未上线 = 预约胶囊；已上线 = 播放 + 详情。 */
import { useState } from 'react';
import { useNavigate } from '@tanstack/react-router';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { BellRing, Loader2, Play, Star } from 'lucide-react';
import { toast } from 'sonner';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { SeriesRowCard } from '@/components/series-row-card';
import { rank as rankApi } from '@/service/commands';
import { usePlaySeries } from '@/hooks/use-play-series';
import { t, tf } from '@/locales';
import type { CalendarItem } from '@/service/schema';

/** 日历一行：封面 | 标题/分类/简介 | 上线状态与热度 | 预约/播放。 */
export function CalendarRow({ item }: { item: CalendarItem }) {
  const navigate = useNavigate();
  const playSeries = usePlaySeries();
  const qc = useQueryClient();
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

  // 卡片一律进详情（排行榜预约榜同款口径）：未上线剧详情解析分集必然
  // 失败，带上行内档案快照（recTags 当题材标签）撑起「即将上线」降级视图
  const openDetail = () =>
    void navigate({
      to: '/detail',
      search: {
        seriesId: item.seriesId,
        title: item.title,
        cover: item.cover,
        tags: item.recTags.join(','),
        desc: item.description,
      },
    });

  // 官方同款：预约人数红色醒目，后跟分类/评分/集数
  const heat = item.recTags[0] ?? '';

  return (
    <SeriesRowCard
      cover={item.cover}
      title={item.title}
      onOpen={openDetail}
      titleExtra={
        item.isOnline ? (
          <Badge variant="success" className="shrink-0 text-[10px]">
            {t('newDrama.online')}
          </Badge>
        ) : (
          <Badge variant="secondary" className="shrink-0 text-[10px]">
            {t('newDrama.upcoming')}
          </Badge>
        )
      }
      metaLine={
        heat !== '' || item.category !== '' || item.score > 0 || item.episodeCnt > 0 ? (
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
        ) : undefined
      }
      description={item.description}
      trailing={
        item.publishTime > 0 ? (
          <span className="text-muted-foreground text-xs tabular-nums">
            {formatPublishTime(item.publishTime)}
          </span>
        ) : undefined
      }
      actions={
        item.isOnline ? (
          <>
            <Button
              size="sm"
              variant="outline"
              className="shrink-0 gap-1"
              onClick={(e) => {
                e.stopPropagation();
                playSeries(item.seriesId);
              }}
            >
              <Play className="size-3.5" aria-hidden />
              {t('player.play')}
            </Button>
            <button
              type="button"
              className="text-muted-foreground hover:text-foreground cursor-pointer text-center text-xs transition-colors"
              onClick={(e) => {
                e.stopPropagation();
                openDetail();
              }}
            >
              {t('rank.detail')}
            </button>
          </>
        ) : (
          // 播放器之外的按钮走项目主题（primary 单色），不模仿 hgplayer 品牌红
          <Button
            size="sm"
            variant={reserved ? 'secondary' : 'default'}
            disabled={reserved || reserve.isPending}
            className="rounded-full"
            onClick={(e) => {
              e.stopPropagation();
              reserve.mutate();
            }}
          >
            {reserve.isPending ? (
              <Loader2 className="size-4 animate-spin" aria-hidden />
            ) : (
              <BellRing className="size-4" aria-hidden />
            )}
            {reserved ? t('reservation.reserved') : t('reservation.action')}
          </Button>
        )
      }
    />
  );
}

/** unix 秒 → "MM-DD HH:mm"（本地时区）。 */
function formatPublishTime(sec: number): string {
  const dt = new Date(sec * 1000);
  if (Number.isNaN(dt.getTime())) return '';
  const pad = (n: number) => String(n).padStart(2, '0');
  return `${pad(dt.getMonth() + 1)}-${pad(dt.getDate())} ${pad(dt.getHours())}:${pad(dt.getMinutes())}`;
}
