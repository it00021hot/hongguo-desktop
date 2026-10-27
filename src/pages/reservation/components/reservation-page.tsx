import { useEffect, useRef, useState } from 'react';
import { useNavigate } from '@tanstack/react-router';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { BellRing, CalendarClock, LogIn, Play, Star, Tv } from 'lucide-react';
import { toast } from 'sonner';
import { Badge } from '@/components/ui/badge';
import { Spinner } from '@/components/ui/spinner';
import { Button } from '@/components/ui/button';
import { ErrorState } from '@/components/common/error-state';
import {
  Empty,
  EmptyDescription,
  EmptyHeader,
  EmptyMedia,
  EmptyTitle,
} from '@/components/ui/empty';
import { ListSearch } from '@/components/common/list-search';
import { matchListQuery } from '@/utils/list-filter';
import { SENTINEL_ROOT_MARGIN_PX } from '@/utils/infinite-scroll';
import { cn } from '@/lib/utils';
import { Skeleton } from '@/components/ui/skeleton';
import { LoginDialog } from '@/pages/settings/components/login-dialog';
import { rank as rankApi } from '@/service/commands';
import { BatchBar, ManageToggle, PickDot } from '@/components/common/batch-manage';
import { useBatchSelect } from '@/hooks/use-batch-select';
import { useScrollRestore, useSessionState } from '@/hooks/use-scroll-restore';
import { keys, useAccount, useReservations, useReservationsDelete } from '@/service/queries';
import { enhancedCover, isRenderableCover } from '@/utils/cover';
import { t, tf, locale } from '@/locales';
import { usePlaySeries } from '@/hooks/use-play-series';
import type { CalendarItem } from '@/service/schema';

/**
 * 我的预约：已上线 / 待上线两个 tab（带计数），
 * 卡片 = 封面（左下上线日期角标 / 右下集数角标）+ 标题 + 简介一行 +
 * 分类标签。卡片一律进详情（排行榜预约榜同款口径，未上线剧落地
 * 「即将上线」降级视图）；行尾动作列已上线 = 播放 + 详情，
 * 未上线 = 取消预约。
 *
 * 列表单页懒翻页：首屏一页即回、角标计数用响应下发
 * 的 total，滚到底再翻下一页——此前一次性串行拉全，预约上百部时
 * 「已上线」要等 5–8 秒才出来（issue #11）。搜索态在后台把剩余页补齐
 * （对全量条目过滤）。
 *
 * 预约列表接口需要登录，匿名返回空表——未登录时给登录入口而不是空列表。
 */

