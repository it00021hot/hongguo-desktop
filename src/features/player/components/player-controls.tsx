import { useCallback, useEffect, useRef, useState } from 'react';
import {
  Download,
  Eye,
  Gauge,
  ListVideo,
  Maximize,
  Minimize,
  MonitorPlay,
  Pause,
  PictureInPicture2,
  Play,
  Settings2,
  SkipBack,
  SkipForward,
  MessageSquareText,
  Volume2,
  VolumeX,
} from 'lucide-react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { usePlayerStore } from '@/lib/stores/player';
import { useAccount, useSendDanmaku } from '@/lib/queries';
import { EmojiPickerButton } from './emoji-picker';
import { RichEmojiInput, type RichEmojiInputHandle } from './rich-emoji-input';
import { formatDuration } from '@/lib/format';
import { cn } from '@/lib/utils';
import { t, tf } from '@/i18n';
import type { DanmakuDisplaySettings } from '@/lib/playback-prefs';
import { DownloadSheet } from './download-sheet';
import { EpisodePicker } from './episode-picker';
import type { Episode, VideoDefinition } from '@/lib/schema';

/** 倍速档位与主流播放器一致，用户不用猜。 */
const RATES = [0.75, 1, 1.25, 1.5, 2, 3];

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
  /** 弹幕开关（状态在播放页，控件只做展示与回调） */
  danmakuOn: boolean;
  onToggleDanmaku: () => void;
  /** 弹幕显示设置与修改回调（透明度/字号/密度/显示区域） */
  danmakuDisplay: DanmakuDisplaySettings;
  onDanmakuDisplayChange: (patch: Partial<DanmakuDisplaySettings>) => void;
  /** 放大镜用的容器：全屏时进的是它，不是整个窗口 */
  stageRef: React.RefObject<HTMLDivElement | null>;
  seriesId: string;
  episodes: Episode[];
  currentIndex: number;
  /**
   * 下载面板是否打开。
   *
   * 状态由播放器持有而不是控件内部：连播要不要继续只有播放器知道，
   * 控件自己藏起来的话，播完自动下一集照样切集，
   * `PlayerView` 带着 key 整体重挂载，面板和勾选一起被吃掉。
   */
  downloading: boolean;
  onDownloadingChange: (open: boolean) => void;
  /** 当前实际生效的清晰度档位。0 = 本地文件或尚未取到 */
  definition: number;
  /** 本集提供的全部档位。只有一档时不渲染切换菜单 */
  definitions: VideoDefinition[];
  /**
   * 当前播放地址。
   *
   * `<video>` 以它为 `key`，换清晰度就会**重建元素**。控件的事件监听必须
   * 跟着重建：绑在旧元素上时，新元素的 `play` 事件收不到，
   * 播放按钮就会一直卡在「暂停」图标。
   */
  /**
   * 正在播的地址。**只用作 effect 依赖键**：切清晰度、或兜底转成 H.264 之后
   * 地址都会变，监听器要跟着重建，所以必须是精确值而不是旧元素的。
   */
  src: string | null;
  /** 选清晰度。`undefined` 表示交回后端自动取最高档 */
  onDefinitionChange: (definition: number | undefined) => void;
  /** 步进一集：-1 上一集，+1 下一集（与 `↑` `↓` 快捷键同一逻辑） */
  onStepEpisode: (delta: number) => void;
  /**
   * 进入小窗播放：主窗把当前进度落库后开置顶小窗并隐藏自己。
   * 状态与取流编排都在播放页，这里只挂按钮。
   */
  onOpenMini: () => void;
  /** 隐身模式开关（鼠标离开窗口自动隐藏 + 暂停）。状态在播放页。 */
  incognito: boolean;
  onToggleIncognito: () => void;
  /**
   * 沉浸流形态开关：为 true 时控制栏带「选集」入口（贴按钮向上弹的
   * 数字网格，hgplayer 同款）。播放页右侧已有 SeriesPanel，不传即无。
   */
  immersive?: boolean;
  /** 选集浮层里点选某一集（跳集，由播放器接 store） */
  onPickEpisode?: (vidIndex: number) => void;
  /**
   * 选集浮层底部的提示文案（信息流里教「选一集 = 进入本剧连播」）。
   * 播放页本来就在连播状态，不用教，不传即无。
   */
  pickerHint?: string;
  /**
   * 悬浮层可见性（受控）。
   *
   * 裁决在 PlayerView：静止倒计时 / 暂停 / 任一面板打开都在那一层算好，
   * 这里不再自养一套定时器——两套定时器各行其是时，就会出现
   * 「控制栏还在、简介没了」或反过来的精神分裂。
   */
  visible: boolean;
  /** 指针进入/离开控制栏本体：悬在控制栏上时不许静止倒计时收起（宿主页裁决） */
  onControlsEnter?: () => void;
  onControlsLeave?: () => void;
}

