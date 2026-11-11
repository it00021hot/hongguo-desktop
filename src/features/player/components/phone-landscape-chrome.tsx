/** 窄屏横屏全屏播映壳（对齐官方红果）：顶返回+标题，底进度 + 弹开关 + 内嵌发弹幕 + 倍速/清晰度/选集。 */
import { useEffect, useState, type RefObject } from 'react';
import { ChevronLeft } from 'lucide-react';
import { toast } from 'sonner';
import { formatDuration } from '@/utils/format';
import { readPlaybackRate, writePlaybackRate } from '@/utils/playback-prefs';
import type { VideoDefinition } from '@/service/schema';
import { useAccount, useSendDanmaku } from '@/service/queries';
import { t, tf } from '@/locales';
import { cn } from '@/lib/utils';
import { ChromeSurface } from './chrome-surface';
import { ScrubBar } from './controls/scrub-bar';
import { DanmakuSwitchIcon } from './controls/player-icons';
import { EmojiSendBox } from '@/components/common/emoji/emoji-send-box';
import { useImeBottom } from '@/utils/ime-inset';

const RATES = [0.75, 1, 1.25, 1.5, 2, 3] as const;

export function PhoneLandscapeChrome({
  videoRef,
  visible,
  title,
  vidIndex,
  episodeCount,
  definition,
  automaticDefinition,
  definitions,
  onDefinitionChange,
  danmakuOn,
  onToggleDanmaku,
  onOpenMore,
  onOpenSeries,
  onExit,
  vid,
  onControlsEnter,
  onControlsLeave,
}: {
  videoRef: RefObject<HTMLVideoElement | null>;
  visible: boolean;
  title?: string;
  vidIndex: number;
  episodeCount: number;
  definition: number;
  automaticDefinition: boolean;
  definitions: VideoDefinition[];
  onDefinitionChange: (definition: number | undefined) => void;
  danmakuOn: boolean;
  onToggleDanmaku: () => void;
  onOpenMore: () => void;
  onOpenSeries: () => void;
  onExit: () => void;
  /** 「vid:seriesId」组合形态 */
  vid: string;
  onControlsEnter: () => void;
  onControlsLeave: () => void;
}) {
  const [rate, setRate] = useState(() => readPlaybackRate());
  const [current, setCurrent] = useState(0);
  const [duration, setDuration] = useState(0);
  const [text, setText] = useState('');
  const [inputFocused, setInputFocused] = useState(false);
  const imeBottom = useImeBottom();
  const send = useSendDanmaku();
  const { data: account } = useAccount();
  const loggedIn = !!account;

  useEffect(() => {
    const video = videoRef.current;
    if (!video) return;
    setRate(readPlaybackRate());
    setCurrent(video.currentTime);
    setDuration(Number.isFinite(video.duration) ? video.duration : 0);
    const onTime = () => {
      setCurrent(video.currentTime);
      setDuration(Number.isFinite(video.duration) ? video.duration : 0);
    };
    const onRate = () => setRate(readPlaybackRate());
    video.addEventListener('timeupdate', onTime);
    video.addEventListener('durationchange', onTime);
    video.addEventListener('ratechange', onRate);
    return () => {
      video.removeEventListener('timeupdate', onTime);
      video.removeEventListener('durationchange', onTime);
      video.removeEventListener('ratechange', onRate);
    };
  }, [videoRef]);

  const seekTo = (ratio: number) => {
    const video = videoRef.current;
    if (!video || duration <= 0) return;
    const next = Math.min(Math.max(ratio, 0), 1) * duration;
    video.currentTime = next;
    setCurrent(next);
  };

  const cycleRate = () => {
    const idx = RATES.findIndex((r) => Math.abs(r - rate) < 0.01);
    const next = RATES[(idx + 1) % RATES.length] ?? 1;
    writePlaybackRate(next);
    setRate(next);
    const video = videoRef.current;
    if (video) video.playbackRate = next;
  };

  const cycleDefinition = () => {
    if (definitions.length === 0) return;
    const values = [0, ...definitions.map((d) => d.value)];
    const cur = automaticDefinition ? 0 : definition;
    const idx = values.indexOf(cur);
    const next = values[(idx + 1) % values.length] ?? 0;
    onDefinitionChange(next === 0 ? undefined : next);
  };

  const definitionLabel =
    automaticDefinition || definition === 0 ? t('player.definitionAuto') : `${definition}P`;

  const submit = () => {
    const content = text.trim();
    if (!content || send.isPending) return;
    if (!loggedIn) {
      toast.info(t('player.interact.loginRequired'));
      return;
    }
    if (!vid.includes(':')) return;
    const offsetMs = Math.round((videoRef.current?.currentTime ?? 0) * 1000);
    send.mutate(
      { vid, text: content, offsetMs },
      {
        onSuccess: () => {
          toast.success(t('player.interact.danmakuSent'));
          setText('');
        },
      },
    );
  };

  // 输入聚焦 / 键盘升起时强制亮 chrome，避免 3s 自动收起把输入框带走
  const shown = visible || inputFocused || imeBottom > 0;

  return (
    <>
      <ChromeSurface
        shown={shown}
        data-wheel-block
        onClick={(e) => e.stopPropagation()}
        onMouseEnter={onControlsEnter}
        onMouseLeave={onControlsLeave}
        className="absolute inset-x-0 top-0 z-30 flex items-center gap-2 bg-gradient-to-b from-black/55 to-transparent px-3 pt-2 pb-6 text-white"
      >
        <button
          type="button"
          aria-label={t('player.exitWatchFullscreen')}
          onClick={(e) => {
            e.stopPropagation();
            onExit();
          }}
          className="-ml-1 grid size-11 shrink-0 place-items-center rounded-full hover:bg-white/10"
        >
          <ChevronLeft className="size-7" />
        </button>
        <div className="min-w-0 flex-1 truncate text-sm font-medium drop-shadow-md">
          {title ?? ''}
          {vidIndex > 0 && (
            <span className="text-white/75"> {tf('player.episodeNo', { index: vidIndex })}</span>
          )}
        </div>
      </ChromeSurface>

      {/* 键盘升起时改 fixed，贴在 IME 上方（absolute 会被 100vh/舞台裁切盖住） */}
      <ChromeSurface
        shown={shown}
        data-wheel-block
        onClick={(e) => e.stopPropagation()}
        onMouseEnter={onControlsEnter}
        onMouseLeave={onControlsLeave}
        className={
          imeBottom > 0
            ? 'fixed inset-x-0 z-50 bg-gradient-to-t from-black/70 to-transparent px-3 pt-8 pb-2.5 text-white'
            : 'absolute inset-x-0 bottom-0 z-30 bg-gradient-to-t from-black/70 to-transparent px-3 pt-8 pb-2.5 text-white'
        }
        style={imeBottom > 0 ? { bottom: imeBottom } : undefined}
      >
        <div className="mb-1 flex justify-between font-mono text-[10px] text-white/70 tabular-nums">
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
        <div className="mt-2 flex items-center gap-2">
          <button
            type="button"
            aria-label={t('player.danmaku')}
            aria-pressed={danmakuOn}
            onClick={(e) => {
              e.stopPropagation();
              onToggleDanmaku();
            }}
            className={cn(
              'grid size-8 shrink-0 place-items-center rounded-md',
              danmakuOn ? 'bg-white/20 text-white' : 'bg-white/10 text-white/45',
            )}
          >
            <DanmakuSwitchIcon off={!danmakuOn} />
          </button>
          <div
            className="min-w-0 flex-1"
            onFocusCapture={() => setInputFocused(true)}
            onBlurCapture={(e) => {
              const next = e.relatedTarget;
              if (!(next instanceof Node) || !e.currentTarget.contains(next)) {
                setInputFocused(false);
              }
            }}
          >
            <EmojiSendBox
              value={text}
              onChange={setText}
              onSubmit={submit}
              placeholder={t('player.interact.danmakuPlaceholder')}
              maxLength={100}
              pending={send.isPending}
              pickerAlign="left"
              className="h-9 w-full gap-1 rounded-full bg-white/10 pr-1 pl-2.5"
              inputClassName="h-full min-w-0 flex-1 scrollbar-none overflow-x-auto text-xs leading-9 whitespace-pre text-white placeholder:text-white/45"
              sendClassName="h-7 rounded-full px-2.5 text-[11px]"
            />
          </div>
          <button
            type="button"
            aria-label={t('player.playbackRate')}
            onClick={(e) => {
              e.stopPropagation();
              cycleRate();
            }}
            onContextMenu={(e) => {
              e.preventDefault();
              onOpenMore();
            }}
            className="shrink-0 rounded-md px-1.5 py-1 font-mono text-xs text-white/90 hover:bg-white/10"
          >
            {rate}x
          </button>
          <button
            type="button"
            aria-label={t('player.definition')}
            onClick={(e) => {
              e.stopPropagation();
              cycleDefinition();
            }}
            onContextMenu={(e) => {
              e.preventDefault();
              onOpenMore();
            }}
            className="shrink-0 rounded-md px-1.5 py-1 text-xs text-white/90 hover:bg-white/10"
          >
            {definitionLabel}
          </button>
          <button
            type="button"
            aria-label={t('player.episodes')}
            onClick={(e) => {
              e.stopPropagation();
              onOpenSeries();
            }}
            className="shrink-0 rounded-md px-1.5 py-1 text-xs text-white/90 hover:bg-white/10"
          >
            {episodeCount > 0
              ? tf('player.episodesBar', { count: episodeCount })
              : t('player.episodes')}
          </button>
        </div>
      </ChromeSurface>
    </>
  );
}
