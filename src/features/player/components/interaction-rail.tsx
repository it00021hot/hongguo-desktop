//! 播放页互动栏：收藏 / 评论 / 点赞 / 分享（抖音系右缘竖排形态）。
//!
//! 布局对齐抖音：**icon 在上、计数在下**，实心纯白 + 投影贴着画面右缘，
//! 不做圆底按钮。图标为自绘抖音系实心 SVG——lucide 线框形态不像；
//! fill/stroke 走 currentColor，激活态（红心/黄星）由外层 fill-*/text-* 类接管。
//! 计数来自剧集档案的 detail 公开计数（匿名可见），登录后 mget 兜底
//! 「已赞/已追」红标。预约在侧边栏「预约」页，不在这。
//! 发弹幕入口在控制栏（hgplayer 同款），不在这里。
//!
//! 2026-10-05 抓包端点：点赞 do_action(3/4)、收藏 bookshelf(0/1)、
//! 状态 mget；口径见 docs 第 9 节。

import { useCallback } from 'react';
import { useNavigate } from '@tanstack/react-router';
import { toast } from 'sonner';
import { t } from '@/locales';
import { cn } from '@/lib/utils';
import { useAccount, useInteractionState, useSeriesCollect, useVideoDigg } from '@/service/queries';
import { usePlayerStore } from '@/stores/player';

// ── 抖音系实心图标（自绘；激活态类优先于 fill/stroke 属性，直接生效） ──

function DouyinStarIcon({ className }: { className?: string }) {
  return (
    <svg viewBox="0 0 24 24" className={className} aria-hidden="true">
      {/* 描边同色 + 圆角连接把星角撑圆，得到抖音那颗胖星 */}
      <path
        d="M12 17.27L18.18 21l-1.64-7.03L22 9.24l-7.19-.61L12 2 9.19 8.63 2 9.24l5.46 4.73L5.82 21z"
        fill="currentColor"
        stroke="currentColor"
        strokeWidth={2.2}
        strokeLinejoin="round"
      />
    </svg>
  );
}

function DouyinCommentIcon({ className }: { className?: string }) {
  return (
    <svg viewBox="0 0 24 24" className={className} aria-hidden="true">
      {/* 圆气泡撑满画幅（与心/星同量级）+ 左下甩尾，三个圆点用 evenodd 挖空（露出画面） */}
      <path
        fillRule="evenodd"
        clipRule="evenodd"
        d="M12 2.4C17.7 2.4 22.2 6.1 22.2 10.8C22.2 15.5 17.7 19.2 12 19.2C11.3 19.2 10.62 19.15 9.96 19.04C8.7 20 6.7 20.95 4.3 21.17C3.83 21.21 3.6 20.62 3.95 20.29C4.85 19.44 5.4 18.32 5.55 17.26C3.2 15.84 1.8 13.6 1.8 10.8C1.8 6.1 6.3 2.4 12 2.4ZM7.2 9.3A1.5 1.5 0 1 0 7.2 12.3A1.5 1.5 0 0 0 7.2 9.3ZM12 9.3A1.5 1.5 0 1 0 12 12.3A1.5 1.5 0 0 0 12 9.3ZM16.8 9.3A1.5 1.5 0 1 0 16.8 12.3A1.5 1.5 0 0 0 16.8 9.3Z"
        fill="currentColor"
      />
    </svg>
  );
}

function DouyinHeartIcon({ className }: { className?: string }) {
  return (
    <svg viewBox="0 0 24 24" className={className} aria-hidden="true">
      <path
        d="M12 21.35l-1.45-1.32C5.4 15.36 2 12.28 2 8.5 2 5.42 4.42 3 7.5 3c1.74 0 3.41.81 4.5 2.09C13.09 3.81 14.76 3 16.5 3 19.58 3 22 5.42 22 8.5c0 3.78-3.4 6.86-8.55 11.54L12 21.35z"
        fill="currentColor"
      />
    </svg>
  );
}

function DouyinShareIcon({ className }: { className?: string }) {
  return (
    <svg viewBox="0 0 24 24" className={className} aria-hidden="true">
      {/* 实心前转箭头（转发形态），描边圆角软化箭头尖角 */}
      <path
        d="M14 9V5l7 7-7 7v-4.1c-5 0-8.5 1.6-11 5.1 1-5 4-10 11-11z"
        fill="currentColor"
        stroke="currentColor"
        strokeWidth={1.5}
        strokeLinejoin="round"
      />
    </svg>
  );
}

