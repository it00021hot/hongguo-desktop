/** 窄屏官方同款 chrome：右上倍速、底细进度；横片「全屏观看」= 锁横屏 + 藏壳栏。 */
import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type RefObject,
} from 'react';
import { ChevronRight, Download, ListVideo, MonitorPlay, Sparkles } from 'lucide-react';
import { formatDuration } from '@/utils/format';
import { readPlaybackRate, writePlaybackRate } from '@/utils/playback-prefs';
import type { DanmakuDisplaySettings, EnhancementSettings } from '@/utils/playback-prefs';
import type { VideoDefinition } from '@/service/schema';
import { useUiStore } from '@/stores/ui';
import { applyWindowFullscreen } from '@/hooks/use-window-fullscreen';
import { setAndroidLandscape } from '@/utils/android-shell';
import { t, tf } from '@/locales';
import { cn } from '@/lib/utils';
import { Sheet, SheetContent, SheetTitle } from '@/components/ui/sheet';
import { Switch } from '@/components/ui/switch';
import { ChromeSurface } from './chrome-surface';
import { ScrubBar } from './controls/scrub-bar';
import { DisplaySlider } from './controls/sliders';
import { DARK_SHEET_CLOSE } from './phone-series-sheet';
import type { EnhancementStatus } from '../hooks/use-enhancement';

const RATES = [0.75, 1, 1.25, 1.5, 2, 3] as const;
const DEFAULT_RATE: (typeof RATES)[number] = 1;

/** object-contain 内容框相对舞台的像素矩形。 */
function containRect(
  stageW: number,
  stageH: number,
  videoW: number,
  videoH: number,
): { left: number; top: number; width: number; height: number } {
  if (stageW <= 0 || stageH <= 0 || videoW <= 0 || videoH <= 0) {
    return { left: 0, top: 0, width: stageW, height: stageH };
  }
  const videoAspect = videoW / videoH;
  const stageAspect = stageW / stageH;
  if (videoAspect > stageAspect) {
    const height = stageW / videoAspect;
    return { left: 0, top: (stageH - height) / 2, width: stageW, height };
  }
  const width = stageH * videoAspect;
  return { left: (stageW - width) / 2, top: 0, width, height: stageH };
}

function Segmented<T extends string | number>({
  value,
  options,
  labelOf,
  onChange,
  columns,
}: {
  value: T;
  options: readonly T[];
  labelOf: (v: T) => string;
  onChange: (v: T) => void;
  /** 固定列数：避免末项被拉成整行宽（倍速 3x 曾独占第二行） */
  columns?: number;
}) {
  return (
    <div
      className={cn(
        'grid gap-1.5',
        columns ? undefined : 'grid-cols-[repeat(auto-fit,minmax(4.5rem,1fr))]',
      )}
      style={columns ? { gridTemplateColumns: `repeat(${columns}, minmax(0, 1fr))` } : undefined}
    >
      {options.map((opt) => {
        const on = opt === value;
        return (
          <button
            key={String(opt)}
            type="button"
            onClick={() => onChange(opt)}
            className={cn(
              'rounded-lg px-2 py-2 font-mono text-xs transition-colors',
              on ? 'bg-white/20 text-white' : 'bg-white/5 text-white/70 hover:bg-white/10',
            )}
          >
            {labelOf(opt)}
          </button>
        );
      })}
    </div>
  );
}

