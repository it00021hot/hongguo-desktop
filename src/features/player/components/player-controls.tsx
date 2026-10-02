import { useCallback, useEffect, useRef, useState } from 'react';
import {
  Download,
  Gauge,
  ListVideo,
  Maximize,
  Minimize,
  Pause,
  PictureInPicture2,
  Play,
  SkipBack,
  SkipForward,
  Volume2,
  VolumeX,
} from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Sheet, SheetContent, SheetHeader, SheetTitle } from '@/components/ui/sheet';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { formatDuration } from '@/lib/format';
import { cn } from '@/lib/utils';
import { t } from '@/i18n';
import { DownloadSheet } from './download-sheet';
import type { Episode } from '@/lib/schema';

/** 倍速档位与主流播放器一致，用户不用猜。 */
const RATES = [0.75, 1, 1.25, 1.5, 2, 3];

/** 播放中静止多久后隐藏控件（毫秒）。 */
const HIDE_DELAY_MS = 3_000;

/**
 * 控件按钮的统一样式。
 *
 * 悬浮层压在深色视频上，必须固定白字。默认 `ghost` 变体的 hover 底色来自
 * 主题 token，浅色主题下会变成一块浅灰糊在画面上，所以这里覆盖掉。
 * 常态完全透明——只有鼠标真的移上去才给一点反馈。
 */
const CHROME_BUTTON = 'h-8 gap-1.5 bg-transparent text-white hover:bg-white/20 hover:text-white';

interface Props {
  videoRef: React.RefObject<HTMLVideoElement | null>;
  /** 放大镜用的容器：全屏时进的是它，不是整个窗口 */
  stageRef: React.RefObject<HTMLDivElement | null>;
  seriesId: string;
  episodes: Episode[];
  currentIndex: number;
  onSelectEpisode: (vidIndex: number) => void;
  /**
   * 下载面板是否打开。
   *
   * 状态由播放器持有而不是控件内部：连播要不要继续只有播放器知道，
   * 控件自己藏起来的话，播完自动下一集照样切集，
   * `PlayerView` 带着 key 整体重挂载，面板和勾选一起被吃掉。
   */
  downloading: boolean;
  onDownloadingChange: (open: boolean) => void;
}

/**
 * 自绘播放控件。
 *
 * 不用原生 `controls`：它既不跟主题，也放不下「下载到本地 / 选集」这类业务动作。
 * 媒体状态（进度、时长、音量、倍速）全部由本组件持有——原生控件撤掉后，
 * 没人再替我们发 `timeupdate`，状态只能自己接。
 */
