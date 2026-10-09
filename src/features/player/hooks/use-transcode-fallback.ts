import { useCallback, useEffect, useRef, useState } from 'react';
import { toast } from 'sonner';
import { useCompatPlayback } from '@/service/queries';
import { useEvent } from '@/service/tauri/events';
import { EVENTS } from '@/service/tauri/types';
import { t, tf } from '@/locales';
import type { CompatProgress, Series } from '@/service/schema';

/**
 * 转码兜底：`videoWidth === 0` 探测 + H.264 转码 + 最终供流地址裁决。
 *
 * `videoWidth === 0` 而声音正常，是「系统解不了这一集编码」的确凿信号——
 * 容器解析得出时长、样本照样缓冲到位，唯独送进解码器的码流缺参数集。
 * 这时把这一集转成 H.264 换一条路走，产物落缓存，同一集只转一次。
 *
 * 状态一律带 `key`（集标识）并在**读时**过滤，而不是在换集时清空：
 * 清空要在 effect 里同步 setState，会触发级联渲染（eslint 会拦）；
 * 带 key 读时过滤则是天生正确的——上一集的产物自动失效，不需要谁去清它。
 *
 * 最终喂给 `<video>` 的地址（playSrc）也在这里收口：它要叠「兜底产物
 * 优先」与「重试防缓存钉」两层裁决，两层的输入分属取流（src/retryTick）
 * 与兜底（compatSrc）两侧，放同一处推导才不会出现两份 playSrc 各算各的。
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
  } = params;
  const compatPlay = useCompatPlayback();

  const [compatResult, setCompatResult] = useState<{ key: string; url: string } | null>(null);
  const [compatProgress, setCompatProgress] = useState<{
    key: string;
    percent: number;
    phase: string;
  } | null>(null);
  /** 同一集只兜底一次：失败后允许重试，成功后不再触发 */
  const compatStarted = useRef(false);

  /** 兜底产物地址，只认当前这一集 */
  const compatSrc = compatResult?.key === episodeKey ? compatResult.url : null;
  /** 兜底进度，只认当前这一集 */
  const compat = compatProgress?.key === episodeKey ? compatProgress : null;
  /** 实际喂给 `<video>` 的地址：有兜底产物就用它 */
  const baseSrc = compatSrc ?? src;
  /**
   * 重试要换 URL：WebView2 可能缓存了失败瞬间的坏响应（自定义协议历史上
   * 没带 no-store），同一个地址重试永远拿坏数据。后端处理器只解析 path，
   * query 是纯缓存钉（后端已补 no-store，这层双保险兜老进程/旧缓存）。
   */
  const playSrc =
    baseSrc && baseSrc.includes('hongguo-stream') && retryTick > 0
      ? `${baseSrc}?r=${retryTick}`
      : baseSrc;
  /** 当前流的指纹：live/stalled 状态读时校验它，换流（切剧/切集/换清晰度）即失效 */
  const streamKey = playSrc ?? '';

  useEvent<CompatProgress>(
    EVENTS.compatPlayProgress,
    useCallback((p: CompatProgress) => {
      setCompatProgress({ key: p.key, percent: p.percent, phase: p.phase });
    }, []),
  );

  const startCompat = useCallback(() => {
    if (!seriesId || !vidIndex || compatStarted.current) return;
    compatStarted.current = true;
    setCompatProgress({ key: episodeKey, percent: 0, phase: 'downloading' });
    const ep = currentSeries?.episodes.find((e) => e.vidIndex === vidIndex);
    compatPlay.mutate(
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
  }, [seriesId, vidIndex, episodeKey, currentSeries, compatPlay, srcKeyRef, setSrcKey]);

  // 解码失败探测：播放在走、画面出不来、且时间确实在推进。
  // 三个条件缺一不可——刚起播那一瞬间 videoWidth 本来就是 0。
  useEffect(() => {
    if (!playSrc || compatSrc || error) return;
    const v = videoRef.current;
    if (!v) return;
    const timer = setInterval(() => {
      if (v.videoWidth === 0 && !v.paused && v.currentTime > 0.3) {
        clearInterval(timer);
        startCompat();
      }
    }, 1200);
    return () => clearInterval(timer);
  }, [playSrc, compatSrc, error, startCompat, videoRef]);

  // 换集就换一把「已兜底过」的记号：产物与进度都靠 key 自己失效，
  // 这里只需允许新的一集再兜底一次。
  useEffect(() => {
    compatStarted.current = false;
  }, [episodeKey]);

  return { compatSrc, compat, playSrc, streamKey };
}
