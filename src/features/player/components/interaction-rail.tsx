//! 播放页互动栏：发弹幕 / 点赞 / 收藏 / 预约（2026-10-05 抓包端点的前端落点）。
//!
//! 形态对齐 hgplayer 沉浸流：画面右缘竖排悬浮键，弹幕输入浮层从键位展开。
//! 登录态回显是 best-effort（最近互动列表 100 条内匹配），未登录点击直接
//! 指路登录页，不放行空请求。

import { useCallback, useRef, useState } from 'react';
import { useNavigate } from '@tanstack/react-router';
import { BellRing, Heart, MessageSquareText, Send, Star } from 'lucide-react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { t } from '@/i18n';
import { cn } from '@/lib/utils';
import {
  useAccount,
  useInteractionState,
  useReserveSeries,
  useReservations,
  useSeriesCollect,
  useSendDanmaku,
  useVideoDigg,
} from '@/lib/queries';

interface InteractionRailProps {
  seriesId: string;
  /** 「vid:seriesId」组合形态（与弹幕缓存 key 同构；空 = 档案未就绪，互动键禁用） */
  vid: string;
  /** 读当前播放位置（发弹幕的时间轴），由 PlayerView 的 videoRef 提供 */
  getCurrentMs: () => number;
  /** 播放器悬浮层可见性（鼠标静止 3 秒后整体淡出，动一下即回） */
  visible?: boolean;
}

/** 画面右缘竖排互动栏（悬浮在 stage 内，不随控制栏隐没）。 */
export function InteractionRail({ seriesId, vid, getCurrentMs, visible = true }: InteractionRailProps) {
  const navigate = useNavigate();
  const { data: account } = useAccount();
  const loggedIn = !!account;
  /** 点赞对象是裸 vid（组合形态前半段） */
  const bareVid = vid.split(':')[0] ?? '';

  const { data: state } = useInteractionState();
  const digged = !!bareVid && (state?.diggedVids.includes(bareVid) ?? false);
  const collected = state?.collectedSeries.includes(seriesId) ?? false;
  // 预约状态在「我的预约（待上线）」列表里匹配（已上线剧无预约概念，点了也会成功但无意义——照放，服务端兜底）
  const { data: reservations } = useReservations(false);
  const reserved = reservations?.items.some((i) => i.seriesId === seriesId) ?? false;

  const digg = useVideoDigg();
  const collect = useSeriesCollect();
  const reserve = useReserveSeries();
  /** 弹幕输入开着时强制可见（打字时鼠标多半不动，别把输入框藏没了） */
  const [composerOpen, setComposerOpen] = useState(false);
  const railVisible = visible || composerOpen;

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

  return (
    <div
      className={cn(
        'absolute right-3 top-1/2 z-20 flex -translate-y-1/2 flex-col items-center gap-1.5',
        'transition-opacity duration-300',
        railVisible ? 'opacity-100' : 'pointer-events-none opacity-0',
      )}
    >
      <DanmakuComposer
        vid={vid}
        getCurrentMs={getCurrentMs}
        disabled={!loggedIn}
        onNeedLogin={requireLogin}
        open={composerOpen}
        onOpenChange={setComposerOpen}
      />
      <RailButton
        icon={<Heart className={cn('size-5', digged && 'fill-red-500 text-red-500')} />}
        label={t('player.interact.like')}
        active={digged}
        onClick={onDigg}
      />
      <RailButton
        icon={<Star className={cn('size-5', collected && 'fill-amber-400 text-amber-400')} />}
        label={t('player.interact.collect')}
        active={collected}
        onClick={onCollect}
      />
      <RailButton
        icon={<BellRing className={cn('size-5', reserved && 'text-sky-400')} />}
        label={t('player.interact.reserve')}
        active={reserved}
        onClick={onReserve}
      />
    </div>
  );
}

function RailButton({
  icon,
  label,
  active,
  onClick,
}: {
  icon: React.ReactNode;
  label: string;
  active: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      title={label}
      className={cn(
        'grid size-10 place-items-center rounded-full bg-black/45 text-white backdrop-blur-sm',
        'transition-colors hover:bg-black/65',
        active && 'bg-black/60',
      )}
    >
      {icon}
    </button>
  );
}

/** 弹幕发送：键位展开输入浮层，Enter / 发送按钮提交，成功后乐观进弹幕列表。 */
function DanmakuComposer({
  vid,
  getCurrentMs,
  disabled,
  onNeedLogin,
  open,
  onOpenChange,
}: {
  vid: string;
  getCurrentMs: () => number;
  disabled: boolean;
  onNeedLogin: () => void;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const [text, setText] = useState('');
  const inputRef = useRef<HTMLInputElement>(null);
  const send = useSendDanmaku();

  const submit = () => {
    const content = text.trim();
    if (!content || !vid.includes(':')) return;
    send.mutate(
      { vid, text: content, offsetMs: Math.round(getCurrentMs()) },
      {
        onSuccess: () => {
          toast.success(t('player.interact.danmakuSent'));
          setText('');
          onOpenChange(false);
        },
        onError: (e) => toast.error(String(e)),
      },
    );
  };

  return (
    <div className="flex flex-col items-center gap-2">
      <div
        className={cn(
          'flex items-center gap-1.5 rounded-full bg-black/70 p-1.5 pl-3 backdrop-blur-sm',
          'transition-all duration-200',
          open ? 'mr-0 opacity-100' : 'pointer-events-none -mr-2 opacity-0',
        )}
      >
        <Input
          ref={inputRef}
          value={text}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter') submit();
            if (e.key === 'Escape') onOpenChange(false);
          }}
          placeholder={t('player.interact.danmakuPlaceholder')}
          className="h-7 w-44 border-none bg-transparent text-sm text-white placeholder:text-neutral-400 focus-visible:ring-0"
          maxLength={100}
        />
        <Button
          size="icon"
          variant="ghost"
          className="size-7 rounded-full text-white hover:bg-white/15"
          disabled={send.isPending || !text.trim()}
          onClick={submit}
        >
          <Send className="size-4" />
        </Button>
      </div>
      <RailButton
        icon={<MessageSquareText className="size-5" />}
        label={t('player.interact.danmaku')}
        active={open}
        onClick={() => {
          if (disabled) return onNeedLogin();
          onOpenChange(!open);
          if (!open) setTimeout(() => inputRef.current?.focus(), 50);
        }}
      />
    </div>
  );
}
