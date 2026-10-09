import { useState } from 'react';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { BellRing, CalendarClock, Loader2, LogIn, Play, Star, Tv } from 'lucide-react';
import { toast } from 'sonner';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { ListSearch } from '@/components/list-search';
import { matchListQuery } from '@/utils/list-filter';
import { cn } from '@/lib/utils';
import { Skeleton } from '@/components/ui/skeleton';
import { LoginDialog } from '@/features/settings/components/login-dialog';
import { rank as rankApi } from '@/service/commands';
import {
  RESERVATIONS_KEY_ROOT,
  useAccount,
  useReservations,
  useWebCover,
} from '@/service/queries';
import { isRenderableCover } from '@/utils/cover';
import { t, tf, locale } from '@/locales';
import { usePlaySeries } from '@/hooks/use-play-series';
import type { CalendarItem } from '@/service/schema';

/**
 * 我的预约（对齐 hgplayer 1.1.3）：已上线 / 待上线两个 tab（带计数），
 * 卡片 = 封面（左下上线日期角标 / 右下集数角标）+ 标题 + 简介一行 +
 * 分类标签。待上线条目可直接取消预约；已上线点击进详情。
 *
 * 预约列表接口需要登录，匿名返回空表——未登录时给登录入口而不是空列表。
 */

export function ReservationPage() {
  const [online, setOnline] = useState(true);
  const [query, setQuery] = useState('');
  const { data: account } = useAccount();
  const onlineQ = useReservations(true);
  const offlineQ = useReservations(false);
  const current = online ? onlineQ : offlineQ;
  // 标题搜索（对齐参考端 v1.1.6「搜索预约的剧」）：客户端过滤，条目自带标题
  const shown = (current.data?.items ?? []).filter((item) =>
    matchListQuery(query, item.title, item.seriesId),
  );

  // 角标计数：total 优先、条数兜底（后端已翻页拉全并兜底，这里双保险；
  // 数据未到显示骨架点，不让 0 冒充「没有预约」）
  const tabs: { key: boolean; label: string; count: number | null }[] = [
    {
      key: true,
      label: t('reservation.tab.online'),
      count: onlineQ.data ? Math.max(onlineQ.data.onlineTotal, onlineQ.data.items.length) : null,
    },
    {
      key: false,
      label: t('reservation.tab.offline'),
      count: offlineQ.data
        ? Math.max(offlineQ.data.offlineTotal, offlineQ.data.items.length)
        : null,
    },
  ];

  const playSeries = usePlaySeries();
  const handleSelect = (item: CalendarItem) => playSeries(item.seriesId);

  return (
    <div className="flex flex-col gap-4 p-4">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="flex items-center gap-2">
          {tabs.map((tab) => (
            <button
              key={String(tab.key)}
              type="button"
              onClick={() => setOnline(tab.key)}
              className={cn(
                'rounded-full px-4 py-1.5 text-sm transition-colors',
                tab.key === online
                  ? 'bg-primary text-primary-foreground font-medium'
                  : 'bg-muted text-muted-foreground hover:text-foreground',
              )}
            >
              {tab.label}
              {tab.count === null ? (
                <Skeleton className="ml-1.5 inline-block h-3 w-5 align-middle" />
              ) : (
                <span className="ml-1.5 tabular-nums opacity-80">{tab.count}</span>
              )}
            </button>
          ))}
        </div>
        <ListSearch
          value={query}
          onChange={setQuery}
          placeholder={t('reservation.searchPlaceholder')}
        />
      </div>

      {account == null ? (
        <NotLoggedIn />
      ) : current.isLoading ? (
        <div className="flex flex-col gap-3">
          {Array.from({ length: 4 }, (_, i) => (
            <Skeleton key={i} className="h-28 rounded-xl" />
          ))}
        </div>
      ) : current.error ? (
        <div className="text-muted-foreground flex flex-col items-center gap-3 py-16">
          <p>{t('reservation.loadFailed')}</p>
          <p className="text-destructive text-xs">{current.error.message}</p>
          <Button variant="outline" size="sm" onClick={() => void current.refetch()}>
            <Loader2 className="mr-1 size-4 animate-spin" aria-hidden />
            {t('feed.retry')}
          </Button>
        </div>
      ) : (current.data?.items.length ?? 0) === 0 ? (
        <div className="text-muted-foreground flex flex-col items-center gap-2 py-16">
          <BellRing className="size-8 opacity-40" aria-hidden />
          <p className="text-sm">{t('reservation.empty')}</p>
          <p className="text-xs opacity-70">{t('reservation.emptyHint')}</p>
        </div>
      ) : (
        <div className="flex flex-col gap-2">
          {shown.map((item) => (
            <ReservationCard key={item.seriesId} item={item} onSelect={handleSelect} />
          ))}
        </div>
      )}
    </div>
  );
}

