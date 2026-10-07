import { useCallback, useEffect, useRef, useState } from 'react';
import {
  Eye,
  Loader2,
  Maximize2,
  Minus,
  Pause,
  Play,
  SkipBack,
  SkipForward,
  Volume2,
  VolumeX,
  X,
} from 'lucide-react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { usePlayerStore } from '@/lib/stores/player';
import { usePlay, useSavePosition, useSeriesEpisodes, useSettings } from '@/lib/queries';
import { useEvent } from '@/lib/ipc/events';
import { EVENTS } from '@/lib/ipc/types';
import { app as appApi } from '@/lib/ipc/commands';
import { formatDuration } from '@/lib/format';
import {
  readMuted,
  readPlaybackRate,
  readVolume,
  writeMuted,
  writePlaybackRate,
} from '@/lib/playback-prefs';
import { t, tf } from '@/i18n';
import { cn } from '@/lib/utils';
import type { OnlineProgress } from '@/lib/schema';
import { useIncognitoMode } from './incognito';

/** 小窗控制栏静止隐藏的等待（与主播放器一致的节奏）。 */
const CHROME_HIDE_MS = 3_000;
/** 进度落库节流：小窗可能被系统直接销毁（拿不到卸载时机），周期存档兜底。 */
const SAVE_INTERVAL = 5_000;
/** 小窗里点一下倍速标签循环的档位（常用档，完整菜单回主窗）。 */
const MINI_RATES = [1, 1.5, 2, 3];

/**
 * 小窗播放页（`/mini`，独立 Tauri 窗口），形态对齐 hgplayer 的桌面小窗：
 *
 * - **窗口比例跟视频走**：起播拿到宽高比后 `fit_mini_window` 把窗口校正成
 *   视频的长宽（横屏剧=横窗、竖屏剧=竖窗），视频铺满整个窗口零黑边——
 *   不是「把视频塞进一个手机形状的窗」；
 * - 右上角 Windows 风格窗口按钮组（回完整模式 / 最小化 / 关闭）；
 * - 左侧中部 `@剧名 · 第 N 集 / 共 M 集` 信息块；
 * - 底部红色进度条 + 播放/步进/时间/倍速/音量/隐身控件行，全部悬浮。
 *
 * 与主播放器共用后端取流/进度链路：小窗 `play_series` 命中的是同一个
 * 内存流缓存，主窗里已下好的字节直接复用。弹幕与兼容转码兜底不进小窗
 * ——小窗要的是「小而快」，完整体验回主窗。
 */
