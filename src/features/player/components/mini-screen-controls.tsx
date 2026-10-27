import { useEffect, useState } from 'react';
import {
  Eye,
  Maximize,
  Maximize2,
  Minimize,
  Pause,
  Pin,
  Play,
  RectangleHorizontal,
  SkipBack,
  SkipForward,
  Volume2,
  VolumeX,
} from 'lucide-react';
import { formatDuration } from '@/utils/format';
import { readPlaybackRate, writeMuted, writePlaybackRate } from '@/utils/playback-prefs';
import { t, tf } from '@/locales';
import { cn } from '@/lib/utils';
import { ScrubBar } from './controls/scrub-bar';
import { ChromeSurface } from './chrome-surface';

/** 小屏里点一下倍速标签循环的档位（常用档，完整菜单回大屏）。 */
const MINI_RATES = [1, 1.5, 2, 3];

/**
 * 紧凑控制条，两种形态共用一行按钮集：
 * - `mini`（默认）：480×270 小窗。顶栏已被摘掉，所以剧名/简介、置顶
 *   都长在条上；⤢ = 退出小屏回大窗。
 * - `portrait`：竖屏窄条。顶栏还在（置顶钮在顶栏，条上不重复），
 *   剧名/简介由 ImmersiveInfoOverlay 承担（条上不重复）；⤢ = 退出竖屏
 *   回横屏，那个位置给**铺满**切换（竖屏专属档位，Esc 退铺满回竖条）。
 *
 * 曾有的 ✕「结束播放」已删（用户口径）：它与 ⤢ 都是回大窗，只差退出时
 * 暂停一拍，视觉重复；想暂停直接按 ▶。
 *
 * 挂在播放器舞台底部，与全量控制栏共用同一个 `<video>`——切换形态时
 * 视频元素不卸载，播放零中断。进度/暂停/静音/倍速都直接从 video 元素
 * 的事件里读（元素归播放页所有，这里不复制状态源），写操作直接改元素
 * 并落偏好。
 */