export function PlayerControls({
  videoRef,
  stageRef,
  seriesId,
  episodes,
  currentIndex,
  onSelectEpisode,
  downloading,
  onDownloadingChange,
}: Props) {
  const [current, setCurrent] = useState(0);
  const [duration, setDuration] = useState(0);
  const [paused, setPaused] = useState(true);
  const [volume, setVolume] = useState(1);
  const [muted, setMuted] = useState(false);
  const [rate, setRate] = useState(1);
  const [fullscreen, setFullscreen] = useState(false);
  const [picking, setPicking] = useState(false);
  /** 悬浮层可见性：播放中无操作 3 秒后隐藏 */
  const [chromeVisible, setChromeVisible] = useState(true);
  const hideTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  /** 拖动进度时不要让 timeupdate 把用户正在拖的位置冲掉 */
  const scrubbing = useRef(false);

  /**
   * 悬浮层跟随鼠标：进入视频区显示，静止 3 秒后隐藏，移出视频区立刻隐藏。
   *
   * 暂停时永远显示——画面停住却把控件也藏了，用户只会以为界面卡死。
   * 这个「暂停时可见」用派生值算，不在 effect 里同步 setState。
   */
  const chromeShown = paused || chromeVisible;

  const showChrome = useCallback(() => {
    setChromeVisible(true);
    if (hideTimer.current) clearTimeout(hideTimer.current);
    hideTimer.current = setTimeout(() => setChromeVisible(false), HIDE_DELAY_MS);
  }, []);

  useEffect(() => {
    const stage = stageRef.current;
    if (!stage || paused) {
      if (hideTimer.current) clearTimeout(hideTimer.current);
      return;
    }

    // 初始状态就是可见的，这里只启动倒计时（不在 effect 里同步 setState）
    if (hideTimer.current) clearTimeout(hideTimer.current);
    hideTimer.current = setTimeout(() => setChromeVisible(false), HIDE_DELAY_MS);

    const onLeave = () => setChromeVisible(false);
    stage.addEventListener('mousemove', showChrome);
    stage.addEventListener('mouseenter', showChrome);
    stage.addEventListener('mouseleave', onLeave);

    return () => {
      stage.removeEventListener('mousemove', showChrome);
      stage.removeEventListener('mouseenter', showChrome);
      stage.removeEventListener('mouseleave', onLeave);
      if (hideTimer.current) clearTimeout(hideTimer.current);
    };
  }, [paused, showChrome, stageRef]);

  useEffect(() => {
    const video = videoRef.current;
    if (!video) return;

    const onTime = () => {
      if (!scrubbing.current) setCurrent(video.currentTime);
    };
    const onMeta = () => setDuration(Number.isFinite(video.duration) ? video.duration : 0);
    const onPlay = () => setPaused(false);
    const onPauseEvt = () => setPaused(true);
    const onVolume = () => {
      setVolume(video.volume);
      setMuted(video.muted);
    };
    const onRate = () => setRate(video.playbackRate);
    const onFull = () => setFullscreen(document.fullscreenElement !== null);

    video.addEventListener('timeupdate', onTime);
    video.addEventListener('loadedmetadata', onMeta);
    video.addEventListener('durationchange', onMeta);
    video.addEventListener('play', onPlay);
    video.addEventListener('playing', onPlay);
    video.addEventListener('pause', onPauseEvt);
    video.addEventListener('volumechange', onVolume);
    video.addEventListener('ratechange', onRate);
    document.addEventListener('fullscreenchange', onFull);

    // 元素可能在挂载后才拿到 src 变化，进来先同步一次
    onMeta();
    onVolume();
    onRate();
    onPauseEvt();

    return () => {
      video.removeEventListener('timeupdate', onTime);
      video.removeEventListener('loadedmetadata', onMeta);
      video.removeEventListener('durationchange', onMeta);
      video.removeEventListener('play', onPlay);
      video.removeEventListener('playing', onPlay);
      video.removeEventListener('pause', onPauseEvt);
      video.removeEventListener('volumechange', onVolume);
      video.removeEventListener('ratechange', onRate);
      document.removeEventListener('fullscreenchange', onFull);
    };
  }, [videoRef]);

  const seekTo = useCallback(
    (ratio: number) => {
      const video = videoRef.current;
      if (!video || !Number.isFinite(video.duration) || video.duration <= 0) return;
      const next = Math.min(Math.max(ratio, 0), 1) * video.duration;
      video.currentTime = next;
      setCurrent(next);
    },
    [videoRef]
  );

  const togglePlay = useCallback(() => {
    const video = videoRef.current;
    if (!video) return;
    if (video.paused) void video.play().catch(() => undefined);
    else video.pause();
  }, [videoRef]);

  const skip = useCallback(
    (delta: number) => {
      const video = videoRef.current;
      if (!video) return;
      const next = Math.min(Math.max(video.currentTime + delta, 0), video.duration || 0);
      video.currentTime = next;
      setCurrent(next);
    },
    [videoRef]
  );

  const toggleMute = useCallback(() => {
    const video = videoRef.current;
    if (!video) return;
    video.muted = !video.muted;
  }, [videoRef]);

  const toggleFullscreen = useCallback(() => {
    const stage = stageRef.current;
    if (!stage) return;
    if (document.fullscreenElement) void document.exitFullscreen();
    else void stage.requestFullscreen().catch(() => undefined);
  }, [stageRef]);

  // 画中画不是所有 WebView2 版本都支持，不支持就别摆一个点了的按钮
  const pipSupported =
    typeof document !== 'undefined' && document.pictureInPictureEnabled === true;

  const togglePip = useCallback(() => {
    const video = videoRef.current;
    if (!video) return;
    if (document.pictureInPictureElement) void document.exitPictureInPicture();
    else void video.requestPictureInPicture().catch(() => undefined);
  }, [videoRef]);

  const applyRate = useCallback(
    (next: number) => {
      const video = videoRef.current;
      if (!video) return;
      video.playbackRate = next;
    },
    [videoRef]
  );

  return (
    <div
      className={cn(
        // 纯悬浮：不要任何底色/渐变蒙版，就一组裸图标压在画面上。
        // 可读性靠白色 + 投影，不靠底板——底板一加就变成一条色块，破坏了画面。
        'absolute inset-x-0 bottom-0 flex flex-col gap-2 px-4 pt-10 pb-3 text-white',
        'drop-shadow-[0_1px_3px_rgba(0,0,0,0.85)]',
        'transition-opacity duration-200',
        chromeShown ? 'opacity-100' : 'pointer-events-none opacity-0',
      )}
      onMouseMove={showChrome}
    >
      <ScrubBar
        current={current}
        duration={duration}
        onScrubStart={() => {
          scrubbing.current = true;
        }}
        onScrubEnd={() => {
          scrubbing.current = false;
        }}
        onSeek={seekTo}
      />

      <div className="flex items-center gap-1">
        <IconButton label={t('player.play')} onClick={togglePlay}>
          {paused ? <Play className="size-4" /> : <Pause className="size-4" />}
        </IconButton>
        <IconButton label={t('player.skipBack')} onClick={() => skip(-5)}>
          <SkipBack className="size-4" />
        </IconButton>
        <IconButton label={t('player.skipForward')} onClick={() => skip(5)}>
          <SkipForward className="size-4" />
        </IconButton>

        <span className="ml-1 font-mono text-xs text-white/90 tabular-nums">
          {formatDuration(current)} / {formatDuration(duration)}
        </span>

        <div className="ml-auto flex items-center gap-1">
          <div className="group/vol flex items-center">
            <IconButton label={t('player.mute')} onClick={toggleMute}>
              {muted || volume === 0 ? (
                <VolumeX className="size-4" />
              ) : (
                <Volume2 className="size-4" />
              )}
            </IconButton>
            <input
              type="range"
              min={0}
              max={1}
              step={0.05}
              value={muted ? 0 : volume}
              onChange={(e) => {
                const video = videoRef.current;
                if (!video) return;
                video.volume = Number(e.target.value);
                video.muted = Number(e.target.value) === 0;
              }}
              aria-label={t('player.volume')}
              // 厂商伪元素样式写在 index.css 的 .volume-range 里，
              // Tailwind 变体压不住原生 range 的默认蓝色滑块
              className="volume-range w-0 opacity-0 transition-all group-hover/vol:w-20 group-hover/vol:opacity-100 focus:w-20 focus:opacity-100"
            />
          </div>

          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button variant="ghost" size="sm" className={cn(CHROME_BUTTON, 'font-mono')}>
                <Gauge className="size-4" />
                {rate}x
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end">
              <DropdownMenuLabel>{t('player.playbackRate')}</DropdownMenuLabel>
              {RATES.map((r) => (
                <DropdownMenuItem key={r} onSelect={() => applyRate(r)}>
                  <span className="font-mono">{r}x</span>
                  {r === rate && <span className="ml-auto text-xs">✓</span>}
                </DropdownMenuItem>
              ))}
            </DropdownMenuContent>
          </DropdownMenu>

              <Button
                variant="ghost"
                size="sm"
                className={CHROME_BUTTON}
                onClick={() => setPicking(true)}
              >
                <ListVideo className="size-4" />
                {t('player.episodes')}
              </Button>

              <Button
                variant="ghost"
                size="sm"
                className={CHROME_BUTTON}
                onClick={() => onDownloadingChange(true)}
              >
                <Download className="size-4" />
                {t('player.download')}
              </Button>

          {pipSupported && (
            <IconButton label={t('player.pictureInPicture')} onClick={togglePip}>
              <PictureInPicture2 className="size-4" />
            </IconButton>
          )}

          <IconButton label={t('player.fullscreen')} onClick={toggleFullscreen}>
            {fullscreen ? <Minimize className="size-4" /> : <Maximize className="size-4" />}
          </IconButton>
        </div>
      </div>

      <Sheet open={picking} onOpenChange={setPicking}>
        <SheetContent side="bottom" className="max-h-[70vh]">
          <SheetHeader>
            <SheetTitle>{t('player.pickEpisode')}</SheetTitle>
          </SheetHeader>
          <div className="grid grid-cols-8 gap-1 overflow-y-auto py-2 md:grid-cols-12">
            {episodes.map((ep) => (
              <Button
                key={ep.vidIndex}
                variant={ep.vidIndex === currentIndex ? 'default' : 'outline'}
                size="sm"
                className="h-8 font-mono tabular-nums"
                onClick={() => {
                  onSelectEpisode(ep.vidIndex);
                  setPicking(false);
                }}
              >
                {ep.vidIndex}
              </Button>
            ))}
          </div>
        </SheetContent>
      </Sheet>

      <DownloadSheet
        seriesId={seriesId}
        episodes={episodes}
        defaultSelected={[currentIndex]}
        open={downloading}
        onOpenChange={onDownloadingChange}
      />
    </div>
  );
}

