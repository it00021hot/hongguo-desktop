/** 自绘播放控件主体：媒体状态同步、菜单与面板编排；RATES/CHROME_BUTTON 为本组件专用常量。 */
import { useCallback, useEffect, useRef, useState } from 'react';
import {
  Download,
  Eye,
  Gauge,
  ListVideo,
  MonitorPlay,
  Pause,
  Play,
  RectangleVertical,
  SkipBack,
  SkipForward,
  Sparkles,
} from 'lucide-react';
import { Button } from '@/components/ui/button';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { useWindowFullscreen, applyWindowFullscreen } from '@/hooks/use-window-fullscreen';
import { usePlayerStore } from '@/stores/player';
import { useUiStore } from '@/stores/ui';
import { app as appApi } from '@/service/commands';
import { formatDuration } from '@/utils/format';
import { cn } from '@/lib/utils';
import { t, tf } from '@/locales';
import type { DanmakuDisplaySettings, EnhancementSettings } from '@/utils/playback-prefs';
import type { EnhancementStatus } from '../../hooks/use-enhancement';
import { DownloadSheet } from '../download-sheet';
import { EpisodePicker } from '../episode-picker';
import { ChromeSurface } from '../chrome-surface';
import type { Episode, VideoDefinition } from '@/service/schema';
import { DanmakuSendBox } from './danmaku-send-box';
import {
  DanmakuSettingsIcon,
  DanmakuSwitchIcon,
  FullscreenIcon,
  MiniWindowIcon,
  WebFullscreenIcon,
} from './player-icons';
import { ScrubBar } from './scrub-bar';
import { DisplaySlider } from './sliders';
import { VolumePopup } from './volume-popup';
import { IconButton } from './icon-button';
import { markPickerClosed } from '../../playback-signals';

/** 倍速档位与主流播放器一致，用户不用猜。 */
const RATES = [0.75, 1, 1.25, 1.5, 2, 3];

/**
 * 控件按钮统一样式：悬浮层压在深色视频上必须固定白字（ghost 变体 hover 底色来自
 * 主题 token，浅色主题下会糊在画面上，所以覆盖）；常态完全透明。
 */
const CHROME_BUTTON = 'h-8 gap-1.5 bg-transparent text-white hover:bg-white/20 hover:text-white';