interface InteractionRailProps {
  seriesId: string;
  /** 「vid:seriesId」组合形态（与弹幕缓存 key 同构；空 = 档案未就绪，互动键禁用） */
  vid: string;
  /** 播放器悬浮层可见性（鼠标静止 3 秒后整体淡出，动一下即回） */
  visible?: boolean;
  /** 剧标题（分享文案用） */
  title?: string;
  /** 该集评论数（剧集档案 detail 公开计数，匿名可见） */
  commentCount?: number;
  /** 该集点赞数（同上） */
  diggCount?: number;
  /** 全剧收藏数（同上） */
  followCount?: number;
}

/** 计数格式化：抖音系「1.4万」样式。 */
function fmtCount(n: number): string {
  if (n >= 100_0000) return `${(n / 100_0000).toFixed(1).replace(/\.0$/, '')}百万`;
  if (n >= 10_000)
    return `${(n / 10_000).toFixed(n % 10_000 >= 1000 ? 1 : 0).replace(/\.0$/, '')}万`;
  return `${n}`;
}

/** 画面右缘竖排互动栏（悬浮在 stage 内，跟随悬浮层淡出）。 */
export function InteractionRail({
  seriesId,
  vid,
  visible = true,
  title,
  commentCount,
  diggCount: publicDiggCount,
  followCount: publicFollowCount,
}: InteractionRailProps) {
  const navigate = useNavigate();
  const { data: account } = useAccount();
  const loggedIn = !!account;
  /** 点赞对象是裸 vid（组合形态前半段） */
  const bareVid = vid.split(':')[0] ?? '';

  const { data: state } = useInteractionState();
  // best-effort 匹配：当前集/剧在最近互动列表里才有「已互动」红标
  const hit = state?.items.find((i) => i.vid === bareVid);
  const seriesHit = state?.items.find((i) => i.seriesId === seriesId);
  const digged = hit?.userDigg ?? false;
  const collected = seriesHit?.followed ?? false;
  // 计数优先档案公开计数（detail 下发，匿名可见）；mget 命中值兜底
  const diggCount = (publicDiggCount ?? 0) > 0 ? publicDiggCount! : (hit?.diggedCount ?? 0);
  const collectCount =
    (publicFollowCount ?? 0) > 0 ? publicFollowCount! : (seriesHit?.followedCnt ?? 0);

  const digg = useVideoDigg();
  const collect = useSeriesCollect();
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
        onSuccess: () =>
          toast.success(t(digged ? 'player.interact.undone' : 'player.interact.liked')),
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

  const onShare = async () => {
    const url = `https://hongguoduanju.com/detail?series_id=${seriesId}`;
    try {
      await navigator.clipboard.writeText(`${title ?? ''} ${url}`.trim());
      toast.success(t('player.interact.shareCopied'));
    } catch {
      toast.error(t('player.interact.shareFailed'));
    }
  };

  // 顺序对齐抖音：收藏 → 评论 → 点赞 → 分享
  return (
    <div
      data-wheel-block
      className={cn(
        'absolute right-2 bottom-24 z-20 flex flex-col items-center gap-4',
        'transition-opacity duration-300',
        visible ? 'opacity-100' : 'pointer-events-none opacity-0',
      )}
    >
      <RailItem
        icon={
          <DouyinStarIcon className={cn('size-7', collected && 'fill-amber-400 text-amber-400')} />
        }
        label={t('player.interact.collect')}
        text={collectCount > 0 ? fmtCount(collectCount) : undefined}
        onClick={onCollect}
      />
      {/* 气泡形状宽扁、实心面积小，同尺寸下视觉偏小 → icon 光学校偿 +1 档（size-8） */}
      <RailItem
        icon={<DouyinCommentIcon className="size-8" />}
        label={t('player.interact.comments')}
        text={commentCount && commentCount > 0 ? fmtCount(commentCount) : undefined}
        onClick={() => setCommentPanelOpen(true)}
      />
      <RailItem
        icon={<DouyinHeartIcon className={cn('size-7', digged && 'fill-red-500 text-red-500')} />}
        label={t('player.interact.like')}
        text={diggCount > 0 ? fmtCount(diggCount) : undefined}
        onClick={onDigg}
      />
      <RailItem
        icon={<DouyinShareIcon className="size-7" />}
        label={t('player.interact.share')}
        text={t('player.interact.share')}
        onClick={() => void onShare()}
      />
    </div>
  );
}

/** icon 在上、计数/文案在下，纯白投影——抖音系右栏单元。 */
function RailItem({
  icon,
  label,
  text,
  onClick,
}: {
  icon: React.ReactNode;
  label: string;
  /** icon 下方内容：计数（已格式化）或固定文案（分享的「分享」）；空 = 不显示 */
  text?: string;
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
      {text != null && <span className="text-xs font-medium tabular-nums">{text}</span>}
    </button>
  );
}
