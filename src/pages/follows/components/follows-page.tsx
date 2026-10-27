import { useState } from 'react';
import { useNavigate } from '@tanstack/react-router';
import { useQueryClient } from '@tanstack/react-query';
import { BellRing, CheckCheck, LogIn, Play, RefreshCw, Tv } from 'lucide-react';
import { toast } from 'sonner';
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
import { Skeleton } from '@/components/ui/skeleton';
import { cn } from '@/lib/utils';
import { LoginDialog } from '@/pages/settings/components/login-dialog';
import { FollowButton } from '@/components/common/follow-button';
import { BatchBar, ManageToggle, PickDot } from '@/components/common/batch-manage';
import { useBatchSelect } from '@/hooks/use-batch-select';
import { useSessionState } from '@/hooks/use-scroll-restore';
import { useUiStore } from '@/stores/ui';
import { usePlaySeries } from '@/hooks/use-play-series';
import {
  keys,
  unreadNotices,
  useAccount,
  useFollowCheck,
  useFollowNotices,
  useFollowNoticesMutations,
  useFollowToggle,
  useFollowUpdates,
} from '@/service/queries';
import { enhancedCover, isRenderableCover } from '@/utils/cover';
import { t, tf, locale } from '@/locales';
import type { FollowNotice, FollowUpdateItem } from '@/service/schema';

/**
 * 追更页：「追更的剧 / 更新提醒」双 tab。
 *
 * - 追更的剧：游标翻页列表，状态行「已追平 / N 集未看 · 连载中/已完结」；
 *   管理模式批量取消追更（逐条 do_action 35，player 同款）；
 * - 更新提醒：本地通知（追更更新 / 预约上线），点条目跳播放并标记已读；
 *   「检查更新」手动触发一轮（定时器已在后端跑，按钮是 player 同款的
 *   立即回音入口）。
 */

export function FollowsPage() {
  const [tab, setTab] = useSessionState<'follows' | 'notices'>('hongguo.follows.tab', 'follows');
  const { data: account } = useAccount();
  const noticesQ = useFollowNotices();
  const unread = unreadNotices(noticesQ.data);

  const tabs: { key: 'follows' | 'notices'; label: string; badge?: number }[] = [
    { key: 'follows', label: t('follow.tabFollows') },
    { key: 'notices', label: t('follow.tabNotices'), badge: unread },
  ];

  return (
    <div className="flex flex-col gap-4 p-4">
      <div className="flex flex-wrap items-center gap-2">
        {tabs.map((item) => (
          <button
            key={item.key}
            type="button"
            onClick={() => setTab(item.key)}
            className={cn(
              'rounded-full px-4 py-1.5 text-sm transition-colors',
              item.key === tab
                ? 'bg-primary text-primary-foreground font-medium'
                : 'bg-muted text-muted-foreground hover:text-foreground',
            )}
          >
            {item.label}
            {item.badge !== undefined && item.badge > 0 && (
              <span className="bg-destructive ml-1.5 rounded-full px-1.5 py-0.5 text-[10px] leading-none text-white tabular-nums">
                {item.badge}
              </span>
            )}
          </button>
        ))}
      </div>

      {account == null ? (
        <NotLoggedIn />
      ) : tab === 'follows' ? (
        <FollowsTab />
      ) : (
        <NoticesTab notices={noticesQ.data ?? []} />
      )}
    </div>
  );
}