/** 控制栏内的弹幕发送框（hgplayer 同款：常驻控制栏左段）。
 *
 * 输入 Enter / 点「发送」提交；offset 取控件自己持有的播放秒数（实时）。
 * 未登录点发送给指路提示；vid 未就绪时静默忽略。
 * 表情：hgplayer 同款 `[名字]` 代码——选择器插入代码，弹幕层渲染成图。 */
function DanmakuSendBox({ vid, currentSec }: { vid: string; currentSec: number }) {
  const [text, setText] = useState('');
  const [emojiOpen, setEmojiOpen] = useState(false);
  const send = useSendDanmaku();
  const { data: account } = useAccount();
  const loggedIn = !!account;
  const richRef = useRef<RichEmojiInputHandle | null>(null);

  const pickEmoji = (name: string) => {
    richRef.current?.insertEmoji(name);
  };

  const submit = () => {
    const content = text.trim();
    if (!content || send.isPending) return;
    if (!loggedIn) {
      toast.info(t('player.interact.loginRequired'));
      return;
    }
    if (!vid.includes(':')) return;
    send.mutate(
      { vid, text: content, offsetMs: Math.round(currentSec * 1000) },
      {
        onSuccess: () => {
          toast.success(t('player.interact.danmakuSent'));
          setText('');
        },
        onError: (e) => toast.error(String(e)),
      },
    );
  };

  return (
    <div className="relative ml-2 flex h-8 w-44 min-w-0 shrink items-center gap-1 overflow-hidden rounded-full bg-white/15 pr-1 pl-3 backdrop-blur-sm sm:w-52">
      <RichEmojiInput
        ref={richRef}
        value={text}
        onChange={setText}
        onEnter={submit}
        placeholder={t('player.interact.danmakuPlaceholder')}
        maxLength={100}
        className="h-full min-w-0 flex-1 scrollbar-none overflow-x-auto text-xs leading-8 whitespace-pre text-white"
      />
      <EmojiPickerButton
        open={emojiOpen}
        onToggle={() => setEmojiOpen((o) => !o)}
        onPick={pickEmoji}
      />
      <button
        type="button"
        onClick={submit}
        disabled={!text.trim() || send.isPending}
        className="grid h-6 shrink-0 cursor-pointer place-items-center rounded-full bg-red-500 px-2.5 text-xs text-white transition-opacity disabled:opacity-40"
      >
        {t('player.interact.send')}
      </button>
    </div>
  );
}

/**
 * 自绘播放控件。
 *
 * 不用原生 `controls`：它既不跟主题，也放不下「下载到本地 / 清晰度」这类业务动作。
 * 媒体状态（进度、时长、音量、倍速）全部由本组件持有——原生控件撤掉后，
 * 没人再替我们发 `timeupdate`，状态只能自己接。
 *
 * 快进/快退不占按钮位，由快捷键 `←` `→` 承担；播放键旁的步进按钮是
 * **上一集 / 下一集**（与 `↑` `↓` 同一逻辑）。完整选集仍在右侧 `SeriesPanel`，
 * 这里只给最常用的「接着看下一集」一个单击入口。
 */