interface Props {
  videoRef: React.RefObject<HTMLVideoElement | null>;
  /** 弹幕开关（状态在播放页，控件只做展示与回调） */
  danmakuOn: boolean;
  onToggleDanmaku: () => void;
  /** 弹幕显示设置与修改回调（透明度/字号/密度/显示区域） */
  danmakuDisplay: DanmakuDisplaySettings;
  onDanmakuDisplayChange: (patch: Partial<DanmakuDisplaySettings>) => void;
  /** 画质增强设置与修改回调（档位/锐化强度）；状态由播放页的增强层回写 */
  enhanceSettings: EnhancementSettings;
  onEnhanceChange: (patch: Partial<EnhancementSettings>) => void;
  enhanceStatus: EnhancementStatus;
  /** 画面可用（有流且已出画）才允许开启增强 */
  enhanceAvailable: boolean;
  seriesId: string;
  episodes: Episode[];
  currentIndex: number;
  /**
   * 下载面板开合状态由播放器持有：连播要不要继续只有播放器知道，
   * 控件自己藏起来的话播完自动下一集照样切集，面板和勾选一起被吃掉。
   */
  downloading: boolean;
  onDownloadingChange: (open: boolean) => void;
  /** 当前实际生效的清晰度档位。0 = 本地文件或尚未取到 */
  definition: number;
  automaticDefinition: boolean;
  /** 本集提供的全部档位。只有一档时不渲染切换菜单 */
  definitions: VideoDefinition[];
  /**
   * 当前播放地址。`<video>` 以它为 `key`，换清晰度就会重建元素，所以它同时是
   * 控件 effect 的依赖键：地址变了事件监听跟着重建，绑在旧元素上新元素的 play 事件收不到。
   */
  src: string | null;
  /** 选清晰度。`undefined` 表示交回后端自动策略。 */
  onDefinitionChange: (definition: number | undefined) => void;
  /** 步进一集：-1 上一集，+1 下一集（与 `↑` `↓` 快捷键同一逻辑） */
  onStepEpisode: (delta: number) => void;
  /** 进入小屏播放：只挂按钮，状态编排都在宿主（见 use-mini-window）。 */
  onOpenMini: () => void;
  /** 进入竖屏播放（几何与 IME 组合都收在 use-portrait-window）；竖屏形态控件由 MiniScreenControls 承担，本组件只在横屏出现。 */
  onEnterPortrait: () => void;
  /** 隐身模式开关（鼠标离开窗口自动隐藏 + 暂停）。状态在播放页。 */
  incognito: boolean;
  onToggleIncognito: () => void;
  /** 沉浸流形态开关：为 true 时控制栏带「选集」入口。 */
  immersive?: boolean;
  /** 选集浮层里点选某一集（跳集，由播放器接 store） */
  onPickEpisode?: (vidIndex: number) => void;
  /** 悬浮层可见性（受控）：裁决在 PlayerView，这里不自养定时器（两套定时器会出现「控制栏还在、简介没了」）。 */
  visible: boolean;
  /** 指针进入/离开控制栏本体：悬在控制栏上时不许静止倒计时收起（宿主页裁决） */
  onControlsEnter?: () => void;
  onControlsLeave?: () => void;
}

/**
 * 自绘播放控件。不用原生 `controls`（不跟主题、放不下业务动作）；媒体状态全部
 * 由本组件持有（原生控件撤掉后没人再发 timeupdate）。快进/快退由快捷键承担，
 * 步进按钮是上一集/下一集；完整选集在「选集」浮层。
 */