/** 追更的剧：无限翻页列表 + 管理模式批量取消。 */
function FollowsTab() {
  const list = useFollowUpdates();
  const batch = useBatchSelect();
  const toggle = useFollowToggle();
  const items = list.data?.pages.flatMap((p) => p.items) ?? [];
  const [cancelling, setCancelling] = useState(false);

  if (list.isLoading) {
    return (
      <div className="flex flex-col gap-3">
        {Array.from({ length: 4 }, (_, i) => (
          <Skeleton key={i} className="h-28 rounded-xl" />
        ))}
      </div>
    );
  }
  if (list.error) {
    return (
      <ErrorState
        message={t('follow.loadFailed')}
        detail={list.error.message}
        onRetry={() => void list.refetch()}
      />
    );
  }
  if (items.length === 0) {
    return (
      <Empty className="py-16">
        <EmptyHeader>
          <EmptyMedia>
            <Tv className="size-8 opacity-40" aria-hidden />
          </EmptyMedia>
          <EmptyTitle className="text-muted-foreground text-sm font-normal">
            {t('follow.empty')}
          </EmptyTitle>
          <EmptyDescription className="text-xs opacity-70">
            {t('follow.emptyHint')}
          </EmptyDescription>
        </EmptyHeader>
      </Empty>
    );
  }

  const cancelPicked = () => {
    const picked = items.filter((i) => batch.selected.has(i.seriesId));
    if (picked.length === 0 || cancelling) return;
    setCancelling(true);
    // player 同款：逐条 do_action 35（服务端无批量端点），成功一项去一项
    let done = 0;
    let failed = 0;
    let lastError = '';
    const next = () => {
      if (done + failed === picked.length) {
        setCancelling(false);
        if (failed > 0) toast.error(tf('batch.partialFail', { error: lastError }));
        else toast.success(tf('batch.done', { count: done }));
        if (failed === 0) batch.exit();
      }
    };
    for (const item of picked) {
      toggle.mutate(
        { seriesId: item.seriesId, follow: false },
        {
          onSuccess: () => {
            done += 1;
            next();
          },
          onError: (e: Error) => {
            failed += 1;
            lastError = e.message;
            next();
          },
        },
      );
    }
  };

  return (
    <>
      <div className="flex items-center justify-between">
        <ManageToggle managing={batch.managing} onEnter={batch.enter} onExit={batch.exit} />
      </div>
      <div className="flex flex-col gap-2">
        {items.map((item) => (
          <FollowCard
            key={item.seriesId}
            item={item}
            managing={batch.managing}
            picked={batch.selected.has(item.seriesId)}
            onPick={() => batch.toggle(item.seriesId)}
          />
        ))}
      </div>
      {list.hasNextPage && (
        <div className="flex justify-center">
          <Button
            variant="outline"
            size="sm"
            disabled={list.isFetchingNextPage}
            onClick={() => void list.fetchNextPage()}
          >
            {list.isFetchingNextPage ? <Spinner aria-hidden /> : t('follow.loadMore')}
          </Button>
        </div>
      )}
      {!list.hasNextPage && items.length > 0 && (
        <p className="text-muted-foreground text-center text-xs">{t('follow.noMore')}</p>
      )}
      {batch.managing && (
        <BatchBar
          count={batch.selected.size}
          total={items.length}
          deleting={cancelling}
          deleteLabel={t('follow.unfollow')}
          confirmTitle={t('follow.batchCancelTitle')}
          onToggleAll={() => batch.toggleAll(items.map((i) => i.seriesId))}
          onDelete={cancelPicked}
        />
      )}
    </>
  );
}

/** 追更卡：封面 + 标题 + 状态行（已追平 / N 集未看 · 连载中/已完结）+ 追更钮。 */
function FollowCard({
  item,
  managing,
  picked,
  onPick,
}: {
  item: FollowUpdateItem;
  managing: boolean;
  picked: boolean;
  onPick: () => void;
}) {
  const navigate = useNavigate();
  const playSeries = usePlaySeries();
  const webCover = enhancedCover(item.cover);
  const sourceRenderable = isRenderableCover(item.cover);
  const cover = webCover ?? (sourceRenderable ? item.cover : '');
  const [brokenFor, setBrokenFor] = useState('');
  const imgBroken = brokenFor !== '' && brokenFor === cover;
  const showImg = cover !== '' && !imgBroken;

  const openDetail = () =>
    void navigate({
      to: '/detail',
      search: { seriesId: item.seriesId, title: item.title, cover: item.cover },
    });
  const select = () => (managing ? onPick() : openDetail());

  // 状态行（player p() 同款拼装）：追平/未看集数 + 连载中(更新频率)/已完结
  const remain = item.episodeCnt - item.latestWatchedVidIndex;
  const progress =
    item.latestWatchedVidIndex > 0
      ? remain <= 0
        ? t('follow.caughtUp')
        : tf('follow.unwatched', { count: remain })
      : '';
  const serial = item.finished ? t('follow.finished') : t('follow.serializing');
  const status = [progress, serial].filter(Boolean).join(' · ');

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
      </button>

      <div className="flex min-w-0 flex-1 flex-col justify-center gap-1">
        <button
          type="button"
          className="cursor-pointer truncate text-left text-sm font-semibold hover:underline"
          title={item.title}
          onClick={select}
        >
          {item.title}
        </button>
        {status !== '' && <p className="text-muted-foreground text-xs">{status}</p>}
        {item.episodeCnt > 0 && (
          <p className="text-muted-foreground text-xs">
            {tf('follow.episodeProgress', {
              watched: item.latestWatchedVidIndex,
              total: item.episodeCnt,
            })}
          </p>
        )}
      </div>

      {!managing && (
        <div className="flex shrink-0 flex-col items-stretch justify-center gap-1.5">
          <Button
            size="sm"
            variant="outline"
            className="gap-1"
            onClick={() => playSeries(item.seriesId)}
          >
            <Play className="size-3.5" aria-hidden />
            {t('player.play')}
          </Button>
          {/* 已完结剧不再追更（无新集可等） */}
          {!item.finished && <FollowButton seriesId={item.seriesId} className="w-full" />}
        </div>
      )}
    </article>
  );
}