export function ReservationPage() {
  // tab/搜索词会话级保留：进详情再返回，停在离开时的 tab（此前回落到
  // 已上线 tab，是返回体验的头号槽点）
  const [online, setOnline] = useSessionState('hongguo.reservation.tab', true);
  const [query, setQuery] = useSessionState('hongguo.reservation.query', '');
  const { data: account } = useAccount();
  const onlineQ = useReservations(true);
  const offlineQ = useReservations(false);
  const current = online ? onlineQ : offlineQ;
  // 批量管理（subscribe_delete）：多选后一请求删除
  const batch = useBatchSelect();
  const batchDelete = useReservationsDelete();
  // 标题搜索（对齐官方客户端「搜索预约的剧」）：客户端过滤，条目自带标题
  const shown = current.items.filter((item) => matchListQuery(query, item.title, item.seriesId));
  // 搜索要对全量条目过滤：搜索态下后台持续翻页补齐（失败即停，重试走
  // 列表自身的失效/重挂）；普通浏览仍是一页懒翻
  useEffect(() => {
    if (query === '' || current.error || current.isLoading || !current.hasMore) return;
    if (current.isFetchingMore) return;
    current.loadMore();
  });
  // 滚近底部（400px 提前量）懒翻下一页；常驻哨兵（同日历/找剧口径）
  const sentinelRef = useRef<HTMLDivElement | null>(null);
  useEffect(() => {
    const el = sentinelRef.current;
    if (!el) return;
    const observer = new IntersectionObserver(
      (entries) => {
        if (
          entries.some((e) => e.isIntersecting) &&
          current.hasMore &&
          !current.isFetchingMore &&
          shown.length > 0
        ) {
          current.loadMore();
        }
      },
      { rootMargin: `${SENTINEL_ROOT_MARGIN_PX}px` },
    );
    observer.observe(el);
    return () => observer.disconnect();
  });
  // 返回保留浏览位置（页面滚在 app-shell 的 #content）
  useScrollRestore(
    'hongguo.reservation.scroll',
    () => document.getElementById('content'),
    !current.isLoading && shown.length > 0,
  );

  // 角标计数：响应下发的 total（首页即真值），缺失时条数兜底；
  // 数据未到显示骨架点，不让 0 冒充「没有预约」
  const tabs: { key: boolean; label: string; count: number | null }[] = [
    { key: true, label: t('reservation.tab.online'), count: onlineQ.count },
    { key: false, label: t('reservation.tab.offline'), count: offlineQ.count },
  ];

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
        {account != null && (
          <ManageToggle managing={batch.managing} onEnter={batch.enter} onExit={batch.exit} />
        )}
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
        <ErrorState
          message={t('reservation.loadFailed')}
          detail={current.error.message}
          onRetry={() => void current.refetch()}
        />
      ) : current.items.length === 0 ? (
        <Empty className="py-16">
          <EmptyHeader>
            <EmptyMedia>
              <BellRing className="size-8 opacity-40" aria-hidden />
            </EmptyMedia>
            <EmptyTitle className="text-muted-foreground text-sm font-normal">
              {t('reservation.empty')}
            </EmptyTitle>
            <EmptyDescription className="text-xs opacity-70">
              {t('reservation.emptyHint')}
            </EmptyDescription>
          </EmptyHeader>
        </Empty>
      ) : (
        <>
          <div className="flex flex-col gap-2">
            {shown.map((item) => (
              <ReservationCard
                key={item.seriesId}
                item={item}
                managing={batch.managing}
                picked={batch.selected.has(item.seriesId)}
                onPick={() => batch.toggle(item.seriesId)}
              />
            ))}
          </div>
          {/* 懒翻页：哨兵常驻列表尾（rootMargin 提前量），翻页中给反馈 */}
          <div ref={sentinelRef} className="h-px" aria-hidden />
          {current.isFetchingMore && (
            <p className="text-muted-foreground flex items-center justify-center gap-2 py-2 text-sm">
              <Spinner aria-hidden />
              {t('feed.loadingMore')}
            </p>
          )}
          {batch.managing && (
            <BatchBar
              count={batch.selected.size}
              total={shown.length}
              deleting={batchDelete.isPending}
              deleteLabel={t('reservation.cancel')}
              confirmTitle={t('reservation.batchCancelTitle')}
              onToggleAll={() => batch.toggleAll(shown.map((i) => i.seriesId))}
              onDelete={() => {
                const count = batch.selected.size;
                batchDelete.mutate(
                  {
                    itemIds: shown
                      .filter((i) => batch.selected.has(i.seriesId))
                      .map((i) => i.seriesId),
                    isOnline: online,
                  },
                  {
                    onSuccess: () => {
                      toast.success(tf('batch.done', { count }));
                      batch.exit();
                    },
                    onError: (e: Error) =>
                      toast.error(tf('batch.partialFail', { error: e.message })),
                  },
                );
              }}
            />
          )}
        </>
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
          void qc.invalidateQueries({ queryKey: keys.reservationsRoot });
        }}
      />
    </div>
  );
}

/** 预约卡：封面（角标）+ 标题/简介/标签 + 上线信息 / 取消预约。
 *  卡片一律进详情（排行榜预约榜同款口径：未上线剧落地「即将上线」
 *  降级视图，行内档案快照 prefill）；管理模式下点击卡 = 切换选中。 */