export function PlayerControls({
  videoRef,
  danmakuOn,
  onToggleDanmaku,
  danmakuDisplay,
  onDanmakuDisplayChange,
  enhanceSettings,
  onEnhanceChange,
  enhanceStatus,
  enhanceAvailable,
  seriesId,
  episodes,
  currentIndex,
  downloading,
  onDownloadingChange,
  definition,
  automaticDefinition,
  definitions,
  onDefinitionChange,
  src,
  onStepEpisode,
  onOpenMini,
  onEnterPortrait,
  incognito,
  onToggleIncognito,
  immersive,
  onPickEpisode,
  visible,
  onControlsEnter,
  onControlsLeave,
}: Props) {
  const [current, setCurrent] = useState(0);
  const [duration, setDuration] = useState(0);
  const [paused, setPaused] = useState(true);
  const [volume, setVolume] = useState(1);
  const [muted, setMuted] = useState(false);
  const [rate, setRate] = useState(1);
  // 全屏：窗口级真值（Esc/绿按钮变化经 useWindowFullscreenSync 回流）
  const { fullscreen, toggleFullscreen } = useWindowFullscreen();
  // 无边框播放与全屏互斥：点全屏先退无边框，点无边框先退全屏
  const frameless = useUiStore((s) => s.playerFrameless);
  const setFrameless = useUiStore((s) => s.setPlayerFrameless);
  const toggleFrameless = () => {
    const next = !frameless;
    if (next && useUiStore.getState().windowFullscreen) {
      void applyWindowFullscreen(false).catch(() => undefined);
    }
    setFrameless(next);
    void appApi.setTrafficLightsVisible(!next).catch(() => undefined);
  };
  const onToggleFullscreen = () => {
    if (!fullscreen && useUiStore.getState().playerFrameless) {
      setFrameless(false);
      void appApi.setTrafficLightsVisible(true).catch(() => undefined);
    }
    toggleFullscreen();
  };
  // 面板/浮层开合在全局播放 store：切集/切剧重挂载不丢
  const danmakuPanelOpen = usePlayerStore((s) => s.danmakuPanelOpen);
  const setDanmakuPanelOpen = usePlayerStore((s) => s.setDanmakuPanelOpen);
  const enhancePanelOpen = usePlayerStore((s) => s.enhancePanelOpen);
  const setEnhancePanelOpen = usePlayerStore((s) => s.setEnhancePanelOpen);
  const seriesPanelOpen = usePlayerStore((s) => s.seriesPanelOpen);
  const setSeriesPanelOpen = usePlayerStore((s) => s.setSeriesPanelOpen);
  const danmakuPanelRef = useRef<HTMLDivElement | null>(null);
  const enhancePanelRef = useRef<HTMLDivElement | null>(null);
  /** 选集按钮 + 浮层的锚点容器：点外部关闭时用它排除「点在选集按钮上」 */
  const seriesAnchorRef = useRef<HTMLDivElement | null>(null);

  /** 拖动进度时不要让 timeupdate 把用户正在拖的位置冲掉 */
  const scrubbing = useRef(false);

  /** 当前集的 vid（发弹幕对象；档案未就绪为空串，发送时 guard） */
  const currentVid = episodes.find((e) => e.vidIndex === currentIndex)?.vid ?? '';

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

    video.addEventListener('timeupdate', onTime);
    video.addEventListener('loadedmetadata', onMeta);
    video.addEventListener('durationchange', onMeta);
    video.addEventListener('play', onPlay);
    video.addEventListener('playing', onPlay);
    video.addEventListener('pause', onPauseEvt);
    video.addEventListener('volumechange', onVolume);
    video.addEventListener('ratechange', onRate);

    // 元素可能在挂载后才拿到 src 变化，进来先同步一次；小屏切回大屏时控件对正在播的流挂载，不补同步进度条会闪 0:00
    onMeta();
    onTime();
    onVolume();
    onRate();
    // 播放态双向同步：小屏切回大屏时元素正在播、play 事件不会再来了，不推一把按钮永远停在暂停态
    if (video.paused) onPauseEvt();
    else onPlay();

    return () => {
      video.removeEventListener('timeupdate', onTime);
      video.removeEventListener('loadedmetadata', onMeta);
      video.removeEventListener('durationchange', onMeta);
      video.removeEventListener('play', onPlay);
      video.removeEventListener('playing', onPlay);
      video.removeEventListener('pause', onPauseEvt);
      video.removeEventListener('volumechange', onVolume);
      video.removeEventListener('ratechange', onRate);
    };
    // `src` 进依赖：换清晰度时 `<video>` 以它为 key 被重建，
    // 监听器不跟着重建就会一直绑在已卸载的旧元素上。
  }, [videoRef, src]);

  const seekTo = useCallback(
    (ratio: number) => {
      const video = videoRef.current;
      if (!video || !Number.isFinite(video.duration) || video.duration <= 0) return;
      const next = Math.min(Math.max(ratio, 0), 1) * video.duration;
      video.currentTime = next;
      setCurrent(next);
    },
    [videoRef],
  );

  const togglePlay = useCallback(() => {
    const video = videoRef.current;
    if (!video) return;
    if (video.paused) void video.play().catch(() => undefined);
    else video.pause();
  }, [videoRef]);

  // 步进边界：第 1 集没有上一集；下一集要看分集表里是否真有下一号
  // （表还没加载出来时不置灰，保持与 `↑` `↓` 一致的行为——点了由后端兜底）
  const hasPrev = currentIndex > 1;
  const hasNext = episodes.length === 0 || episodes.some((e) => e.vidIndex === currentIndex + 1);

  const toggleMute = useCallback(() => {
    const video = videoRef.current;
    if (!video) return;
    video.muted = !video.muted;
  }, [videoRef]);

  const applyRate = useCallback(
    (next: number) => {
      const video = videoRef.current;
      if (!video) return;
      video.playbackRate = next;
    },
    [videoRef],
  );

  // 弹幕面板点外部关闭（面板在 ref 容器里，拖滑条不会误关）
  useEffect(() => {
    if (!danmakuPanelOpen) return;
    const onDown = (e: MouseEvent) => {
      if (!danmakuPanelRef.current?.contains(e.target as Node)) setDanmakuPanelOpen(false);
    };
    document.addEventListener('mousedown', onDown);
    return () => document.removeEventListener('mousedown', onDown);
  }, [danmakuPanelOpen, setDanmakuPanelOpen]);

  // 画质增强面板同款：点外部关闭
  useEffect(() => {
    if (!enhancePanelOpen) return;
    const onDown = (e: MouseEvent) => {
      if (!enhancePanelRef.current?.contains(e.target as Node)) setEnhancePanelOpen(false);
    };
    document.addEventListener('mousedown', onDown);
    return () => document.removeEventListener('mousedown', onDown);
  }, [enhancePanelOpen, setEnhancePanelOpen]);

  // 选集浮层点外部关闭：选中集数不关浮层（连选连跳）；mousedown 先关、click 后到，
  // 关浮层打进点击冷却这一下不会变成暂停
  useEffect(() => {
    if (!seriesPanelOpen) return;
    const onDown = (e: MouseEvent) => {
      if (!seriesAnchorRef.current?.contains(e.target as Node)) {
        markPickerClosed();
        setSeriesPanelOpen(false);
      }
    };
    document.addEventListener('mousedown', onDown);
    return () => document.removeEventListener('mousedown', onDown);
  }, [seriesPanelOpen, setSeriesPanelOpen]);

  const setVolumeValue = useCallback(
    (v: number) => {
      const video = videoRef.current;
      if (!video) return;
      video.volume = v;
      video.muted = v === 0;
    },
    [videoRef],
  );

  return (
    <ChromeSurface
      shown={visible}
      data-wheel-block
      onMouseEnter={onControlsEnter}
      onMouseLeave={onControlsLeave}
      className={cn(
        // 纯悬浮：无底色蒙版，可读性靠白色 + 投影。root 自带 z-30 抬整层（非 none 的
        // filter 会把本层变成 stacking context，困住向上弹的浮层），pointer-events-none
        // 让出指针（透明 padding 不截获悬停/点击/滚轮），两行内容可见时 auto 接回
        'pointer-events-none absolute inset-x-0 bottom-0 z-30 flex flex-col gap-2 px-4 pt-10 pb-3 text-white',
        'drop-shadow-[0_1px_3px_rgba(0,0,0,0.85)]',
      )}
    >
      <div className={cn(visible && 'pointer-events-auto')}>
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
      </div>

      <div className={cn('flex items-center gap-1', visible && 'pointer-events-auto')}>
        <IconButton label={t('player.play')} onClick={togglePlay}>
          {paused ? <Play className="size-4" /> : <Pause className="size-4" />}
        </IconButton>
        <IconButton
          label={t('player.prevEpisode')}
          disabled={!hasPrev}
          onClick={() => onStepEpisode(-1)}
        >
          <SkipBack className="size-4" />
        </IconButton>
        <IconButton
          label={t('player.nextEpisode')}
          disabled={!hasNext}
          onClick={() => onStepEpisode(1)}
        >
          <SkipForward className="size-4" />
        </IconButton>

        <span className="ml-1 shrink-0 font-mono text-xs whitespace-nowrap text-white/90 tabular-nums">
          {formatDuration(current)} / {formatDuration(duration)}
        </span>

        {/* 弹幕发送框（常驻）；窄舞台下允许收缩，输入框 min-w-0 兜底 */}
        <DanmakuSendBox vid={currentVid ? `${currentVid}:${seriesId}` : ''} currentSec={current} />

        <div className="ml-auto flex items-center gap-1">
          {/* modal={false}：modal 菜单会把 body 设成 pointer-events:none，控件重挂载撞上就是大片区域点不动 */}
          <DropdownMenu modal={false}>
            <DropdownMenuTrigger asChild>
              <Button variant="ghost" size="sm" className={cn(CHROME_BUTTON, 'font-mono')}>
                <Gauge className="size-4" />
                {rate}x
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end" data-wheel-block>
              <DropdownMenuLabel>{t('player.playbackRate')}</DropdownMenuLabel>
              {RATES.map((r) => (
                <DropdownMenuItem key={r} onSelect={() => applyRate(r)}>
                  <span className="font-mono">{r}x</span>
                  {r === rate && <span className="ml-auto text-xs">✓</span>}
                </DropdownMenuItem>
              ))}
            </DropdownMenuContent>
          </DropdownMenu>

          {/* 清晰度切换：本地已下载的集只有一版（definitions 空）时按钮置灰并说明原因，
              直接不渲染会让用户以为功能本来就没有 */}
          <DropdownMenu modal={false}>
            <DropdownMenuTrigger asChild>
              <Button
                variant="ghost"
                size="sm"
                className={cn(CHROME_BUTTON, 'font-mono')}
                disabled={definitions.length === 0}
                aria-label={t('player.definition')}
                title={
                  definitions.length === 0
                    ? t('player.definitionLocalOnly')
                    : automaticDefinition && definition > 0
                      ? `${t('player.definitionAuto')} · ${definition}P`
                      : undefined
                }
              >
                <MonitorPlay className="size-4" />
                {automaticDefinition || definition === 0
                  ? t('player.definitionAuto')
                  : `${definition}P`}
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end" data-wheel-block>
              <DropdownMenuLabel>{t('player.definition')}</DropdownMenuLabel>
              <DropdownMenuItem onSelect={() => onDefinitionChange(undefined)}>
                {t('player.definitionAuto')}
                {automaticDefinition && <span className="ml-auto text-xs">✓</span>}
              </DropdownMenuItem>
              {definitions.map((d) => (
                <DropdownMenuItem
                  key={d.value}
                  onSelect={() => onDefinitionChange(d.value)}
                  className="justify-between"
                >
                  <span className="font-mono">
                    {d.value}P{/* 竖屏剧的宽高是反的，只报分辨率会误导 */}
                    {d.height > d.width && (
                      <span className="text-muted-foreground ml-2 text-xs">
                        {d.width}×{d.height}
                      </span>
                    )}
                  </span>
                  {!automaticDefinition && definition === d.value && (
                    <span className="ml-auto text-xs">✓</span>
                  )}
                </DropdownMenuItem>
              ))}
            </DropdownMenuContent>
          </DropdownMenu>

          {/* 画质增强：档位三段选 + 锐化强度滑条 + 状态说明，与弹幕面板互斥；无流可用时置灰 */}
          <div ref={enhancePanelRef} className="relative flex items-center">
            <IconButton
              label={t('player.enhanceSettings')}
              disabled={!enhanceAvailable}
              onClick={() => {
                const next = !enhancePanelOpen;
                setEnhancePanelOpen(next);
                if (next) setDanmakuPanelOpen(false);
              }}
            >
              <Sparkles
                className={cn(
                  'size-4',
                  enhanceSettings.anime4kOn || enhanceSettings.sharpenOn || enhancePanelOpen
                    ? 'text-white'
                    : 'text-white/70',
                )}
              />
            </IconButton>
            {enhancePanelOpen && (
              <div className="absolute right-0 bottom-full z-30 mb-3 w-60 rounded-xl border border-white/10 bg-black/85 p-4 backdrop-blur-sm">
                <div className="flex flex-col gap-4">
                  {/* 两个独立开关，可同时开：Anime4K 放大后偏软，叠加锐化
                      才是完整形态。都关 = 原画 */}
                  <div className="flex gap-1">
                    <button
                      type="button"
                      aria-pressed={enhanceSettings.anime4kOn}
                      onClick={() => onEnhanceChange({ anime4kOn: !enhanceSettings.anime4kOn })}
                      className={cn(
                        'flex-1 rounded-lg px-2 py-1.5 text-xs transition-colors',
                        enhanceSettings.anime4kOn
                          ? 'bg-white text-black'
                          : 'bg-white/10 text-white/80 hover:bg-white/20',
                      )}
                    >
                      {t('player.enhanceAnime4k')}
                    </button>
                    <button
                      type="button"
                      aria-pressed={enhanceSettings.sharpenOn}
                      onClick={() => onEnhanceChange({ sharpenOn: !enhanceSettings.sharpenOn })}
                      className={cn(
                        'flex-1 rounded-lg px-2 py-1.5 text-xs transition-colors',
                        enhanceSettings.sharpenOn
                          ? 'bg-white text-black'
                          : 'bg-white/10 text-white/80 hover:bg-white/20',
                      )}
                    >
                      {t('player.enhanceSharpen')}
                    </button>
                  </div>
                  {enhanceSettings.sharpenOn && (
                    <DisplaySlider
                      label={t('player.enhanceStrength')}
                      value={enhanceSettings.strength / 100}
                      min={0}
                      max={1}
                      onChange={(v) => onEnhanceChange({ strength: Math.round(v * 100) })}
                    />
                  )}
                  <p className="text-[11px] leading-relaxed text-white/60">
                    {t('player.enhanceNote')}
                  </p>
                  {enhanceSettings.anime4kOn && (
                    <p className="text-[11px] leading-relaxed text-white/60">
                      {t('player.enhanceAnime4kNote')}
                    </p>
                  )}
                  {enhanceStatus.message && (
                    <p
                      role="status"
                      className={cn(
                        'text-[11px] leading-relaxed',
                        enhanceStatus.state === 'error' ? 'text-white' : 'text-white/60',
                      )}
                    >
                      {enhanceStatus.message}
                    </p>
                  )}
                </div>
              </div>
            )}
          </div>

          {immersive && (
            <div ref={seriesAnchorRef} className="relative flex items-center">
              <Button
                variant="ghost"
                size="sm"
                className={CHROME_BUTTON}
                onClick={() => setSeriesPanelOpen(!seriesPanelOpen)}
              >
                <ListVideo className="size-4" />
                {episodes.length > 0
                  ? `${t('player.episodes')} · ${tf('player.totalEpisodes', { count: episodes.length })}`
                  : t('player.episodes')}
              </Button>
              {seriesPanelOpen && (
                // 贴按钮向上弹。宽度写死在 wrapper（% 会相对按钮宽度塌缩）；
                // z-30 压过互动栏(z-20)——浮层打开时互动栏可见可悬停，不抬层会盖住浮层右下几列
                <div className="absolute right-0 bottom-full z-30 mb-3 max-h-[62vh] w-[460px] max-w-[92vw] scrollbar-thin overflow-y-auto">
                  <EpisodePicker
                    seriesId={seriesId}
                    currentIndex={currentIndex}
                    onSelect={(idx) => onPickEpisode?.(idx)}
                    onClose={() => {
                      // 打点给舞台点击冷却用（浮层一关，跟手点击别变暂停）
                      markPickerClosed();
                      setSeriesPanelOpen(false);
                    }}
                  />
                </div>
              )}
            </div>
          )}

          <Button
            variant="ghost"
            size="sm"
            className={CHROME_BUTTON}
            onClick={() => onDownloadingChange(true)}
          >
            <Download className="size-4" />
            {t('player.download')}
          </Button>

          {/* 小屏播放：同一窗口缩成 480×270，播放不断——不是系统 PiP 也不是第二个窗口 */}
          <IconButton label={t('player.miniWindow')} onClick={onOpenMini}>
            <MiniWindowIcon />
          </IconButton>

          {/* 弹幕设置面板，与画质增强面板互斥 */}
          <div ref={danmakuPanelRef} className="relative flex items-center">
            <IconButton
              label={t('player.danmakuSettings')}
              onClick={() => {
                const next = !danmakuPanelOpen;
                setDanmakuPanelOpen(next);
                if (next) setEnhancePanelOpen(false);
              }}
            >
              <DanmakuSettingsIcon className={danmakuPanelOpen ? 'text-white' : 'text-white/70'} />
            </IconButton>
            {danmakuPanelOpen && (
              <div className="absolute right-0 bottom-full z-30 mb-3 w-60 rounded-xl border border-white/10 bg-black/85 p-4 backdrop-blur-sm">
                <div className="flex flex-col gap-4">
                  <DisplaySlider
                    label={t('player.danmakuOpacity')}
                    value={danmakuDisplay.opacity}
                    min={0.1}
                    max={1}
                    onChange={(v) => onDanmakuDisplayChange({ opacity: v })}
                  />
                  <DisplaySlider
                    label={t('player.danmakuFontSize')}
                    value={danmakuDisplay.fontScale}
                    min={0.5}
                    max={2}
                    onChange={(v) => onDanmakuDisplayChange({ fontScale: v })}
                  />
                  <DisplaySlider
                    label={t('player.danmakuDensity')}
                    value={danmakuDisplay.density}
                    min={0}
                    max={1}
                    onChange={(v) => onDanmakuDisplayChange({ density: v })}
                  />
                  <DisplaySlider
                    label={t('player.danmakuArea')}
                    value={danmakuDisplay.area}
                    min={0.25}
                    max={1}
                    onChange={(v) => onDanmakuDisplayChange({ area: v })}
                  />
                </div>
              </div>
            )}
          </div>

          <IconButton label={t('player.danmaku')} onClick={onToggleDanmaku}>
            <DanmakuSwitchIcon
              off={!danmakuOn}
              className={danmakuOn ? 'text-white' : 'text-white/40'}
            />
          </IconButton>

          {/* 音量：hover 弹出竖条浮层（绝对定位不占布局，旧的横向展开会把弹幕按钮挤走） */}
          <VolumePopup
            volume={volume}
            muted={muted}
            onToggleMute={toggleMute}
            onSetVolume={setVolumeValue}
          />

          {/* 隐身模式：开启后鼠标离开窗口 → 整窗透明 + 暂停，回来即恢复 */}
          <IconButton label={t('player.incognito')} onClick={onToggleIncognito}>
            <Eye className={cn('size-4', incognito ? 'text-white' : 'text-white/55')} aria-hidden />
          </IconButton>

          {/* 竖屏播放：窗口收窄成竖条包住画面；小屏不出现，竖屏中的控件是 MiniScreenControls 竖屏形态 */}
          <IconButton label={t('player.portrait')} onClick={onEnterPortrait}>
            <RectangleVertical className="size-4" />
          </IconButton>

          {/* 无边框播放：chrome 全藏但不进 OS 全屏，窗口保持原几何；与全屏互斥 */}
          <IconButton
            label={t(frameless ? 'player.exitFrameless' : 'player.frameless')}
            onClick={toggleFrameless}
          >
            <WebFullscreenIcon className={frameless ? 'text-white' : 'text-white/70'} />
          </IconButton>

          <IconButton label={t('player.fullscreen')} onClick={onToggleFullscreen}>
            <FullscreenIcon exit={fullscreen} />
          </IconButton>
        </div>
      </div>

      <DownloadSheet
        seriesId={seriesId}
        episodes={episodes}
        defaultSelected={[currentIndex]}
        open={downloading}
        onOpenChange={onDownloadingChange}
      />
    </ChromeSurface>
  );
}