/** 更新提醒：检查更新 + 清空 + 通知列表（点条目跳播放并标已读）。 */
function NoticesTab({ notices }: { notices: FollowNotice[] }) {
  const qc = useQueryClient();
  const playSeries = usePlaySeries();
  const { markRead, clear } = useFollowNoticesMutations();
  const check = useFollowCheck();
  // 打开 tab 即全部已读（player 同款：tab 切到 notice 时 $o()）
  const [readAll, setReadAll] = useState(false);
  const popupOn = useUiStore((s) => s.aibotPopupOn);
  const setPopupOn = useUiStore((s) => s.setAibotPopupOn);

  const open = (n: FollowNotice) => {
    if (!n.read) {
      void markRead.mutateAsync([n.id]).then(() => setReadAll(true));
    }
    void qc.invalidateQueries({ queryKey: keys.followNotices });
    playSeries(n.seriesId);
  };

  return (
    <div className="flex flex-col gap-3">
      <div className="flex items-center gap-2">
        <label className="flex cursor-pointer items-center gap-1.5 text-sm">
          <input
            type="checkbox"
            checked={popupOn}
            onChange={(e) => setPopupOn(e.target.checked)}
            className="accent-primary size-3.5"
          />
          {t('follow.popupOn')}
        </label>
        <Button
          size="sm"
          variant="outline"
          disabled={check.isPending}
          onClick={() =>
            check.mutate(undefined, {
              onSuccess: (count) =>
                toast.success(
                  count > 0 ? tf('follow.checkResult', { count }) : t('follow.checkNone'),
                ),
              onError: (e: Error) => toast.error(e.message),
            })
          }
        >
          {check.isPending ? (
            <Spinner aria-hidden />
          ) : (
            <RefreshCw className="size-3.5" aria-hidden />
          )}
          {check.isPending ? t('follow.checking') : t('follow.checkUpdate')}
        </Button>
        {notices.length > 0 && (
          <Button
            size="sm"
            variant="ghost"
            disabled={clear.isPending}
            onClick={() =>
              clear.mutate(undefined, {
                onSuccess: () => {
                  setReadAll(false);
                  toast.success(t('follow.cleared'));
                },
              })
            }
          >
            <CheckCheck className="size-3.5" aria-hidden />
            {t('follow.clearAll')}
          </Button>
        )}
      </div>

      {notices.length === 0 ? (
        <Empty className="py-16">
          <EmptyHeader>
            <EmptyMedia>
              <BellRing className="size-8 opacity-40" aria-hidden />
            </EmptyMedia>
            <EmptyTitle className="text-muted-foreground text-sm font-normal">
              {t('follow.noticesEmpty')}
            </EmptyTitle>
            <EmptyDescription className="text-xs opacity-70">
              {t('follow.noticesEmptyHint')}
            </EmptyDescription>
          </EmptyHeader>
        </Empty>
      ) : (
        <div className="flex flex-col gap-2">
          {notices.map((n) => {
            const unread = !n.read && !readAll;
            return (
              <button
                key={n.id}
                type="button"
                onClick={() => open(n)}
                className={cn(
                  'bg-card hover:border-foreground/30 flex items-center gap-3 rounded-xl border p-3 text-left transition-colors hover:shadow-md',
                  unread && 'border-primary/40',
                )}
              >
                <span
                  className={cn(
                    'shrink-0 rounded px-1.5 py-0.5 text-[10px] font-medium',
                    n.kind === 'online'
                      ? 'bg-primary/10 text-primary'
                      : 'bg-muted text-muted-foreground',
                  )}
                >
                  {n.kind === 'online' ? t('follow.kindOnline') : t('follow.kindFollow')}
                </span>
                <span className="min-w-0 flex-1">
                  <span className="block truncate text-sm font-medium">{n.title}</span>
                  <span className="text-muted-foreground block truncate text-xs">{n.detail}</span>
                </span>
                <span className="text-muted-foreground shrink-0 text-xs">
                  {formatNoticeTime(n.at)}
                </span>
                {unread && <span className="bg-destructive size-2 shrink-0 rounded-full" />}
              </button>
            );
          })}
        </div>
      )}
    </div>
  );
}

/** 未登录态（追更是服务端跟随列表，匿名拿不到数据）。 */
function NotLoggedIn() {
  const [open, setOpen] = useState(false);
  const qc = useQueryClient();
  return (
    <div className="text-muted-foreground flex flex-col items-center gap-3 py-20">
      <LogIn className="size-8 opacity-40" aria-hidden />
      <p className="text-sm">{t('follow.loginRequired')}</p>
      <Button size="sm" onClick={() => setOpen(true)}>
        <LogIn className="size-4" aria-hidden />
        {t('settings.loginAction')}
      </Button>
      <LoginDialog
        open={open}
        onOpenChange={setOpen}
        onSuccess={() => {
          // 登录成功后账号态翻转，本页的查询（enabled: !!account）自动开跑
          void qc.invalidateQueries({ queryKey: keys.followRoot });
          void qc.invalidateQueries({ queryKey: keys.followNotices });
        }}
      />
    </div>
  );
}

/** unix 秒 → 「10月15日 12:24」（随界面语言）。 */
function formatNoticeTime(secs: number): string {
  const dt = new Date(secs * 1000);
  if (Number.isNaN(dt.getTime())) return '';
  return new Intl.DateTimeFormat(locale() === 'zh-CN' ? 'zh-CN' : 'en-US', {
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  }).format(dt);
}
