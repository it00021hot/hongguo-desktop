import { useCallback, useEffect, useRef, useState } from 'react';
import { toast } from 'sonner';
import { useCompatPlayback } from '@/service/queries';
import { useEvent } from '@/service/tauri/events';
import { EVENTS } from '@/service/tauri/types';
import { t, tf } from '@/locales';
import { STREAM_SCHEME } from '@/utils/scheme';
import type { CompatProgress, Series } from '@/service/schema';

/**
 * 转码兜底：videoWidth===0 而声音正常 = 系统解不了编码，转 H.264 换路走（产物落缓存，同一集只转一次）；
 * playSrc 在此收口，叠「兜底产物优先」与「重试防缓存钉」两层裁决。状态带集 key 读时过滤，换集自动失效。
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
    onDecodeFailure,
  } = params;
  const compatPlay = useCompatPlayback();
  /** mutate 引用恒定；整个 mutation 对象每渲染都是新的，进依赖会重置探测计时 */
  const mutateCompat = compatPlay.mutate;

  const [compatResult, setCompatResult] = useState<{ key: string; url: string } | null>(null);
  const [compatProgress, setCompatProgress] = useState<CompatProgress | null>(null);
  /** 同一集只兜底一次：失败后允许重试，成功后不再触发 */
  const compatStarted = useRef(false);

  /** 兜底产物地址，只认当前这一集 */
  const compatSrc = compatResult?.key === episodeKey ? compatResult.url : null;
  /** 兜底进度，只认当前这一集 */
  const compat = compatProgress?.key === episodeKey ? compatProgress : null;
  /** 实际喂给 `<video>` 的地址：有兜底产物就用它 */
  const baseSrc = compatSrc ?? src;
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
    useCallback((p: CompatProgress) => {
      setCompatProgress(p);
    }, []),
  );

  const startCompat = useCallback(() => {
    if (!seriesId || !vidIndex || compatStarted.current) return;
    compatStarted.current = true;
    setCompatProgress({ key: episodeKey, percent: 0, phase: 'downloading' });
    const ep = currentSeries?.episodes.find((e) => e.vidIndex === vidIndex);
    mutateCompat(
      { seriesId, vidIndex, vid: ep?.vid },
      {
        onSuccess: (r) => {
          srcKeyRef.current = episodeKey;
          setSrcKey(episodeKey);
          setCompatResult({ key: episodeKey, url: r.url });
          setCompatProgress(null);
          toast.success(
            r.cached
              ? t('player.compatCached')
              : tf('player.compatDone', {
                  backend: r.backend,
                  seconds: Math.round(r.elapsedMs / 100),
                }),
          );
        },
        onError: (e) => {
          setCompatProgress(null);
          compatStarted.current = false;
          toast.error(tf('player.compatFailed', { reason: e.message }));
        },
      },
    );
    // ref 形参按仓库惯例写进依赖（身份恒定，见 use-playback-progress）
  }, [seriesId, vidIndex, episodeKey, currentSeries, mutateCompat, srcKeyRef, setSrcKey]);

  // 解码失败探测：播放在走、画面出不来、时间在推进，三个条件缺一不可（刚起播 videoWidth 本来就是 0）；
  // 命中走 onDecodeFailure 裁决：无 HEVC 机器先问「装扩展还是转兼容」，其余直接兜底转码
  useEffect(() => {
    if (!playSrc || compatSrc || error) return;
    const v = videoRef.current;
    if (!v) return;
    const timer = setInterval(() => {
      if (v.videoWidth === 0 && !v.paused && v.currentTime > 0.3) {
        clearInterval(timer);
        onDecodeFailure();
      }
    }, 1200);
    return () => clearInterval(timer);
  }, [playSrc, compatSrc, error, onDecodeFailure, videoRef]);

  // 换集换「已兜底过」记号：产物与进度靠 key 自己失效，这里只需允许新一集再兜底一次
  useEffect(() => {
    compatStarted.current = false;
  }, [episodeKey]);

  return { compatSrc, compat, playSrc, streamKey, startCompat };
}
