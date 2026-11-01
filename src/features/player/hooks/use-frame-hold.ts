import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react';

/** 切换类型：同剧切集走黑幕暗转（use-switch-veil），切剧/初次起播走封面接管。 */
export type SwitchKind = 'episode' | 'series';

/** 定格帧淡出时长（FrameHold 的 transition 是 300ms，出画后再留一拍清掉）。 */
const HOLD_FADE_MS = 350;

/**
 * 换流间隙的末帧定格（清晰度切换专用）。
 *
 * 同剧**切集**已改黑幕暗转（use-switch-veil）：切集间隙由黑幕盖住，静止
 * 定格帧不再参与——「静止 0.7s 再突跳」的卡感正来自它。定格帧剩下的唯一
 * 场景是**清晰度切换**（episodeKey 不变、veil 不置）：换档换流换元素，
 * 抓下的当前帧把间隙顶住（调用方在换档前手动 [`capture`]）。
 */
export function useFrameHold(params: {
  videoRef: React.RefObject<HTMLVideoElement | null>;
  seriesId: string | null;
  episodeKey: string;
  /** 播放侧真值（定义见 player-page）：出画 / 切换在途 / 流指纹。 */
  videoLive: boolean;
  switching: boolean;
  streamKey: string;
  playSrc: string | null;
  error: string | null;
  compatActive: boolean;
  /** 黑幕在场（同剧切集间隙）：定格帧与封面都退场，观感交给暗转。 */
  veiled: boolean;
}) {
  const {
    videoRef,
    seriesId,
    episodeKey,
    videoLive,
    switching,
    streamKey,
    playSrc,
    error,
    compatActive,
    veiled,
  } = params;
  const [transition, setTransition] = useState<SwitchKind | null>(null);
  /**
   * 定格帧与抓帧时的流指纹：新流（≠抓帧来源）真正出画后据此判定帧已
   * 功成身退——抓帧来源自身还在播（取流窗口）时不算，否则帧会在切换
   * 途中被误清（清晰度切换全程 srcKey 不变，尤其踩这条）。
   */
  const [held, setHeld] = useState<{ canvas: HTMLCanvasElement; fromStream: string } | null>(null);
  const prevTargetRef = useRef<{ seriesId: string | null; episodeKey: string }>({
    seriesId: null,
    episodeKey: '',
  });

  /** 把当前画面定格成一张 canvas（拿不到画面就清空，回落封面）。 */
  const capture = useCallback(() => {
    const video = videoRef.current;
    if (!video || video.videoWidth === 0 || video.videoHeight === 0) {
      setHeld(null);
      return;
    }
    const canvas = document.createElement('canvas');
    canvas.width = video.videoWidth;
    canvas.height = video.videoHeight;
    canvas.getContext('2d')?.drawImage(video, 0, 0);
    setHeld({ canvas, fromStream: streamKey });
  }, [videoRef, streamKey]);

  useLayoutEffect(() => {
    const prev = prevTargetRef.current;
    if (prev.episodeKey === episodeKey) return;
    prevTargetRef.current = { seriesId, episodeKey };
    if (!seriesId) return;
    const kind: SwitchKind = prev.seriesId === seriesId ? 'episode' : 'series';
    setTransition(kind);
    // 切集交给黑幕、切剧交给封面：跨切换的旧帧一律清掉，绝不许泄漏到
    // 新内容上（换档的帧由调用方在换档前手动抓）
    setHeld(null);
  }, [seriesId, episodeKey]);

  // 新流出画后定格帧淡出并清掉：一张 ~1080p 帧的 canvas 有几 MB，不留整集
  useEffect(() => {
    if (!held || !videoLive || held.fromStream === streamKey) return;
    const timer = setTimeout(() => setHeld(null), HOLD_FADE_MS);
    return () => clearTimeout(timer);
  }, [held, videoLive, streamKey]);

  const { holdVisible, coverVisible } = overlayVisibility({
    transition,
    veiled,
    videoLive,
    switching,
    hasFrame: held !== null,
    hasPlaySrc: playSrc !== null,
    error: error !== null,
    compat: compatActive,
  });

  return { frame: held?.canvas ?? null, holdVisible, coverHidden: !coverVisible, capture };
}

/**
 * 封面与定格帧的显隐真值表（纯函数，用例见 use-frame-hold.test.ts）：
 * - 黑幕在场（同剧切集间隙）：定格帧与封面全部退场，观感交给暗转——
 *   揭幕时新画面已在动，任何静止遮罩都不需要；
 * - 定格帧：黑幕不在场、有帧、无出画画面、仍在视频分支（换流间隙顶住）；
 * - 封面：画面真空（既没出画也没定格帧顶住），或切剧接管（switching 且
 *   非同剧切集——新剧的封面要立刻顶上，否则就是「切了没反应」）。
 *   同剧切集全程不出封面：黑幕接管（换档间隙由定格帧接棒）。
 */
export function overlayVisibility(v: {
  transition: SwitchKind | null;
  veiled: boolean;
  videoLive: boolean;
  switching: boolean;
  hasFrame: boolean;
  hasPlaySrc: boolean;
  error: boolean;
  compat: boolean;
}): { holdVisible: boolean; coverVisible: boolean } {
  const holdVisible =
    v.hasFrame && !v.videoLive && v.hasPlaySrc && !v.error && !v.compat && !v.veiled;
  const coverVisible = v.veiled
    ? false
    : !holdVisible && (!v.videoLive || (v.switching && v.transition !== 'episode'));
  return { holdVisible, coverVisible };
}