export function PhonePlayerChrome({
  videoRef,
  visible,
  moreOpen,
  onMoreOpenChange,
  enhanceOpen,
  onEnhanceOpenChange,
  onOpenSeries,
  episodeCount,
  definition,
  definitions,
  onDefinitionChange,
  danmakuOn,
  onToggleDanmaku,
  danmakuDisplay,
  onDanmakuDisplayChange,
  enhanceSettings,
  onEnhanceChange,
  enhanceStatus,
  enhanceAvailable,
  onDownloadingChange,
  onControlsEnter,
  onControlsLeave,
}: {
  videoRef: RefObject<HTMLVideoElement | null>;
  visible: boolean;
  moreOpen: boolean;
  onMoreOpenChange: (open: boolean) => void;
  enhanceOpen: boolean;
  onEnhanceOpenChange: (open: boolean) => void;
  onOpenSeries: () => void;
  episodeCount: number;
  definition: number;
  definitions: VideoDefinition[];
  onDefinitionChange: (definition: number | undefined) => void;
  danmakuOn: boolean;
  onToggleDanmaku: () => void;
  danmakuDisplay: DanmakuDisplaySettings;
  onDanmakuDisplayChange: (patch: Partial<DanmakuDisplaySettings>) => void;
  enhanceSettings: EnhancementSettings;
  onEnhanceChange: (patch: Partial<EnhancementSettings>) => void;
  enhanceStatus: EnhancementStatus;
  enhanceAvailable: boolean;
  onDownloadingChange: (open: boolean) => void;
  onControlsEnter: () => void;
  onControlsLeave: () => void;
}) {
  const stageRef = useRef<HTMLDivElement>(null);
  const [danmakuPanel, setDanmakuPanel] = useState(false);
  const [rate, setRate] = useState(() => readPlaybackRate());
  const [current, setCurrent] = useState(0);
  const [duration, setDuration] = useState(0);
  const [landscape, setLandscape] = useState(false);
  const [fsTop, setFsTop] = useState<number | null>(null);
  // Android 上窗口 setFullscreen 基本空操作；窄屏「全屏观看」走 UI 沉浸（playerFrameless 藏顶栏/底栏）
  const immersive = useUiStore((s) => s.playerFrameless);
  const setImmersive = useUiStore((s) => s.setPlayerFrameless);

  const syncFullscreenAnchor = useCallback(() => {
    const video = videoRef.current;
    const stage = stageRef.current ?? video?.parentElement;
    if (!video || !stage || video.videoWidth <= 0 || video.videoHeight <= 0) {
      setFsTop(null);
      return;
    }
    const rect = stage.getBoundingClientRect();
    const box = containRect(rect.width, rect.height, video.videoWidth, video.videoHeight);
    // 落在 letterbox 黑边里（画面底边下方），不压在画面上
    setFsTop(box.top + box.height + 10);
  }, [videoRef]);

  useEffect(() => {
    const video = videoRef.current;
    if (!video) return;
    setRate(readPlaybackRate());
    setCurrent(video.currentTime);
    setDuration(Number.isFinite(video.duration) ? video.duration : 0);
    const syncLandscape = () => {
      setLandscape(video.videoWidth > 0 && video.videoWidth > video.videoHeight);
      syncFullscreenAnchor();
    };
    syncLandscape();
    const onTime = () => {
      setCurrent(video.currentTime);
      setDuration(Number.isFinite(video.duration) ? video.duration : 0);
    };
    const onRate = () => setRate(readPlaybackRate());
    video.addEventListener('timeupdate', onTime);
    video.addEventListener('durationchange', onTime);
    video.addEventListener('ratechange', onRate);
    video.addEventListener('loadedmetadata', syncLandscape);
    video.addEventListener('loadeddata', syncLandscape);
    return () => {
      video.removeEventListener('timeupdate', onTime);
      video.removeEventListener('durationchange', onTime);
      video.removeEventListener('ratechange', onRate);
      video.removeEventListener('loadedmetadata', syncLandscape);
      video.removeEventListener('loadeddata', syncLandscape);
    };
  }, [videoRef, syncFullscreenAnchor]);

  useLayoutEffect(() => {
    syncFullscreenAnchor();
    const stage = stageRef.current;
    if (!stage || typeof ResizeObserver === 'undefined') return;
    const ro = new ResizeObserver(() => syncFullscreenAnchor());
    ro.observe(stage);
    return () => ro.disconnect();
  }, [landscape, immersive, syncFullscreenAnchor]);

  const applyRate = (next: number) => {
    // 先写 prefs 镜像，再贴到元素——贴回 effect 与 UI 都以镜像为准，
    // 避免 Android WebView 不派发 ratechange 时被旧值打回
    writePlaybackRate(next);
    setRate(next);
    const video = videoRef.current;
    if (video) video.playbackRate = next;
  };

  const seekTo = (ratio: number) => {
    const video = videoRef.current;
    if (!video || duration <= 0) return;
    const next = Math.min(Math.max(ratio, 0), 1) * duration;
    video.currentTime = next;
    setCurrent(next);
  };

  const toggleImmersive = () => {
    const next = !useUiStore.getState().playerFrameless;
    setImmersive(next);
    // 真机：锁 Activity 横屏（完整画面铺满横屏）；桌面窄窗顺带试窗口全屏
    setAndroidLandscape(next);
    void applyWindowFullscreen(next).catch(() => undefined);
  };

  const enhanceOn = enhanceSettings.anime4kOn || enhanceSettings.sharpenOn;

  return (
    <>
      {/* 量舞台尺寸：与 video 同级铺满的空层，不挡指针 */}
      <div ref={stageRef} className="pointer-events-none absolute inset-0 z-0" aria-hidden />

      <ChromeSurface
        shown={visible}
        data-wheel-block
        onClick={(e) => e.stopPropagation()}
        onMouseEnter={onControlsEnter}
        onMouseLeave={onControlsLeave}
        className="absolute top-3 right-3 z-30 text-white drop-shadow-[0_1px_3px_rgba(0,0,0,0.85)]"
      >
        <button
          type="button"
          aria-label={t('player.playbackRate')}
          onClick={() => onMoreOpenChange(true)}
          className="rounded-full bg-black/35 px-2.5 py-1 font-mono text-xs backdrop-blur-sm hover:bg-black/50"
        >
          {rate}x
        </button>
      </ChromeSurface>

      {/* 横片：落在 letterbox 黑边；点了锁 Activity 横屏 + 藏壳栏（完整画面铺满） */}
      {landscape && fsTop != null && (
        <ChromeSurface
          shown={visible}
          data-wheel-block
          className="pointer-events-none absolute inset-x-0 z-20"
          style={{ top: fsTop }}
        >
          <div className="flex justify-center">
            <button
              type="button"
              onClick={(e) => {
                e.stopPropagation();
                toggleImmersive();
              }}
              className="pointer-events-auto flex items-center gap-1.5 rounded-full bg-black/55 px-3.5 py-2 text-sm text-white backdrop-blur-sm hover:bg-black/70"
            >
              <MonitorPlay className="size-4" />
              {immersive ? t('player.exitWatchFullscreen') : t('player.watchFullscreen')}
            </button>
          </div>
        </ChromeSurface>
      )}

      {/* 底：仅细进度（增强/选集/下载进倍速面板） */}
      <ChromeSurface
        shown={visible}
        data-wheel-block
        onClick={(e) => e.stopPropagation()}
        onMouseEnter={onControlsEnter}
        onMouseLeave={onControlsLeave}
        className="absolute inset-x-0 bottom-0 z-30 px-3 pt-4 pb-1.5 text-white"
      >
        <div className="mb-0.5 flex justify-between font-mono text-[10px] text-white/70 tabular-nums">
          <span>{formatDuration(current)}</span>
          <span>{formatDuration(duration)}</span>
        </div>
        <ScrubBar
          current={Math.min(current, duration || 0)}
          duration={duration}
          onSeek={seekTo}
          onScrubStart={() => undefined}
          onScrubEnd={() => undefined}
        />
      </ChromeSurface>

      {/* 倍速 / 清晰度 / 弹幕 */}
      <Sheet
        open={moreOpen}
        onOpenChange={(open) => {
          onMoreOpenChange(open);
          if (!open) setDanmakuPanel(false);
        }}
      >
        <SheetContent
          side="bottom"
          closeClassName={DARK_SHEET_CLOSE}
          className="max-h-[78vh] gap-0 overflow-y-auto rounded-t-2xl border-white/10 bg-neutral-950 p-0 pt-2 text-white"
          onClick={(e) => e.stopPropagation()}
        >
          <SheetTitle className="sr-only">{t('player.playbackRate')}</SheetTitle>
          <div className="flex flex-col gap-5 px-4 pt-2 pr-12 pb-4">
            <section className="flex flex-col gap-2">
              <div className="flex items-center gap-2 text-sm text-white/85">
                <span>{t('player.playbackRate')}</span>
                <span className="ml-auto font-mono text-xs text-white/50">{rate}x</span>
              </div>
              <Segmented
                value={RATES.find((r) => Math.abs(r - rate) < 0.01) ?? DEFAULT_RATE}
                options={RATES}
                labelOf={(r) => `${r}x`}
                onChange={applyRate}
                columns={3}
              />
            </section>
            <section className="flex flex-col gap-2">
              <div className="text-sm text-white/85">{t('player.definition')}</div>
              {definitions.length === 0 ? (
                <p className="text-xs text-white/45">{t('player.definitionLocalOnly')}</p>
              ) : (
                <Segmented
                  value={definition > 0 ? definition : 0}
                  options={[0, ...definitions.map((d) => d.value)] as number[]}
                  labelOf={(v) => (v === 0 ? t('player.definitionAuto') : `${v}P`)}
                  onChange={(v) => onDefinitionChange(v === 0 ? undefined : v)}
                />
              )}
            </section>
            <section className="flex flex-col divide-y divide-white/10 overflow-hidden rounded-xl bg-white/5">
              <div className="flex h-11 items-center gap-3 px-3">
                <span className="flex-1 text-sm leading-none">{t('player.danmaku')}</span>
                <Switch
                  checked={danmakuOn}
                  onCheckedChange={() => onToggleDanmaku()}
                  className="shrink-0"
                />
              </div>
              <button
                type="button"
                className="flex w-full items-center gap-3 px-3 py-3 text-left text-sm hover:bg-white/5"
                onClick={() => setDanmakuPanel((v) => !v)}
              >
                <span className="flex-1">{t('player.danmakuSettings')}</span>
                <ChevronRight
                  className={cn(
                    'size-4 text-white/40 transition-transform',
                    danmakuPanel && 'rotate-90',
                  )}
                />
              </button>
              {danmakuPanel && (
                <div className="flex flex-col gap-3 px-3 py-3">
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
              )}
              <button
                type="button"
                disabled={!enhanceAvailable}
                className="flex w-full items-center gap-3 px-3 py-3 text-left text-sm hover:bg-white/5 disabled:opacity-40"
                onClick={() => {
                  onMoreOpenChange(false);
                  onEnhanceOpenChange(true);
                }}
              >
                <Sparkles className={cn('size-4', enhanceOn ? 'text-white' : 'text-white/70')} />
                <span className="flex-1">{t('player.enhanceSettings')}</span>
                <ChevronRight className="size-4 text-white/40" />
              </button>
              <button
                type="button"
                className="flex w-full items-center gap-3 px-3 py-3 text-left text-sm hover:bg-white/5"
                onClick={() => {
                  onMoreOpenChange(false);
                  onOpenSeries();
                }}
              >
                <ListVideo className="size-4 text-white/70" />
                <span className="flex-1 truncate">
                  {episodeCount > 0
                    ? tf('player.episodesBar', { count: episodeCount })
                    : t('player.episodes')}
                </span>
                <ChevronRight className="size-4 text-white/40" />
              </button>
              <button
                type="button"
                className="flex w-full items-center gap-3 px-3 py-3 text-left text-sm hover:bg-white/5"
                onClick={() => {
                  onMoreOpenChange(false);
                  onDownloadingChange(true);
                }}
              >
                <Download className="size-4 text-white/70" />
                <span className="flex-1">{t('player.downloadToLocal')}</span>
                <ChevronRight className="size-4 text-white/40" />
              </button>
            </section>
          </div>
        </SheetContent>
      </Sheet>

      {/* 画质增强简版 */}
      <Sheet open={enhanceOpen} onOpenChange={onEnhanceOpenChange}>
        <SheetContent
          side="bottom"
          closeClassName={DARK_SHEET_CLOSE}
          className="gap-0 rounded-t-2xl border-white/10 bg-neutral-950 p-0 pt-2 text-white"
          onClick={(e) => e.stopPropagation()}
        >
          <SheetTitle className="sr-only">{t('player.enhanceSettings')}</SheetTitle>
          <div className="flex flex-col gap-4 px-4 pt-2 pr-12 pb-5">
            <div className="text-sm text-white/85">{t('player.enhanceSettings')}</div>
            <div className="flex gap-1">
              <button
                type="button"
                aria-pressed={enhanceSettings.anime4kOn}
                onClick={() => onEnhanceChange({ anime4kOn: !enhanceSettings.anime4kOn })}
                className={cn(
                  'flex-1 rounded-lg px-2 py-2 text-xs transition-colors',
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
                  'flex-1 rounded-lg px-2 py-2 text-xs transition-colors',
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
            <p className="text-[11px] leading-relaxed text-white/55">{t('player.enhanceNote')}</p>
            {enhanceSettings.anime4kOn && (
              <p className="text-[11px] leading-relaxed text-white/55">
                {t('player.enhanceAnime4kNote')}
              </p>
            )}
            {enhanceStatus.message && (
              <p
                role="status"
                className={cn(
                  'text-[11px] leading-relaxed',
                  enhanceStatus.state === 'error' ? 'text-white' : 'text-white/55',
                )}
              >
                {enhanceStatus.message}
              </p>
            )}
          </div>
        </SheetContent>
      </Sheet>
    </>
  );
}
