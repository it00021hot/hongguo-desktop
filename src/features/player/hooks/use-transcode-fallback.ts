import { useCallback, useEffect, useRef, useState } from 'react';
import { compatPlaybackKey, wholeFileSrc } from './growing-fallback';
import { toast } from 'sonner';
import { useCompatPlayback } from '@/service/queries';
import { useEvent } from '@/service/tauri/events';
import { EVENTS } from '@/service/tauri/types';
import { t, tf } from '@/locales';
import { STREAM_SCHEME } from '@/utils/scheme';
import type { CompatProgress, Series } from '@/service/schema';
import { hevcSession } from './hevc-session';
import { decideCompatPlayback, resolvePlayBaseSrc } from './compat-policy';

/**
 * 转码兜底：videoWidth===0 而声音正常 = 系统解不了编码，转 H.264 换路走（产物落缓存，同一集只转一次）；
 * Android / 已授权会话在能力判假后主动转兼容，不先喂 HEVC。
 * playSrc 在此收口，叠「兜底产物优先」「主动兼容挡原生」「重试防缓存钉」三层裁决。
 */
export function useTranscodeFallback(params: {
  videoRef: React.RefObject<HTMLVideoElement | null>;
  /** store 里的目标未恢复前是 null——startCompat 自带空值护栏 */
  seriesId: string | null;
  vidIndex: number | null;
  episodeKey: string;
  error: string | null;
  src: string | null;
  retryTick: number;
  srcKeyRef: React.RefObject<string>;
  setSrcKey: (key: string) => void;
  currentSeries?: Series;
  /** 后端 HEVC 可播结论；`undefined` = 探测未返回 */
  hevcPlayable: boolean | undefined;
  /** Android：无扩展安装路径，软解判假即强制兼容 */
  android: boolean;
  /** 后端标明本集必须转兼容（bytevc2 等） */
  needsCompat?: boolean;
  definition?: number;
  sourceReady: boolean;
  /**
   * 解码失败的裁决入口（轮询命中后调用）：父层据此决定直接兜底转码，
   * 还是先弹「装扩展 vs 转兼容」的主卡（无 HEVC 机器）。
   */
  onDecodeFailure: () => void;
}) {
  const {
    videoRef,
    seriesId,
    vidIndex,
    episodeKey,
    error,
    src,
    retryTick,
    srcKeyRef,
    setSrcKey,
    currentSeries,
    hevcPlayable,
    android,
    needsCompat,
    definition,
    sourceReady,
    onDecodeFailure,
  } = params;
  const compatPlay = useCompatPlayback();
  /** mutate 引用恒定；整个 mutation 对象每渲染都是新的，进依赖会重置探测计时 */
  const mutateCompat = compatPlay.mutate;

  const [compatResult, setCompatResult] = useState<{
    key: string;
    url: string;
    progressive: boolean;
  } | null>(null);
  const [compatProgress, setCompatProgress] = useState<CompatProgress | null>(null);
  const [waitingFor, setWaitingFor] = useState<string | null>(null);
  const compatKey = sourceReady ? compatPlaybackKey(episodeKey, definition) : '';
  const waitingWholeFile = waitingFor === compatKey;
  const activeCompatKey = useRef(compatKey);
  const compatStarted = useRef(false);

  const { forceCompat, holdNative } = decideCompatPlayback({
    hevcPlayable,
    android,
    compatAuthorized: hevcSession.compatAuthorized(),
    needsCompat: sourceReady && needsCompat,
  });

  const compatSrc = compatResult?.key === compatKey ? compatResult.url : null;
  const progressive = compatResult?.key === compatKey && compatResult.progressive;
  const compat = compatProgress?.key === compatKey ? compatProgress : null;
  const producer =
    compat?.phase === 'failed'
      ? ('failed' as const)
      : compat?.phase === 'completed'
        ? ('completed' as const)
        : progressive
          ? ('running' as const)
          : ('completed' as const);
  // Completed 事件本身也算「可以接整片」：不能先清 waiting 再指望 wholeFileSrc。
  const waitingForWhole = waitingWholeFile || compat?.phase === 'completed';
  const wholeSrc = wholeFileSrc({
    episodeKey: compatKey,
    eventKey: compat?.key ?? compatResult?.key ?? null,
    producer,
    completeUrl: compatSrc,
    waiting: waitingForWhole,
  });
  /** 实际喂给 `<video>` 的地址：有兜底产物就用它；主动兼容途中不喂原生 HEVC */
  const baseSrc = resolvePlayBaseSrc(src, compatSrc, holdNative);
  /**
   * 重试要换 URL：WebView2 可能缓存了失败瞬间的坏响应，同一个地址重试永远拿坏数据；
   * 后端只解析 path，query 是纯缓存钉（后端已补 no-store，这层双保险兜旧缓存）。
   */
  const playSrc =
    baseSrc && baseSrc.includes(STREAM_SCHEME) && retryTick > 0
      ? `${baseSrc}?r=${retryTick}`
      : baseSrc;
  /** 当前流的指纹：live/stalled 状态读时校验它，换流（切剧/切集/换清晰度）即失效 */
  const streamKey = playSrc ?? '';

  useEvent(
    EVENTS.compatPlayProgress,
    useCallback(
      (p: CompatProgress) => {
        if (p.key !== compatKey) return;
        if (p.phase === 'completed') {
          setCompatResult((prev) =>
            prev && prev.key === compatKey ? { ...prev, progressive: false } : prev,
          );
          // 保留 completed 相位，让 wholeFileSrc 在 MSE 回退等待后拿到整片地址
          setCompatProgress({ key: compatKey, percent: 100, phase: 'completed' });
          return;
        }
        if (p.phase === 'failed') {
          setCompatResult((prev) => (prev && prev.key === compatKey ? null : prev));
          setWaitingFor(null);
          setCompatProgress(null);
          return;
        }
        if (p.phase === 'ready' && p.percent >= 100) return;
        setCompatProgress(p);
      },
      [compatKey],
    ),
  );

  const startCompat = useCallback(() => {
    if (!seriesId || !vidIndex || !compatKey || compatStarted.current) return;
    compatStarted.current = true;
    setCompatProgress({ key: compatKey, percent: 0, phase: 'downloading' });
    const ep = currentSeries?.episodes.find((e) => e.vidIndex === vidIndex);
    mutateCompat(
      { seriesId, vidIndex, vid: ep?.vid, definition },
      {
        onSuccess: (r) => {
          if (activeCompatKey.current !== compatKey) return;
          srcKeyRef.current = episodeKey;
          setSrcKey(episodeKey);
          setCompatResult({ key: compatKey, url: r.url, progressive: r.progressive });
          if (r.progressive) return;
          setCompatProgress(null);
          // 主动兼容 + 缓存命中是预期路径（软解机连播），不再每集弹 toast 打扰
          if (r.cached) {
            if (!forceCompat) toast.success(t('player.compatCached'));
          } else {
            toast.success(
              tf('player.compatDone', {
                backend: r.backend,
                seconds: Math.round(r.elapsedMs / 1000),
              }),
            );
          }
        },
        onError: (e) => {
          if (activeCompatKey.current !== compatKey) return;
          setCompatProgress(null);
          compatStarted.current = false;
          toast.error(tf('player.compatFailed', { reason: e.message }));
        },
      },
    );
    // ref 形参按仓库惯例写进依赖（身份恒定，见 use-playback-progress）
  }, [
    seriesId,
    vidIndex,
    episodeKey,
    compatKey,
    definition,
    currentSeries,
    mutateCompat,
    srcKeyRef,
    setSrcKey,
    forceCompat,
  ]);

  // 换集或档位后先更新任务身份，再由下一段 effect 启动兼容。
  useEffect(() => {
    activeCompatKey.current = compatKey;
    compatStarted.current = false;
  }, [compatKey]);

  const noteGrowingUnavailable = useCallback(() => {
    setWaitingFor(compatKey);
  }, [compatKey]);

  // 已知不可解：起播前主动转兼容（Android 软解 / 本会话已选转兼容），省掉首集黑屏等待
  useEffect(() => {
    if (!forceCompat || !seriesId || !vidIndex) return;
    if (android) hevcSession.authorizeCompat();
    startCompat();
  }, [forceCompat, seriesId, vidIndex, episodeKey, android, startCompat]);

  // 解码失败探测：播放在走、画面出不来、时间在推进，三个条件缺一不可（刚起播 videoWidth 本来就是 0）；
  // 命中走 onDecodeFailure 裁决：无 HEVC 机器先问「装扩展还是转兼容」，其余直接兜底转码
  useEffect(() => {
    if (!playSrc || compatSrc || error || forceCompat) return;
    const v = videoRef.current;
    if (!v) return;
    const timer = setInterval(() => {
      if (v.videoWidth === 0 && !v.paused && v.currentTime > 0.3) {
        clearInterval(timer);
        onDecodeFailure();
      }
    }, 1200);
    return () => clearInterval(timer);
  }, [playSrc, compatSrc, error, forceCompat, onDecodeFailure, videoRef]);

  const showCompleteSrc = wholeSrc != null || (!progressive && !waitingWholeFile);
  return {
    compatSrc,
    compat,
    playSrc,
    streamKey,
    startCompat,
    forceCompat,
    progressive,
    showCompleteSrc,
    noteGrowingUnavailable,
  };
}