export function MiniScreenControls({
  variant = 'mini',
  videoRef,
  title,
  intro,
  vidIndex,
  total,
  hasNext,
  onStepEpisode,
  onExpand,
  incognitoOn,
  onToggleIncognito,
  pinned,
  onTogglePinned,
  portraitFull,
  onTogglePortraitFull,
  visible,
  onControlsEnter,
  onControlsLeave,
}: {
  /** 控件形态：mini = 小窗（默认）；portrait = 竖屏窄条 */
  variant?: 'mini' | 'portrait';
  videoRef: React.RefObject<HTMLVideoElement | null>;
  title?: string;
  intro?: string;
  vidIndex?: number | null;
  total: number;
  hasNext: boolean;
  onStepEpisode: (delta: 1 | -1) => void;
  /** 退出当前紧凑形态回大窗（mini = 退出小屏；portrait = 退出竖屏），图标文案随形态 */
  onExpand: () => void;
  incognitoOn: boolean;
  onToggleIncognito: () => void;
  pinned?: boolean;
  onTogglePinned?: () => void;
  /** portrait 形态专用：铺满档切换 */
  portraitFull?: boolean;
  onTogglePortraitFull?: () => void;
  visible: boolean;
  /** 指针悬在控制条本体上：上报外层停掉隐藏倒计时（B站同款，悬在控件上不许收） */
  onControlsEnter: () => void;
  onControlsLeave: () => void;
}) {
  const portrait = variant === 'portrait';
  const [paused, setPaused] = useState(true);
  const [current, setCurrent] = useState(0);
  const [duration, setDuration] = useState(0);
  const [muted, setMuted] = useState(false);
  const [rate, setRate] = useState(() => readPlaybackRate());

  // 从 video 元素同步只读状态：元素在大屏时就在播，进入小屏那一刻的
  // 进度/暂停态必须立刻是对的，不能从 0 开始「闪一下再跳」
  useEffect(() => {
    const video = videoRef.current;
    if (!video) return;
    setPaused(video.paused);
    setCurrent(video.currentTime);
    setDuration(Number.isFinite(video.duration) ? video.duration : 0);
    setMuted(video.muted);
    setRate(video.playbackRate);
    const onPlay = () => setPaused(false);
    const onPause = () => setPaused(true);
    const onTime = (e: Event) => {
      const v = e.currentTarget instanceof HTMLVideoElement ? e.currentTarget : null;
      if (!v) return;
      setCurrent(v.currentTime);
      setDuration(Number.isFinite(v.duration) ? v.duration : 0);
    };
    const onVolume = (e: Event) => {
      const v = e.currentTarget instanceof HTMLVideoElement ? e.currentTarget : null;
      if (v) setMuted(v.muted);
    };
    const onRate = (e: Event) => {
      const v = e.currentTarget instanceof HTMLVideoElement ? e.currentTarget : null;
      if (v) setRate(v.playbackRate);
    };
    video.addEventListener('play', onPlay);
    video.addEventListener('pause', onPause);
    video.addEventListener('timeupdate', onTime);
    video.addEventListener('durationchange', onTime);
    video.addEventListener('volumechange', onVolume);
    video.addEventListener('ratechange', onRate);
    return () => {
      video.removeEventListener('play', onPlay);
      video.removeEventListener('pause', onPause);
      video.removeEventListener('timeupdate', onTime);
      video.removeEventListener('durationchange', onTime);
      video.removeEventListener('volumechange', onVolume);
      video.removeEventListener('ratechange', onRate);
    };
  }, [videoRef]);

  const togglePlay = () => {
    const video = videoRef.current;
    if (!video) return;
    if (video.paused) void video.play().catch(() => undefined);
    else video.pause();
  };

  const cycleRate = () => {
    const video = videoRef.current;
    if (!video) return;
    const idx = MINI_RATES.findIndex((r) => Math.abs(r - video.playbackRate) < 0.01);
    const next = MINI_RATES[(idx + 1) % MINI_RATES.length] ?? 1;
    video.playbackRate = next;
    writePlaybackRate(next);
  };

  return (
    <ChromeSurface
      shown={visible}
      onClick={(e) => e.stopPropagation()}
      onMouseEnter={onControlsEnter}
      onMouseLeave={onControlsLeave}
      className="absolute inset-x-0 bottom-0 z-20 flex flex-col gap-1 bg-gradient-to-t from-black/75 to-transparent px-2 pt-6 pb-1.5 text-white"
    >
      {/* 剧名/集数/简介贴着进度条上方（官方客户端小屏同款三行形态），宽度
          收敛 + 截断（小屏 480 宽，长剧名长简介都不挡画面）。portrait
          形态不渲染——竖屏的剧名/简介由 ImmersiveInfoOverlay 承担 */}
      {!portrait && (
        <div className="pointer-events-none max-w-[62%]">
          <p className="truncate text-xs font-semibold drop-shadow-[0_1px_2px_rgba(0,0,0,0.9)]">
            @{title || t('player.miniTitle')}
          </p>
          <p className="mt-0.5 truncate text-[10px] text-white/80">
            {vidIndex ? tf('player.episodeNo', { index: vidIndex }) : ''}
            {total > 0 ? ` · ${tf('player.totalEpisodes', { count: total })}` : ''}
            {intro ? ` ${intro}` : ''}
          </p>
        </div>
      )}
      {/* 进度条与大屏共用 ScrubBar：此前用原生 <input type=range> +
          accent-red-500，在 WebView2 上是粗红条大红钮，和细化后的大屏
          进度条完全两个观感 */}
      <ScrubBar
        current={Math.min(current, duration || 0)}
        duration={duration}
        onSeek={(ratio) => {
          const video = videoRef.current;
          if (!video || duration <= 0) return;
          const next = ratio * duration;
          video.currentTime = next;
          setCurrent(next);
        }}
        onScrubStart={() => undefined}
        onScrubEnd={() => undefined}
      />
      {/* 时长 nowrap——极窄下宁可溢出 1px 也不折行把控制条顶高 */}
      <div className="flex items-center gap-0.5">
        <MiniButton label={paused ? t('player.play') : t('player.pause')} onClick={togglePlay}>
          {paused ? <Play className="size-4" /> : <Pause className="size-4" />}
        </MiniButton>
        <MiniButton label={t('player.prevEpisode')} onClick={() => onStepEpisode(-1)}>
          <SkipBack className="size-4" />
        </MiniButton>
        <MiniButton
          label={t('player.nextEpisode')}
          onClick={() => onStepEpisode(1)}
          disabled={!hasNext}
        >
          <SkipForward className="size-4" />
        </MiniButton>
        <span className="font-mono text-[10px] whitespace-nowrap text-white/85 tabular-nums">
          {formatDuration(current)} / {formatDuration(duration)}
        </span>
        <div className="ml-auto flex items-center gap-0.5">
          {/* 倍速在右组首位（用户口径）：贴着时长会连成一排数字，容易误读 */}
          <MiniButton label={t('player.playbackRate')} onClick={cycleRate}>
            <span className="font-mono text-[10px]">{rate}x</span>
          </MiniButton>
          <MiniButton
            label={t('player.mute')}
            onClick={() => {
              const video = videoRef.current;
              if (!video) return;
              video.muted = !video.muted;
              setMuted(video.muted);
              writeMuted(video.muted);
            }}
          >
            {muted ? <VolumeX className="size-4" /> : <Volume2 className="size-4" />}
          </MiniButton>
          <MiniButton label={t('player.incognito')} onClick={onToggleIncognito}>
            <Eye className={cn('size-4', incognitoOn ? 'text-white' : 'text-white/55')} />
          </MiniButton>
          {/* 置顶只有 mini 形态要：portrait 的顶栏常驻，同款置顶钮已在顶栏 */}
          {!portrait && (
            <MiniButton label={t('player.pin')} onClick={() => onTogglePinned?.()}>
              <Pin className={cn('size-4', pinned ? 'fill-current' : 'text-white/55')} />
            </MiniButton>
          )}
          {/* 铺满切换只有 portrait 形态要：小窗尺寸固定无档位可言。
              竖屏里铺满/收回是核心档位，Esc 退铺满回竖条 */}
          {portrait && (
            <MiniButton
              label={portraitFull ? t('player.exitPortraitFill') : t('player.portraitFill')}
              onClick={() => onTogglePortraitFull?.()}
            >
              {portraitFull ? <Minimize className="size-4" /> : <Maximize className="size-4" />}
            </MiniButton>
          )}
          <MiniButton
            label={portrait ? t('player.exitPortrait') : t('player.exitMiniScreen')}
            onClick={onExpand}
          >
            {portrait ? (
              <RectangleHorizontal className="size-4" />
            ) : (
              <Maximize2 className="size-4" />
            )}
          </MiniButton>
        </div>
      </div>
    </ChromeSurface>
  );
}

/** 紧凑按钮：半透明圆角块，hover 加深（悬浮在视频上，不抢画面）。 */
function MiniButton({
  label,
  onClick,
  disabled,
  children,
}: {
  label: string;
  onClick: () => void;
  disabled?: boolean;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      onClick={onClick}
      disabled={disabled}
      className="grid h-7 min-w-7 place-items-center rounded-md px-1 text-white/85 transition-colors hover:bg-white/20 hover:text-white disabled:opacity-40"
    >
      {children}
    </button>
  );
}
