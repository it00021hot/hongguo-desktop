//! 播放页互动栏：点赞 / 评论 / 收藏 / 预约 / 分享（抖音系右缘竖排形态）。
//!
//! 布局对齐 hgplayer/抖音：**icon 在上、计数在下**，纯白 + 投影贴着画面
//! 右缘，不做圆底按钮。计数从最近互动列表 best-effort 匹配（不在列表里
//! 就只显示 icon）。发弹幕入口在控制栏（hgplayer 同款），不在这里。
//!
//! 2026-10-05 抓包端点：点赞 do_action(3/4)、收藏 bookshelf(0/1)、
//! 预约 uncover_subscribe(1/2)、状态 mget；口径见 docs 第 9 节。

import { useCallback } from 'react';
import { useNavigate } from '@tanstack/react-router';
import { BellRing, Heart, MessageSquareText, Share2, Star } from 'lucide-react';
import { toast } from 'sonner';
import { t } from '@/i18n';
import { cn } from '@/lib/utils';
import {
  useAccount,
  useInteractionState,
  useReserveSeries,
  useReservations,
  useSeriesCollect,
  useVideoDigg,
} from '@/lib/queries';
import { usePlayerStore } from '@/lib/stores/player';

interface InteractionRailProps {
  seriesId: string;
  /** 「vid:seriesId」组合形态（与弹幕缓存 key 同构；空 = 档案未就绪，互动键禁用） */
  vid: string;
  /** 播放器悬浮层可见性（鼠标静止 3 秒后整体淡出，动一下即回） */
  visible?: boolean;
  /** 剧标题（分享文案用） */
  title?: string;
}

/** 计数格式化：抖音系「1.4万」样式。 */
function fmtCount(n: number): string {
  if (n >= 100_0000) return `${(n / 100_0000).toFixed(1).replace(/\.0$/, '')}百万`;
  if (n >= 10_000) return `${(n / 10_000).toFixed(n % 10_000 >= 1000 ? 1 : 0).replace(/\.0$/, '')}万`;
  return `${n}`;
}

/** 画面右缘竖排互动栏（悬浮在 stage 内，跟随悬浮层淡出）。 */
export function InteractionRail({ seriesId, vid, visible = true, title }: InteractionRailProps) {
  const navigate = useNavigate();
  const { data: account } = useAccount();
  const loggedIn = !!account;
  /** 点赞对象是裸 vid（组合形态前半段） */
  const bareVid = vid.split(':')[0] ?? '';

  const { data: state } = useInteractionState();
  // best-effort 匹配：当前集/剧在最近互动列表里才有「已互动」与计数
  const hit = state?.items.find((i) => i.vid === bareVid);
  const seriesHit = state?.items.find((i) => i.seriesId === seriesId);
  const digged = hit?.userDigg ?? false;
  const diggCount = hit?.diggedCount ?? 0;
  const collected = seriesHit?.followed ?? false;
  const collectCount = seriesHit?.followedCnt ?? 0;
  // 预约状态在「我的预约（待上线）」列表里匹配
  const { data: reservations } = useReservations(false);
  const reserved = reservations?.items.some((i) => i.seriesId === seriesId) ?? false;

  const digg = useVideoDigg();
  const collect = useSeriesCollect();
  const reserve = useReserveSeries();
  const setCommentPanelOpen = usePlayerStore((s) => s.setCommentPanelOpen);

  const requireLogin = useCallback(() => {
    toast.info(t('player.interact.loginRequired'));
    // 登录入口在设置页的账户卡片（LoginDialog），没有独立路由
    void navigate({ to: '/settings' });
  }, [navigate]);

  const onDigg = () => {
    if (!loggedIn) return requireLogin();
    if (!bareVid) return;
    digg.mutate(
      { vid: bareVid, seriesId, digg: !digged },
      {
        onSuccess: () => toast.success(t(digged ? 'player.interact.undone' : 'player.interact.liked')),
        onError: (e) => toast.error(String(e)),
      },
    );
  };

  const onCollect = () => {
    if (!loggedIn) return requireLogin();
    collect.mutate(
      { seriesId, collect: !collected },
      {
        onSuccess: () =>
          toast.success(t(collected ? 'player.interact.uncollected' : 'player.interact.collected')),
        onError: (e) => toast.error(String(e)),
      },
    );
  };

  const onReserve = () => {
    if (!loggedIn) return requireLogin();
    reserve.mutate(
      { seriesId, reserve: !reserved },
      {
        onSuccess: () =>
          toast.success(t(reserved ? 'player.interact.unreserved' : 'player.interact.reserved')),
        onError: (e) => toast.error(String(e)),
      },
    );
  };

  const onShare = async () => {
    const url = `https://hongguoduanju.com/detail?series_id=${seriesId}`;
    try {
      await navigator.clipboard.writeText(`${title ?? ''} ${url}`.trim());
      toast.success(t('player.interact.shareCopied'));
    } catch {
      toast.error(t('player.interact.shareFailed'));
    }
  };

  return (
    <div
      className={cn(
        'absolute right-2 top-1/2 z-20 flex -translate-y-1/2 flex-col items-center gap-4',
        'transition-opacity duration-300',
        visible ? 'opacity-100' : 'pointer-events-none opacity-0',
      )}
    >
      <RailItem
        icon={
          <Heart
            className={cn('size-7 drop-shadow-md', digged && 'fill-red-500 text-red-500')}
          />
        }
        label={t('player.interact.like')}
        count={diggCount > 0 ? diggCount : undefined}
        onClick={onDigg}
      />
      <RailItem
        icon={<MessageSquareText className="size-7 drop-shadow-md" />}
        label={t('player.interact.comments')}
        onClick={() => setCommentPanelOpen(true)}
      />
      <RailItem
        icon={
          <Star
            className={cn('size-7 drop-shadow-md', collected && 'fill-amber-400 text-amber-400')}
          />
        }
        label={t('player.interact.collect')}
        count={collectCount > 0 ? collectCount : undefined}
        onClick={onCollect}
      />
      <RailItem
        icon={
          <BellRing
            className={cn('size-7 drop-shadow-md', reserved && 'fill-sky-400 text-sky-400')}
          />
        }
        label={t('player.interact.reserve')}
        onClick={onReserve}
      />
      <RailItem
        icon={<Share2 className="size-6 drop-shadow-md" />}
        label={t('player.interact.share')}
        onClick={() => void onShare()}
      />
    </div>
  );
}

/** icon 在上、计数在下，纯白投影——抖音系右栏单元。 */
function RailItem({
  icon,
  label,
  count,
  onClick,
}: {
  icon: React.ReactNode;
  label: string;
  count?: number;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      title={label}
      className="flex cursor-pointer flex-col items-center gap-0.5 text-white drop-shadow-md transition-transform active:scale-90"
    >
      {icon}
      {count != null && (
        <span className="text-xs font-medium tabular-nums drop-shadow-md">{fmtCount(count)}</span>
      )}
    </button>
  );
}
