import { useCallback, useEffect, useRef, useState } from 'react';
import { toast } from 'sonner';
import { usePlay } from '@/service/queries';
import { usePlayerStore } from '@/stores/player';
import { useEvent } from '@/service/tauri/events';
import { EVENTS } from '@/service/tauri/types';
import { t } from '@/locales';
import {
  readMuted,
  readPlaybackRate,
  readVolume,
  writeMuted,
  writePlaybackRate,
  writeVolume,
} from '@/utils/playback-prefs';
import type { OnlineProgress, VideoDefinition } from '@/service/schema';

/** 取流失败自动重试的节奏：失败页必须持续自动恢复，永不退回死报错。前 4 拍快节奏扛瞬时抖动，之后 30s 一拍长期陪跑。 */
const AUTO_RETRY_FAST_ATTEMPTS = 4;
const AUTO_RETRY_FAST_DELAYS_MS = [2_000, 5_000, 10_000, 15_000];
const AUTO_RETRY_SETTLED_DELAY_MS = 30_000;

/**
 * 取流与起播：play mutation、清晰度切换、断供自动重试、取流进度订阅、媒体偏好贴回。
 * 三把竞态防御 ref 真值在这里持有供相邻 hook 读写：srcKeyRef（流属于哪一集）/ lastKnownRef（带集 key 的最近位置）/ pendingSeek。
 */