/** 未登录态：说明 + 登录弹窗入口（匿名调预约接口只能拿到空表）。 */
function NotLoggedIn() {
  const [open, setOpen] = useState(false);
  const qc = useQueryClient();
  const { refetch: refetchAccount } = useAccount();
  return (
    <div className="text-muted-foreground flex flex-col items-center gap-3 py-20">
      <LogIn className="size-8 opacity-40" aria-hidden />
      <p className="text-sm">{t('reservation.loginRequired')}</p>
      <Button size="sm" onClick={() => setOpen(true)}>
        <LogIn className="size-4" aria-hidden />
        {t('settings.loginAction')}
      </Button>
      <LoginDialog
        open={open}
        onOpenChange={setOpen}
        onSuccess={() => {
          // 登录成功：刷登录态并重拉两个 tab 的预约列表
          void refetchAccount();
          void qc.invalidateQueries({ queryKey: RESERVATIONS_KEY_ROOT });
        }}
      />
    </div>
  );
}

/** 预约卡：封面（角标）+ 标题/简介/标签 + 上线信息 / 取消预约。 */
function ReservationCard({
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

  const cancel = useMutation({
    mutationFn: () => rankApi.reserve(item.seriesId, false),
    onSuccess: () => {
      toast.success(t('reservation.cancelled'));
      void qc.invalidateQueries({ queryKey: RESERVATIONS_KEY_ROOT });
    },
    onError: (e: Error) => toast.error(e.message),
  });

  const publishDate = item.publishTime > 0 ? formatOnlineDate(item.publishTime) : '';
  const tags = item.recTags.filter((x) => x !== '');

  return (
    <article className="bg-card hover:border-foreground/30 flex items-stretch gap-4 overflow-hidden rounded-xl border p-3 transition-colors hover:shadow-md">
      <button
        type="button"
        className="bg-muted relative aspect-[3/4] w-[92px] shrink-0 cursor-pointer overflow-hidden rounded-lg text-left"
        onClick={() => onSelect(item)}
        aria-label={item.title}
      >
        {showImg ? (
          <img
            src={cover}
            alt={item.title}
            loading="lazy"
            className="size-full object-cover"
            onError={() => setBrokenFor(cover)}
          />
        ) : (
          <span className="text-muted-foreground grid size-full place-items-center">
            <Tv className="size-6" />
          </span>
        )}
        {/* 左下：上线日期角标（hgplayer 同款位置） */}
        {publishDate !== '' && (
          <span className="absolute bottom-0 left-0 bg-black/65 px-1.5 py-0.5 text-[10px] text-white">
            {item.isOnline
              ? tf('reservation.onlineBadge', { date: publishDate })
              : tf('reservation.upcomingBadge', { date: publishDate })}
          </span>
        )}
      </button>

      <div className="flex min-w-0 flex-1 flex-col justify-center gap-1">
        <div className="flex items-center gap-2">
          <button
            type="button"
            className="cursor-pointer truncate text-left text-sm font-semibold hover:underline"
            title={item.title}
            onClick={() => onSelect(item)}
          >
            {item.title}
          </button>
          {item.score > 0 && (
            <span className="text-muted-foreground flex shrink-0 items-center gap-0.5 text-xs">
              <Star className="size-3 text-amber-400" aria-hidden />
              {item.score.toFixed(1)}
            </span>
          )}
        </div>
        {item.description !== '' && (
          <p className="text-muted-foreground line-clamp-1 text-xs">{item.description}</p>
        )}
        <div className="mt-1 flex flex-wrap items-center gap-1.5">
          {item.category !== '' && (
            <Badge variant="secondary" className="text-[10px]">
              {item.category}
            </Badge>
          )}
          {tags.slice(0, 3).map((tag) => (
            <Badge key={tag} variant="outline" className="text-[10px]">
              {tag}
            </Badge>
          ))}
          {item.publishTime > 0 && !item.isOnline && (
            <span className="text-muted-foreground flex items-center gap-1 text-xs">
              <CalendarClock className="size-3" aria-hidden />
              {tf('reservation.publishAt', {
                time: formatPublishDateTime(item.publishTime),
              })}
            </span>
          )}
        </div>
      </div>

      <div className="flex shrink-0 flex-col items-end justify-center gap-2">
        {item.isOnline ? (
          <Button size="sm" variant="outline" onClick={() => onSelect(item)}>
            <Play className="size-4" aria-hidden />
            {t('reservation.watch')}
          </Button>
        ) : (
          <Button
            size="sm"
            variant="outline"
            disabled={cancel.isPending}
            onClick={() => cancel.mutate()}
          >
            {cancel.isPending ? (
              <Loader2 className="size-4 animate-spin" aria-hidden />
            ) : (
              <BellRing className="size-4" aria-hidden />
            )}
            {t('reservation.cancel')}
          </Button>
        )}
      </div>
    </article>
  );
}

/** unix 秒 → "9月27日" / "Sep 27"（角标用，随界面语言）。 */
function formatOnlineDate(sec: number): string {
  const dt = new Date(sec * 1000);
  if (Number.isNaN(dt.getTime())) return '';
  return new Intl.DateTimeFormat(locale() === 'zh-CN' ? 'zh-CN' : 'en-US', {
    month: 'short',
    day: 'numeric',
  }).format(dt);
}

/** unix 秒 → "10-07 12:00"（待上线精确时间）。 */
function formatPublishDateTime(sec: number): string {
  const dt = new Date(sec * 1000);
  if (Number.isNaN(dt.getTime())) return '';
  const pad = (n: number) => String(n).padStart(2, '0');
  return `${pad(dt.getMonth() + 1)}-${pad(dt.getDate())} ${pad(dt.getHours())}:${pad(dt.getMinutes())}`;
}
