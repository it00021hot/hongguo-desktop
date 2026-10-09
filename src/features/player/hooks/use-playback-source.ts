import { useCallback, useEffect, useRef, useState } from 'react';
import { usePlay } from '@/service/queries';
import { usePlayerStore } from '@/stores/player';
import { useEvent } from '@/service/tauri/events';
import { EVENTS } from '@/service/tauri/types';
import {
  readMuted,
  readPlaybackRate,
  readVolume,
  writeMuted,
  writePlaybackRate,
  writeVolume,
} from '@/utils/playback-prefs';
import type { OnlineProgress, VideoDefinition } from '@/service/schema';

/**
 * 取流与起播：play mutation、清晰度切换、断供自动重试、在线取流进度
 * 订阅，以及「用户设的倍速/音量/静音」跨元素重建的贴回。
 *
 * 三把竞态防御 ref 的真值在这里持有、供相邻 hook 读写：
 * - `srcKeyRef`：现在这条流属于哪一集——persist 落盘与兜底转码成功
 *   都凭它认流（信息流切剧时 play 在途、旧流还在播，不带这道闸会把
 *   旧画面的秒数记到新剧头上）；
 * - `lastKnownRef`：最近一次播放位置/时长（带集 key），起播续点与
 *   卸载补写共用；
 * - `pendingSeek`：metadata 就绪后再执行的续播位置。
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
  /**
   * 最近一次的播放位置与时长，无论有没有真的落盘。
   *
   * 卸载时 `<video>` 已经先一步被卸载，从元素上再读 currentTime 是拿不到的；
   * 内存里这份是唯一能在 cleanup 里用到的真值。
   *
   * `key` 记的是这份位置属于哪一集：组件按剧集整体重挂载，但 ref 不会重置，
   * 不带 key 的话新一集会拿着上一集的秒数去续播。
   */
  const lastKnownRef = useRef({ key: '', time: 0, duration: 0 });
  /**
   * 现在这条流属于哪一集（episodeKey）。信息流切剧时 play 请求在途、
   * 旧流还在播（timeupdate 一直在来），不带这道闸会把旧画面的秒数
   * 记到新剧头上——续播位置凭空串剧。
   */
  const srcKeyRef = useRef('');

  // 切集 / 加载中都会让 <video> 被卸载重建，新元素的倍速音量静音
  // 一律回到默认值，所以「用户设的值」要存在这里，每次渲染后再贴回元素。
  const playbackRateRef = useRef(readPlaybackRate());
  const volumeRef = useRef(readVolume());
  const mutedRef = useRef(readMuted());

  const resumeHint = usePlayerStore((s) => s.resumeHint);
  const clearResumeHint = usePlayerStore((s) => s.setResumeHint);
  const [src, setSrc] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  /**
   * 现在这条流属于哪一集（srcKeyRef 的 state 镜像，同点写入）。
   * 渲染期据此判定「切换在途」：目标已变、新流未到——旧画面还在播，
   * 但新剧的封面+加载胶囊要立即接管，否则用户看到的就是「切了没反应」。
   */
  const [srcKey, setSrcKey] = useState('');
  /**
   * 在线流断供的自动重试（带集指纹，读时校验，换集自动失效）。
   *
   * 渐进流的数据面断了（切剧竞态、网络抖动）会让 <video> 报 MediaError；
   * 重新 prepare 一次就能拿到新流，续播位置由 lastKnownRef 接上。每集只自动
   * 兜一次——再失败多半不是抖动，亮出重试按钮交给用户。tick 进起播
   * effect 的依赖驱动重新取流；配对的 setSrc(null) 把 <video> 卸载，
   * 重挂载才会真的重新加载（URL 不变时只换 src 属性在 WebView2 上未必
   * 触发重载）。
   */
  const [retry, setRetry] = useState<{ key: string; tick: number }>({ key: '', tick: 0 });
  const retryTick = retry.key === episodeKey ? retry.tick : 0;
  /**
   * 在线取流/解密的进度。
   *
   * 整集取回 + 解密要等一会儿，这段时间里界面上原本只有一个转圈。
   * 播未下载的集时这里显示「正在缓存 45%（54/120MB）」，用户才知道它在动。
   */
  const [buffering, setBuffering] = useState<OnlineProgress | null>(null);
  /**
   * 用户选的清晰度。`undefined` = 不指定，由后端取平台给的最高档。
   *
   * 存在组件里而不是 store：切集时应当回到「自动最高档」，
   * 不同集提供的档位本来就不一样，把上一集的档位带过去多半要触发回退。
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

  // 在线取流进度：只认当前这一集，换集后清掉。
  useEvent<OnlineProgress>(
    EVENTS.onlinePlayProgress,
    useCallback(
      (p: OnlineProgress) => {
        setBuffering(p.key === episodeKey ? p : null);
      },
      [episodeKey],
    ),
  );

  // 起播。依赖里带 definition：切清晰度要重新取流，
  // 而 `<video src>` 换 URL 会重置 currentTime，所以先把当前位置存进
  // pendingSeek —— 否则用户从 10 分钟处切到 720p 会被弹回片头。
  useEffect(() => {
    if (!seriesId || !vidIndex) return;

    const video = videoRef.current;
    // 只在元素里还是这一集的流时才续点（切清晰度场景）；信息流切剧时
    // 元素里还是上一部剧的画面，读它的 currentTime 就是串剧
    if (video && video.currentTime > 0 && srcKeyRef.current === episodeKey) {
      lastKnownRef.current = {
        key: episodeKey,
        time: video.currentTime,
        duration: Number.isFinite(video.duration) ? video.duration : lastKnownRef.current.duration,
      };
    }
    // 切清晰度时续播位置要接着当前播放点，而不是回到「上次看的进度」——
    // 那会把人从 10 分钟处弹回上次退出点，看着像「切清晰度丢了进度」。
    // 只认属于本集的那份：换集后 lastKnownRef 里是上一集的秒数，拿来续播就串集了。
    const keepPosition = lastKnownRef.current.key === episodeKey ? lastKnownRef.current.time : 0;

    // 跨客户端续播提示（信息流从云端历史定的起点）：本地播放档案没有
    // 这一集的位置（resumeAt=0）时，用提示里的集内位置兜底。消费即清。
    const hint = resumeHint;
    const hintMs =
      hint && hint.seriesId === seriesId && hint.vidIndex === vidIndex
        ? hint.positionMs / 1000
        : 0;

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
          // 续播位置要在 metadata 加载后 seek。优先级：本会话播放点 >
          // 本地播放档案 resumeAt > 云端历史提示（跨客户端进度）
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
    // metadata 就绪后再显式播一次：autoPlay 属性在带声音时会被 WebView 拦下，
    // 只靠属性的表现就是「自动播一下就停住」，必须在这里补一次 play()。
    void video.play().catch(() => undefined);
  }, [videoRef, pendingSeek]);

  // 每次渲染后把倍速/音量/静音贴回当前元素。
  // 依赖数组故意留空：元素被重建的时机（切集、加载、转码）不由本组件的依赖决定，
  // 漏掉任何一次重建，用户设的倍速就会在切集后悄悄回到 1。
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