export function PlayerControls({
  videoRef,
  danmakuOn,
  onToggleDanmaku,
  danmakuDisplay,
  onDanmakuDisplayChange,
  stageRef,
  seriesId,
  episodes,
  currentIndex,
  downloading,
  onDownloadingChange,
  definition,
  definitions,
  onDefinitionChange,
  src,
  onStepEpisode,
  onOpenMini,
  incognito,
  onToggleIncognito,
  immersive,
  onPickEpisode,
  pickerHint,
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
  const [fullscreen, setFullscreen] = useState(false);
  // 面板/浮层开合在全局播放 store：切集/切剧重挂载不丢
  const danmakuPanelOpen = usePlayerStore((s) => s.danmakuPanelOpen);
  const setDanmakuPanelOpen = usePlayerStore((s) => s.setDanmakuPanelOpen);
  const seriesPanelOpen = usePlayerStore((s) => s.seriesPanelOpen);
  const setSeriesPanelOpen = usePlayerStore((s) => s.setSeriesPanelOpen);
  const danmakuPanelRef = useRef<HTMLDivElement | null>(null);

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
    // 只在元素真的处于暂停时置位：新元素还没起播，`paused` 本来就是 true，
    // 无脑调 onPauseEvt 会把「正在播」也刷成暂停。
    if (video.paused) onPauseEvt();

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

  const toggleFullscreen = useCallback(() => {
    const stage = stageRef.current;
    if (!stage) return;
    if (document.fullscreenElement) void document.exitFullscreen();
    else void stage.requestFullscreen().catch(() => undefined);
  }, [stageRef]);

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
    <div
      data-wheel-block
      onMouseEnter={onControlsEnter}
      onMouseLeave={onControlsLeave}
      className={cn(
        // 纯悬浮：不要任何底色/渐变蒙版，就一组裸图标压在画面上。
        // 可读性靠白色 + 投影，不靠底板——底板一加就变成一条色块，破坏了画面。
        'absolute inset-x-0 bottom-0 flex flex-col gap-2 px-4 pt-10 pb-3 text-white',
        'drop-shadow-[0_1px_3px_rgba(0,0,0,0.85)]',
        'transition-opacity duration-200',
        visible ? 'opacity-100' : 'pointer-events-none opacity-0',
      )}
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

        {/* 弹幕发送框（hgplayer 同款位置：控制栏左段时间之后，常驻）。
            w-52 在窄舞台下放不下，允许收缩到 w-32，输入框本身 min-w-0 兜底 */}
        <DanmakuSendBox vid={currentVid ? `${currentVid}:${seriesId}` : ''} currentSec={current} />

        <div className="ml-auto flex items-center gap-1">
          <DropdownMenu>
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

          {/* 清晰度切换。本地已下载的集只有一版，definitions 为空——
              这时按钮照常出现但置灰并说明原因：直接不渲染会让用户以为
              「这个功能本来就没有」，而在线流那一集它又出现了。 */}
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button
                variant="ghost"
                size="sm"
                className={cn(CHROME_BUTTON, 'font-mono')}
                disabled={definitions.length === 0}
                title={definitions.length === 0 ? t('player.definitionLocalOnly') : undefined}
              >
                <MonitorPlay className="size-4" />
                {definition > 0 ? `${definition}P` : t('player.definitionAuto')}
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end" data-wheel-block>
              <DropdownMenuLabel>{t('player.definition')}</DropdownMenuLabel>
              <DropdownMenuItem onSelect={() => onDefinitionChange(undefined)}>
                {t('player.definitionAuto')}
                {definition === 0 && <span className="ml-auto text-xs">✓</span>}
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
                  {definition === d.value && <span className="ml-auto text-xs">✓</span>}
                </DropdownMenuItem>
              ))}
            </DropdownMenuContent>
          </DropdownMenu>

          {immersive && (
            <div className="relative flex items-center">
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
                // 贴着按钮向上弹（弹幕设置面板同款锚定）。宽度必须写死在
                // wrapper 上——% 会相对按钮宽度塌缩，8 列网格直接挤死。
                <div className="absolute right-0 bottom-full mb-3 max-h-[62vh] w-[460px] max-w-[92vw] scrollbar-thin overflow-y-auto">
                  <EpisodePicker
                    seriesId={seriesId}
                    currentIndex={currentIndex}
                    hint={pickerHint}
                    onSelect={(idx) => onPickEpisode?.(idx)}
                    onClose={() => setSeriesPanelOpen(false)}
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

          {/* 小屏播放（对齐 hgplayer）：同一窗口缩成 480×270 落屏幕右下角，
              播放不断——不是系统 PiP（尺寸归系统管），也不是第二个窗口 */}
          <IconButton label={t('player.miniWindow')} onClick={onOpenMini}>
            <PictureInPicture2 className="size-4" />
          </IconButton>

          {/* 弹幕设置：齿轮 + 上方浮层面板 */}
          <div ref={danmakuPanelRef} className="relative flex items-center">
            <IconButton
              label={t('player.danmakuSettings')}
              onClick={() => setDanmakuPanelOpen(!danmakuPanelOpen)}
            >
              <Settings2
                className={`size-4 ${danmakuPanelOpen ? 'text-white' : 'text-white/70'}`}
                aria-hidden
              />
            </IconButton>
            {danmakuPanelOpen && (
              <div className="absolute right-0 bottom-full mb-3 w-60 rounded-xl border border-white/10 bg-black/85 p-4 backdrop-blur-sm">
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
            <MessageSquareText
              className={`size-5 ${danmakuOn ? 'text-white' : 'text-white/40'}`}
              aria-hidden
            />
          </IconButton>

          {/* 音量：hover 弹出竖条浮层（绝对定位不占布局——旧的横向展开
              会把弹幕按钮挤走），浮层盖在按钮上方，移出即收起。
              交互保持逻辑见 VolumePopup（pointer capture 伪 mouseleave 坑）。 */}
          <VolumePopup
            volume={volume}
            muted={muted}
            onToggleMute={toggleMute}
            onSetVolume={setVolumeValue}
          />

          {/* 隐身模式（一只眼睛）：开启后鼠标离开窗口 → 整窗透明 + 暂停，
              鼠标回来即恢复显示。看不该看的东西时的「闪避键」 */}
          <IconButton label={t('player.incognito')} onClick={onToggleIncognito}>
            <Eye className={`size-4 ${incognito ? 'text-white' : 'text-white/55'}`} aria-hidden />
          </IconButton>

          <IconButton label={t('player.fullscreen')} onClick={toggleFullscreen}>
            {fullscreen ? <Minimize className="size-4" /> : <Maximize className="size-4" />}
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
    </div>
  );
}

function IconButton({
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
    <Button
      variant="ghost"
      size="icon"
      className="size-8 bg-transparent text-white hover:bg-white/20 hover:text-white disabled:opacity-40 disabled:hover:bg-transparent"
      onClick={onClick}
      disabled={disabled}
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
      aria-label={t('common.progress')}
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
      {/* 进度条固定白色系，不跟主题走。静止态 2px 半透明（细条贴着画面
          不挡内容），悬停/拖动时涨到 4px 并提亮——B站/YouTube 同款的
          「平时隐身、上手好用」。 */}
      <div className="absolute inset-x-0 top-1/2 h-0.5 -translate-y-1/2 rounded-full bg-white/20 transition-all duration-150 group-hover/bar:h-1" />
      <div
        className="absolute top-1/2 left-0 h-0.5 -translate-y-1/2 rounded-full bg-white/60 transition-all duration-150 group-hover/bar:h-1 group-hover/bar:bg-white/90"
        style={{ width: `${ratio * 100}%` }}
      />
      <div
        className="absolute top-1/2 size-3 -translate-x-1/2 -translate-y-1/2 rounded-full bg-white opacity-0 transition-opacity group-hover/bar:opacity-100"
        style={{ left: `${ratio * 100}%` }}
      />
    </div>
  );
}

/**
 * 竖向滑条（音量浮层用）。div 自绘而不是 `<input type=range>` 转向：
 * 竖向 range 的厂商伪元素在 WebView2 上表现不可控，自绘三段（轨道/已填/
 * 滑块）和 ScrubBar 同一套视觉。
 */
function VerticalSlider({
  value,
  onChange,
  label,
}: {
  value: number;
  onChange: (v: number) => void;
  label: string;
}) {
  const trackRef = useRef<HTMLDivElement>(null);

  const ratioAt = (clientY: number) => {
    const track = trackRef.current;
    if (!track) return 0;
    const rect = track.getBoundingClientRect();
    if (rect.height <= 0) return 0;
    // 竖向：顶部 = 1
    return Math.min(Math.max(1 - (clientY - rect.top) / rect.height, 0), 1);
  };

  return (
    <div
      ref={trackRef}
      role="slider"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={Math.round(value * 100)}
      tabIndex={0}
      className="relative h-24 w-6 cursor-pointer"
      onPointerDown={(e) => {
        e.currentTarget.setPointerCapture(e.pointerId);
        onChange(ratioAt(e.clientY));
      }}
      onPointerMove={(e) => {
        if (e.buttons === 1) onChange(ratioAt(e.clientY));
      }}
      onKeyDown={(e) => {
        if (e.key === 'ArrowUp') onChange(Math.min(value + 0.05, 1));
        if (e.key === 'ArrowDown') onChange(Math.max(value - 0.05, 0));
      }}
    >
      <div className="absolute top-0 bottom-0 left-1/2 w-1 -translate-x-1/2 rounded-full bg-white/25" />
      <div
        className="absolute bottom-0 left-1/2 w-1 -translate-x-1/2 rounded-full bg-white"
        style={{ height: `${value * 100}%` }}
      />
      <div
        className="absolute left-1/2 size-3 -translate-x-1/2 translate-y-1/2 rounded-full bg-white"
        style={{ bottom: `${value * 100}%` }}
      />
    </div>
  );
}

/** 弹幕设置面板的一行：label + 百分比 + 白色横滑条（复用 .volume-range 样式）。 */
function DisplaySlider({
  label,
  value,
  min,
  max,
  onChange,
}: {
  label: string;
  value: number;
  min: number;
  max: number;
  onChange: (v: number) => void;
}) {
  return (
    <label className="flex flex-col gap-1.5">
      <span className="flex items-center justify-between text-xs text-white/90">
        {label}
        <span className="font-mono text-white/70">{Math.round(value * 100)}%</span>
      </span>
      <input
        type="range"
        min={min}
        max={max}
        step={0.05}
        value={value}
        onChange={(e) => onChange(Number(e.target.value))}
        aria-label={label}
        className="volume-range w-full"
      />
    </label>
  );
}

/**
 * 音量按钮 + 竖条浮层。
 *
 * 开合：hover 展开、移出收起——但滑条 `setPointerCapture` 会诱发本层**伪
 * mouseleave**（点击/拖动音量的一瞬浮层被收走，想从 100 连点到 30 必须
 * 反复重开）。对策：浮层内任何 pointerdown 置 hold，mouseleave 见 hold
 * 不收；window pointerup 清 hold 后按指针落点裁决（还在按钮/浮层上就
 * 保持，出去了才收）。
 */
function VolumePopup({
  volume,
  muted,
  onToggleMute,
  onSetVolume,
}: {
  volume: number;
  muted: boolean;
  onToggleMute: () => void;
  onSetVolume: (v: number) => void;
}) {
  const volumeOpen = usePlayerStore((s) => s.volumeOpen);
  const setVolumeOpen = usePlayerStore((s) => s.setVolumeOpen);
  const wrapRef = useRef<HTMLDivElement | null>(null);
  const holdRef = useRef(false);

  useEffect(() => {
    const onUp = (e: PointerEvent) => {
      if (!holdRef.current) return;
      holdRef.current = false;
      const { clientX: x, clientY: y } = e;
      // capture 释放要等事件派发完：推一拍再查落点，elementFromPoint 才准
      setTimeout(() => {
        const hit = document.elementFromPoint(x, y);
        const inside = hit != null && wrapRef.current?.contains(hit);
        if (!inside) setVolumeOpen(false);
      }, 0);
    };
    window.addEventListener('pointerup', onUp);
    return () => window.removeEventListener('pointerup', onUp);
  }, [setVolumeOpen]);

  return (
    <div
      ref={wrapRef}
      className="relative flex items-center"
      onMouseEnter={() => setVolumeOpen(true)}
      onMouseLeave={() => {
        if (holdRef.current) return;
        setVolumeOpen(false);
      }}
    >
      <IconButton label={t('player.mute')} onClick={onToggleMute}>
        {muted || volume === 0 ? <VolumeX className="size-4" /> : <Volume2 className="size-4" />}
      </IconButton>
      {volumeOpen && (
        // 浮层必须与按钮**几何贴合**（无 margin 间隙）：鼠标从按钮移向
        // 浮层的路径一旦离开 wrapper 的后代区域，mouseleave 就会把
        // 浮层整个卸载——间隙就是「想移过去却直接隐藏」的元凶。
        // 视觉留白放进浮层自己的 padding 里。
        <div
          className="absolute bottom-full left-1/2 -translate-x-1/2 rounded-lg bg-black/80 px-3 pt-1 pb-3 backdrop-blur-sm"
          onPointerDownCapture={() => {
            holdRef.current = true;
          }}
        >
          <div className="mb-1 text-center font-mono text-[10px] text-white/90">
            {Math.round((muted ? 0 : volume) * 100)}
          </div>
          <VerticalSlider
            value={muted ? 0 : volume}
            onChange={onSetVolume}
            label={t('player.volume')}
          />
        </div>
      )}
    </div>
  );
}