export function MiniPlayerPage({
  initialSeries,
  initialIndex,
}: {
  initialSeries: string;
  initialIndex: number;
}) {
  const videoRef = useRef<HTMLVideoElement | null>(null);
  const setTarget = usePlayerStore((s) => s.setTarget);
  const seriesId = usePlayerStore((s) => s.seriesId);
  const vidIndex = usePlayerStore((s) => s.vidIndex);

  const { mutate: play } = usePlay();
  const { mutate: savePosition } = useSavePosition();
  const { data: settings } = useSettings();
  const autoNext = settings?.autoNextEpisode ?? true;
  const { data: currentSeries } = useSeriesEpisodes(seriesId);

  const [src, setSrc] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [retryTick, setRetryTick] = useState(0);
  const [buffering, setBuffering] = useState<OnlineProgress | null>(null);
  const [paused, setPaused] = useState(true);
  const [current, setCurrent] = useState(0);
  const [duration, setDuration] = useState(0);
  const [muted, setMuted] = useState(() => readMuted());
  const [rate, setRate] = useState(() => readPlaybackRate());
  const [chromeVisible, setChromeVisible] = useState(true);

  const episodeKey = seriesId && vidIndex ? `${seriesId}:${vidIndex}` : '';
  /** 续播位置（metadata 就绪后 seek） */
  const pendingSeek = useRef(0);
 /** 最近一次有效的进度（卸载时 video 元素可能已先被拆走，从这里取） */
  const lastKnown = useRef({ key: '', time: 0, duration: 0 });
  const lastSavedAt = useRef(0);
  /** 窗口只按视频宽高比校正一次（后续换集比例相同；用户手动拉伸不被覆盖） */
  const fitted = useRef(false);

  // 挂载即认领目标：URL 带来的剧集/集号写进 store，步进换集都跟着它走
  useEffect(() => {
    if (initialSeries) setTarget(initialSeries, initialIndex);
  }, [initialSeries, initialIndex, setTarget]);

  // ---- 起播（与主播放器同一套响应语义：url / resumeAt / error） ----
  useEffect(() => {
    if (!seriesId || !vidIndex) return;
    play(
      { seriesId, vidIndex },
      {
        onSuccess: (res) => {
          setError(res.error || null);
          setSrc(res.error ? null : res.url);
          pendingSeek.current = res.error ? 0 : res.resumeAt;
        },
        onError: (e) => {
          setError(e.message);
          setSrc(null);
        },
      },
    );
    // retryTick 进依赖：点重试再取一次流
  }, [seriesId, vidIndex, retryTick, play]);

  // 在线取流进度只认当前这一集
  useEvent<OnlineProgress>(
    EVENTS.onlinePlayProgress,
    useCallback(
      (p: OnlineProgress) => setBuffering(p.key === episodeKey ? p : null),
      [episodeKey],
    ),
  );

  // ---- 进度落库：5s 节流 + 暂停/步进/关窗强制 ----
  const persist = useCallback(
    (time: number, dur: number, force = false) => {
      if (!seriesId || !vidIndex || time <= 0) return;
      if (episodeKey) lastKnown.current = { key: episodeKey, time, duration: dur };
      const now = Date.now();
      if (!force && now - lastSavedAt.current < SAVE_INTERVAL) return;
      lastSavedAt.current = now;
      savePosition({ seriesId, vidIndex, currentTime: time, duration: dur });
    },
    [seriesId, vidIndex, episodeKey, savePosition],
  );

  useEffect(() => {
    return () => {
      const { key, time, duration: dur } = lastKnown.current;
      if (!seriesId || !vidIndex || key !== episodeKey || time <= 0) return;
      savePosition({ seriesId, vidIndex, currentTime: time, duration: dur });
    };
  }, [seriesId, vidIndex, episodeKey, savePosition]);

  const stepEpisode = useCallback(
    (delta: number) => {
      if (!seriesId || !vidIndex) return;
      const next = vidIndex + delta;
      if (next < 1) return;
      const total = currentSeries?.episodes.length ?? 0;
      // 最后一集继续点下一集：亮出控制栏让状态可见，静默不动像坏了
      if (total > 0 && next > total) {
        setChromeVisible(true);
        return;
      }
      lastKnown.current = { key: '', time: 0, duration: 0 };
      setTarget(seriesId, next);
    },
    [seriesId, vidIndex, currentSeries, setTarget],
  );

  // ---- 元素事件 ----
  const handleLoadedMetadata = useCallback(() => {
    const video = videoRef.current;
    if (!video) return;
    // 倍速/音量/静音沿用户偏好（与主窗共享同一份 localStorage）
    video.playbackRate = readPlaybackRate();
    video.volume = readVolume();
    video.muted = readMuted();
    if (pendingSeek.current > 0 && pendingSeek.current < video.duration) {
      video.currentTime = pendingSeek.current;
    }
    pendingSeek.current = 0;
    // 窗口对齐视频宽高比：横屏剧变横窗、视频铺满零黑边（只校正一次）
    if (!fitted.current && video.videoWidth > 0 && video.videoHeight > 0) {
      fitted.current = true;
      void appApi
        .fitMiniWindow(video.videoWidth, video.videoHeight)
        .catch(() => undefined);
    }
    // autoPlay 在带声音时会被 WebView 拦下，metadata 后补一次显式 play
    void video.play().catch(() => undefined);
  }, []);

  const togglePlay = useCallback(() => {
    const video = videoRef.current;
    if (!video) return;
    if (video.paused) void video.play().catch(() => undefined);
    else video.pause();
  }, []);

  const cycleRate = useCallback(() => {
    const video = videoRef.current;
    if (!video) return;
    const idx = MINI_RATES.findIndex((r) => Math.abs(r - video.playbackRate) < 0.01);
    const next = MINI_RATES[(idx + 1) % MINI_RATES.length] ?? 1;
    video.playbackRate = next;
    setRate(next);
    writePlaybackRate(next);
  }, []);

  const handleVideoError = useCallback(() => {
    if (error) return;
    const me = videoRef.current?.error;
    const detail = me ? `（MediaError ${me.code}${me.message ? `: ${me.message}` : ''}）` : '';
    setError(`${t('player.onlineInterrupted')}${detail}`);
  }, [error]);

  /** 返回主窗口：先精确落一次进度再关小窗（主窗恢复由后端事件完成） */
  const backToMain = useCallback(() => {
    const video = videoRef.current;
    const time = video ? video.currentTime : lastKnown.current.time;
    const dur =
      video && Number.isFinite(video.duration) ? video.duration : lastKnown.current.duration;
    if (seriesId && vidIndex && time > 0) {
      savePosition({ seriesId, vidIndex, currentTime: time, duration: dur });
    }
    void appApi.closeMiniWindow().catch(() => undefined);
  }, [seriesId, vidIndex, savePosition]);

  // ---- 悬浮层：静止 3s 收起，动一下唤醒；暂停时常显 ----
  useEffect(() => {
    if (paused || !chromeVisible) return;
    const timer = setTimeout(() => setChromeVisible(false), CHROME_HIDE_MS);
    return () => clearTimeout(timer);
  }, [paused, chromeVisible, current]);

  const incognito = useIncognitoMode(videoRef);
  const appWindow = getCurrentWindow();

  const hasNext =
    currentSeries?.episodes.some((e) => e.vidIndex === (vidIndex ?? 0) + 1) ??
    (currentSeries ? false : true);
  const total = currentSeries?.episodes.length ?? 0;
  const chromeShown = chromeVisible || paused;

  return (
    <div
      className="relative h-screen w-screen overflow-hidden bg-black select-none"
      onMouseMove={() => setChromeVisible(true)}
      onClick={togglePlay}
    >
      {src && (
        <video
          key={src}
          ref={videoRef}
          src={src}
          /* 铺满窗口（窗口比例已对齐视频，无裁切；用户手动拉伸后裁边保满屏） */
          className="absolute inset-0 size-full object-cover"
          autoPlay
          onLoadedMetadata={handleLoadedMetadata}
          onPlay={() => setPaused(false)}
          onPause={() => {
            setPaused(true);
            const video = videoRef.current;
            if (video)
              persist(
                video.currentTime,
                Number.isFinite(video.duration) ? video.duration : 0,
                true,
              );
          }}
          onTimeUpdate={(e) => {
            setCurrent(e.currentTarget.currentTime);
            const dur = Number.isFinite(e.currentTarget.duration) ? e.currentTarget.duration : 0;
            setDuration(dur);
            persist(e.currentTarget.currentTime, dur);
          }}
          onEnded={() => {
            if (autoNext && hasNext) stepEpisode(1);
            else setChromeVisible(true);
          }}
          onError={handleVideoError}
        />
      )}

      {/* 取流/加载中：小窗里也要能看出「在动」 */}
      {!src && !error && (
        <div className="absolute inset-0 grid place-items-center">
          <Loader2 className="size-8 animate-spin text-white/80" aria-hidden />
        </div>
      )}
      {src && buffering && buffering.phase !== 'ready' && buffering.percent < 100 && (
        <div className="absolute top-2 left-1/2 -translate-x-1/2 rounded-full bg-black/60 px-3 py-1 text-xs text-white/85">
          {t('common.loading')} {Math.floor(buffering.percent)}%
        </div>
      )}
      {error && (
        <div className="absolute inset-0 z-10 flex flex-col items-center justify-center gap-3 px-6 text-center">
          <p className="text-sm text-white/85">{error}</p>
          <button
            type="button"
            onClick={(e) => {
              e.stopPropagation();
              setError(null);
              setRetryTick((n) => n + 1);
            }}
            className="rounded-full border border-white/30 px-4 py-1.5 text-xs text-white hover:bg-white/10"
          >
            {t('player.retry')}
          </button>
        </div>
      )}

      {/* 右上角窗口按钮组（桌面小窗的 Windows 习惯位）：回完整模式 / 最小化 / 关闭。
          顶条背景同时是小窗的拖拽区——按钮本身不带 drag 标记，点按钮不拖窗 */}
      <div
        data-tauri-drag-region
        onClick={(e) => e.stopPropagation()}
        className={cn(
          'absolute top-0 right-0 z-20 flex items-center gap-0.5 bg-gradient-to-l from-black/60 to-transparent px-2 py-2 pl-8',
          'transition-opacity duration-200',
          chromeShown ? 'opacity-100' : 'pointer-events-none opacity-0',
        )}
      >
        <MiniButton label={t('player.backToMain')} onClick={backToMain}>
          <Maximize2 className="size-3.5" />
        </MiniButton>
        <MiniButton
          label={t('window.minimize')}
          onClick={() => void appWindow.minimize().catch(() => undefined)}
        >
          <Minus className="size-4" />
        </MiniButton>
        <MiniButton label={t('player.backToMain')} onClick={backToMain}>
          <X className="size-4" />
        </MiniButton>
      </div>

      {/* 左侧中部信息块（hgplayer 同款位置）：@剧名 / 集数进度 */}
      <div
        className={cn(
          'pointer-events-none absolute top-[36%] left-5 z-10 max-w-[70%] text-white',
          'drop-shadow-[0_1px_3px_rgba(0,0,0,0.9)] transition-opacity duration-200',
          chromeShown ? 'opacity-100' : 'opacity-0',
        )}
      >
        <p className="text-sm font-semibold">
          @{currentSeries?.title || t('player.miniTitle')}
        </p>
        {vidIndex ? (
          <p className="mt-1 text-xs text-white/85">
            {tf('player.episodeNo', { index: vidIndex })}
            {total > 0 ? ` · ${tf('player.totalEpisodes', { count: total })}` : ''}
          </p>
        ) : null}
      </div>

      {/* 底部：红色进度条贯穿 + 控件行（播放/步进/时间 | 倍速/音量/隐身） */}
      <div
        onClick={(e) => e.stopPropagation()}
        className={cn(
          'absolute inset-x-0 bottom-0 z-20 flex flex-col gap-1 bg-gradient-to-t from-black/75 to-transparent px-3 pt-10 pb-2 text-white',
          'transition-opacity duration-200',
          chromeShown ? 'opacity-100' : 'pointer-events-none opacity-0',
        )}
      >
        <input
          type="range"
          min={0}
          max={duration || 0}
          step={0.1}
          value={Math.min(current, duration || 0)}
          disabled={!duration}
          onChange={(e) => {
            const video = videoRef.current;
            if (!video) return;
            const next = Number(e.target.value);
            video.currentTime = next;
            setCurrent(next);
          }}
          className="h-1.5 w-full cursor-pointer accent-red-500"
          aria-label={t('player.progress')}
        />
        <div className="flex items-center gap-1">
          <MiniButton label={paused ? t('player.play') : t('player.pause')} onClick={togglePlay}>
            {paused ? <Play className="size-4" /> : <Pause className="size-4" />}
          </MiniButton>
          <MiniButton label={t('player.prevEpisode')} onClick={() => stepEpisode(-1)}>
            <SkipBack className="size-4" />
          </MiniButton>
          <MiniButton
            label={t('player.nextEpisode')}
            onClick={() => stepEpisode(1)}
            disabled={!hasNext}
          >
            <SkipForward className="size-4" />
          </MiniButton>
          <span className="ml-1 font-mono text-[10px] text-white/85 tabular-nums">
            {formatDuration(current)} / {formatDuration(duration)}
          </span>
          <div className="ml-auto flex items-center gap-1">
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
            <MiniButton label={t('player.incognito')} onClick={incognito.toggle}>
              <Eye className={cn('size-4', incognito.on ? 'text-white' : 'text-white/55')} />
            </MiniButton>
          </div>
        </div>
      </div>
    </div>
  );
}

/** 小窗按钮：半透明圆角块，hover 加深（悬浮在视频上，不抢画面）。 */
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