function ReservationCard({
  item,
  managing,
  picked,
  onPick,
}: {
  item: CalendarItem;
  managing: boolean;
  picked: boolean;
  onPick: () => void;
}) {
  const navigate = useNavigate();
  const playSeries = usePlaySeries();
  const qc = useQueryClient();
  const webCover = enhancedCover(item.cover);
  const sourceRenderable = isRenderableCover(item.cover);
  const cover = webCover ?? (sourceRenderable ? item.cover : '');
  const [brokenFor, setBrokenFor] = useState('');
  const imgBroken = brokenFor !== '' && brokenFor === cover;
  const showImg = cover !== '' && !imgBroken;

  const cancel = useMutation({
    mutationFn: () => rankApi.reserve(item.seriesId, false),
    onSuccess: () => {
      toast.success(t('reservation.cancelled'));
      void qc.invalidateQueries({ queryKey: keys.reservationsRoot });
    },
  });

  // 详情带上行内档案快照：未上线剧详情解析分集必然失败，prefill 支撑
  // 详情页渲染「即将上线」降级视图（而非整页报错）
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

  const publishDate = item.publishTime > 0 ? formatOnlineDate(item.publishTime) : '';
  const tags = item.recTags.filter((x) => x !== '');
  const select = () => (managing ? onPick() : openDetail());

  return (
    <article
      className={cn(
        'bg-card hover:border-foreground/30 flex items-stretch gap-4 overflow-hidden rounded-xl border p-3 transition-colors hover:shadow-md',
        picked && 'border-primary/60 bg-primary/5',
      )}
    >
      {managing && <PickDot checked={picked} onToggle={onPick} />}
      <button
        type="button"
        className="bg-muted relative aspect-[3/4] w-[92px] shrink-0 cursor-pointer overflow-hidden rounded-lg text-left"
        onClick={select}
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
        {/* 左下：上线日期角标 */}
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
            onClick={select}
          >
            {item.title}
          </button>
          {item.score > 0 && (
            <span className="text-muted-foreground flex shrink-0 items-center gap-0.5 text-xs">
              <Star className="text-star size-3" aria-hidden />
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

      {!managing && (
        <div className="flex shrink-0 flex-col items-stretch justify-center gap-1.5">
          {item.isOnline ? (
            <>
              <Button
                size="sm"
                variant="outline"
                className="gap-1"
                onClick={() => playSeries(item.seriesId)}
              >
                <Play className="size-3.5" aria-hidden />
                {t('player.play')}
              </Button>
              <button
                type="button"
                className="text-muted-foreground hover:text-foreground cursor-pointer text-center text-xs transition-colors"
                onClick={openDetail}
              >
                {t('rank.detail')}
              </button>
            </>
          ) : (
            <Button
              size="sm"
              variant="outline"
              disabled={cancel.isPending}
              onClick={() => cancel.mutate()}
            >
              {cancel.isPending ? (
                <Spinner aria-hidden />
              ) : (
                <BellRing className="size-4" aria-hidden />
              )}
              {t('reservation.cancel')}
            </Button>
          )}
        </div>
      )}
    </article>
  );
}

/** UTC 毫秒时间戳 → "9月27日" / "Sep 27"（角标用，随界面语言）。 */
function formatOnlineDate(ms: number): string {
  const dt = new Date(ms);
  if (Number.isNaN(dt.getTime())) return '';
  return new Intl.DateTimeFormat(locale() === 'zh-CN' ? 'zh-CN' : 'en-US', {
    month: 'short',
    day: 'numeric',
  }).format(dt);
}

/** UTC 毫秒时间戳 → "10-07 12:00"（待上线精确时间）。 */
function formatPublishDateTime(ms: number): string {
  const dt = new Date(ms);
  if (Number.isNaN(dt.getTime())) return '';
  const pad = (n: number) => String(n).padStart(2, '0');
  return `${pad(dt.getMonth() + 1)}-${pad(dt.getDate())} ${pad(dt.getHours())}:${pad(dt.getMinutes())}`;
}