export function usePlaybackSource(params: {
  videoRef: React.RefObject<HTMLVideoElement | null>;
  /** store 里的目标未恢复前是 null——起播 effect 自带空值护栏 */
  seriesId: string | null;
  vidIndex: number | null;
  episodeKey: string;
}) {
  const { videoRef, seriesId, vidIndex, episodeKey } = params;
  const pendingSeek = useRef(0);
  /** 最近一次播放位置与时长（卸载后元素读不到 currentTime，这份是 cleanup 里唯一真值）；key 记属于哪一集，防换集拿上一集秒数续播。 */
  const lastKnownRef = useRef({ key: '', time: 0, duration: 0 });
  /** 现在这条流属于哪一集——不带这道闸，信息流切剧时旧画面的秒数会记到新剧头上。 */
  const srcKeyRef = useRef('');

  // 切集 / 加载中都会让 <video> 被卸载重建，新元素的倍速音量静音一律回默认值，「用户设的值」存这里每次渲染后贴回
  const playbackRateRef = useRef(readPlaybackRate());
  const volumeRef = useRef(readVolume());
  const mutedRef = useRef(readMuted());

  const resumeHint = usePlayerStore((s) => s.resumeHint);
  const clearResumeHint = usePlayerStore((s) => s.setResumeHint);
  const [src, setSrc] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  /**
   * srcKeyRef 的 state 镜像（同点写入）：渲染期据此判定「切换在途」——
   * 目标已变、新流未到时新剧的封面+加载胶囊立即接管，否则用户看到「切了没反应」。
   */
  const [srcKey, setSrcKey] = useState('');
  /**
   * 在线流断供的自动重试（带集指纹，读时校验，换集自动失效）。数据面断了
   * <video> 报 MediaError，重新 prepare 一次接续，续播位置由 lastKnownRef 接上；
   * 每集只自动兜一次。配对的 setSrc(null) 把 <video> 卸载，重挂载才会真的重新加载。
   */
  const [retry, setRetry] = useState<{ key: string; tick: number }>({ key: '', tick: 0 });
  const retryTick = retry.key === episodeKey ? retry.tick : 0;
  /** 在线取流/解密进度：播未下载的集时显示「正在缓存 45%（54/120MB）」。 */
  const [buffering, setBuffering] = useState<OnlineProgress | null>(null);
  /**
   * 用户选的清晰度，`undefined` = 后端取平台最高档。存组件不存 store：
   * 切集应回到「自动最高档」，把上一集档位带过去多半触发回退。
   */
  const [definition, setDefinition] = useState<number | undefined>(undefined);
  /** 本集实际生效的档位与全部可选档位，由起播响应带回。 */
  const [activeDefinition, setActiveDefinition] = useState(0);
  const [definitions, setDefinitions] = useState<VideoDefinition[]>([]);

  const { mutate: play } = usePlay();

  /** 重新取流接续播放（自动重试与手动重试按钮共用）。 */
  const retryOnline = useCallback(() => {
    setError(null);
    setSrc(null);
    setRetry((r) => ({ key: episodeKey, tick: (r.key === episodeKey ? r.tick : 0) + 1 }));
  }, [episodeKey]);

  // 取流健康守护换档到达：正在失败页的话自动重试并明示（换档是后台静默发生，失败页不会自己恢复）；
  // 正在播则不打扰——换档只影响下一次取流，画面与缓存流都不动
  useEvent(
    EVENTS.deviceRegistered,
    useCallback(() => {
      if (!error) return;
      toast.info(t('player.deviceSwappedRetry'));
      retryOnline();
    }, [error, retryOnline]),
  );

  // 取流失败自动重试：按退避节奏自动重取并显示进度，耗尽才退回手动重试；
  // 换集清零，手动重试不重置计数（自动预算有限）。定时器凭 key+attempt 读时校验作废
  const [autoRetry, setAutoRetry] = useState<{ attempt: number } | null>(null);
  const autoRetryRef = useRef({ key: '', attempt: 0 });
  useEffect(() => {
    if (autoRetryRef.current.key !== episodeKey) {
      autoRetryRef.current = { key: episodeKey, attempt: 0 };
    }
    if (!error) {
      queueMicrotask(() => setAutoRetry(null));
      return;
    }
    const attempt = autoRetryRef.current.attempt + 1;
    autoRetryRef.current.attempt = attempt;
    const show = { attempt };
    queueMicrotask(() => setAutoRetry(show));
    const delay =
      attempt <= AUTO_RETRY_FAST_ATTEMPTS
        ? AUTO_RETRY_FAST_DELAYS_MS[attempt - 1]
        : AUTO_RETRY_SETTLED_DELAY_MS;
    const timer = setTimeout(() => {
      if (autoRetryRef.current.key !== episodeKey || autoRetryRef.current.attempt !== attempt) {
        return;
      }
      retryOnline();
    }, delay);
    return () => clearTimeout(timer);
  }, [error, episodeKey, retryOnline]);

  // 在线取流进度：只认当前这一集，换集后清掉。
  useEvent(
    EVENTS.onlinePlayProgress,
    useCallback(
      (p: OnlineProgress) => {
        setBuffering(p.key === episodeKey ? p : null);
      },
      [episodeKey],
    ),
  );

  // 起播：依赖带 definition（切清晰度要重新取流）。src 换 URL 会重置 currentTime，
  // 当前位置先存 pendingSeek，否则从 10 分钟处切 720p 会被弹回片头
  useEffect(() => {
    if (!seriesId || !vidIndex) return;

    const video = videoRef.current;
    // 只在元素里还是这一集的流时才续点；信息流切剧时元素里是上一部剧的画面，读它就串剧
    if (video && video.currentTime > 0 && srcKeyRef.current === episodeKey) {
      lastKnownRef.current = {
        key: episodeKey,
        time: video.currentTime,
        duration: Number.isFinite(video.duration) ? video.duration : lastKnownRef.current.duration,
      };
    }
    // 切清晰度接着当前播放点（不回到「上次看的进度」），只认属于本集的那份
    const keepPosition = lastKnownRef.current.key === episodeKey ? lastKnownRef.current.time : 0;

    // 跨客户端续播提示：本地播放档案没有这一集位置（resumeAt=0）时用提示里的集内位置兜底，消费即清
    const hint = resumeHint;
    const hintMs =
      hint && hint.seriesId === seriesId && hint.vidIndex === vidIndex ? hint.positionMs / 1000 : 0;

    play(
      { seriesId, vidIndex, definition },
      {
        onSuccess: (res) => {
          srcKeyRef.current = episodeKey;
          setSrcKey(episodeKey);
          setError(res.error || null);
          setSrc(res.error ? null : res.url);
          setActiveDefinition(res.definition);
          setDefinitions(res.definitions);
          // 续播位置在 metadata 加载后 seek，优先级：本会话播放点 > 本地 resumeAt > 云端历史提示
          pendingSeek.current = res.error
            ? 0
            : keepPosition > 0
              ? keepPosition
              : res.resumeAt > 0
                ? res.resumeAt
                : hintMs;
          if (!res.error && hintMs > 0) clearResumeHint(null);
        },
        onError: (e) => {
          srcKeyRef.current = '';
          setSrcKey('');
          setError(e.message);
          setSrc(null);
        },
      },
    );
    // ref 形参按仓库惯例写进依赖（身份恒定，见 use-playback-progress）
  }, [
    seriesId,
    vidIndex,
    definition,
    episodeKey,
    play,
    retryTick,
    resumeHint,
    clearResumeHint,
    videoRef,
    srcKeyRef,
    lastKnownRef,
  ]);

  const handleLoadedMetadata = useCallback(() => {
    const video = videoRef.current;
    if (!video) return;
    if (pendingSeek.current > 0 && pendingSeek.current < video.duration) {
      video.currentTime = pendingSeek.current;
    }
    pendingSeek.current = 0;
    // metadata 就绪后再显式播一次：autoPlay 属性带声音时会被 WebView 拦下，只靠属性就是「自动播一下就停住」
    void video.play().catch(() => undefined);
  }, [videoRef, pendingSeek]);

  // 每次渲染后把倍速/音量/静音贴回当前元素；依赖故意留空——元素重建的时机（切集/加载/转码）不由本组件依赖决定
  useEffect(() => {
    const video = videoRef.current;
    if (!video) return;
    if (Math.abs(video.playbackRate - playbackRateRef.current) > 0.001) {
      video.playbackRate = playbackRateRef.current;
    }
    if (Math.abs(video.volume - volumeRef.current) > 0.001) {
      video.volume = volumeRef.current;
    }
    if (video.muted !== mutedRef.current) {
      video.muted = mutedRef.current;
    }
  });

  /** 记住用户改的倍速（原生 controls 菜单触发） */
  const handleRateChange = useCallback((e: React.SyntheticEvent<HTMLVideoElement>) => {
    const rate = e.currentTarget.playbackRate;
    if (Number.isFinite(rate) && rate > 0) {
      playbackRateRef.current = rate;
      writePlaybackRate(rate);
    }
  }, []);

  /** 记住用户改的音量与静音（静音也会触发 volumechange，所以两项一起记） */
  const handleVolumeChange = useCallback((e: React.SyntheticEvent<HTMLVideoElement>) => {
    const el = e.currentTarget;
    if (Number.isFinite(el.volume) && el.volume >= 0 && el.volume <= 1) {
      volumeRef.current = el.volume;
      writeVolume(el.volume);
    }
    if (el.muted !== mutedRef.current) {
      mutedRef.current = el.muted;
      writeMuted(el.muted);
    }
  }, []);

  return {
    src,
    setSrc,
    error,
    setError,
    autoRetry,
    retryTick,
    srcKey,
    setSrcKey,
    srcKeyRef,
    lastKnownRef,
    buffering,
    definition,
    setDefinition,
    activeDefinition,
    definitions,
    retryOnline,
    handleLoadedMetadata,
    handleRateChange,
    handleVolumeChange,
  };
}