function IconButton({
  label,
  onClick,
  children,
}: {
  label: string;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <Button
      variant="ghost"
      size="icon"
      className="size-8 bg-transparent text-white hover:bg-white/20 hover:text-white"
      onClick={onClick}
      title={label}
      aria-label={label}
    >
      {children}
    </Button>
  );
}

/**
 * 可点击可拖动的进度条。
 *
 * 用 div 而不是 `<input type=range>`：要显示缓冲进度、要跟随容器宽度，
 * 而 range 的原生滑块样式在 WebView2 上跨版本表现不一致。
 */
function ScrubBar({
  current,
  duration,
  onSeek,
  onScrubStart,
  onScrubEnd,
}: {
  current: number;
  duration: number;
  onSeek: (ratio: number) => void;
  onScrubStart: () => void;
  onScrubEnd: () => void;
}) {
  const trackRef = useRef<HTMLDivElement>(null);
  const ratio = duration > 0 ? Math.min(current / duration, 1) : 0;

  const ratioAt = (clientX: number) => {
    const track = trackRef.current;
    if (!track) return 0;
    const rect = track.getBoundingClientRect();
    if (rect.width <= 0) return 0;
    return (clientX - rect.left) / rect.width;
  };

  return (
    <div
      ref={trackRef}
      role="slider"
      aria-label="进度"
      aria-valuemin={0}
      aria-valuemax={Math.floor(duration)}
      aria-valuenow={Math.floor(current)}
      tabIndex={0}
      className="group/bar relative h-4 w-full cursor-pointer"
      onPointerDown={(e) => {
        if (duration <= 0) return;
        e.currentTarget.setPointerCapture(e.pointerId);
        onScrubStart();
        onSeek(ratioAt(e.clientX));
      }}
      onPointerMove={(e) => {
        if (e.buttons === 1 && duration > 0) onSeek(ratioAt(e.clientX));
      }}
      onPointerUp={() => onScrubEnd()}
      onKeyDown={(e) => {
        if (e.key === 'ArrowLeft') onSeek(Math.max(ratio - 5 / (duration || 1), 0));
        if (e.key === 'ArrowRight') onSeek(Math.min(ratio + 5 / (duration || 1), 1));
      }}
    >
      {/* 进度条同样固定白色系，不跟主题走 */}
      <div className="absolute inset-x-0 top-1/2 h-1 -translate-y-1/2 rounded-full bg-white/25" />
      <div
        className="absolute top-1/2 left-0 h-1 -translate-y-1/2 rounded-full bg-white"
        style={{ width: `${ratio * 100}%` }}
      />
      <div
        className="absolute top-1/2 size-3 -translate-x-1/2 -translate-y-1/2 rounded-full bg-white opacity-0 transition-opacity group-hover/bar:opacity-100"
        style={{ left: `${ratio * 100}%` }}
      />
    </div>
  );
}
