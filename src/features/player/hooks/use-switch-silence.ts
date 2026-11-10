import { useCallback, useEffect, useRef } from 'react';

/**
 * 切换在途时让旧流闭嘴：目标已换成新的一集、新流还没到的这段时间，元素里仍是上一条流。
 * 封面和加载圈已经接管画面，但旧流不停的话它的声音会一直压在新剧的封面上——
 * 取流要重新换档时这段间隙有二十多秒。
 *
 * 只欠「被切换暂停」的那一次（与 use-minimize-pause 同一口径）：切回原集、流没换时由这里续上；
 * 用户自己按停的不动。新流到达会换元素并自动起播，那时欠的这次直接作废。
 */
export function useSwitchSilence(params: {
  videoRef: React.RefObject<HTMLVideoElement | null>;
  /** 目标集与元素里的流不一致（定义见 player-page）。 */
  switching: boolean;
}) {
  const { videoRef, switching } = params;
  /** 被本 hook 暂停的那个元素；换了元素就不再欠它一次播放。 */
  const silenced = useRef<HTMLVideoElement | null>(null);

  useEffect(() => {
    const video = videoRef.current;
    const action = switchSilenceAction({
      switching,
      playing: video ? !video.paused : false,
      owed: silenced.current !== null,
      sameElement: video !== null && video === silenced.current,
    });
    if (action === 'pause' && video) {
      silenced.current = video;
      video.pause();
    } else if (action === 'resume' && video) {
      silenced.current = null;
      void video.play().catch(() => undefined);
    } else if (action === 'forget') {
      silenced.current = null;
    }
  }, [switching, videoRef]);

  /** 这次 pause 事件是不是切换暂停引起的（是的话别把界面切到「用户已暂停」）。 */
  const isSwitchPause = useCallback(
    (video: HTMLVideoElement) => silenced.current === video,
    [silenced],
  );

  /** 切换在途时旧流被重新播起来（空格键等）就再按住。返回 true 表示已拦下。 */
  const holdIfSwitching = useCallback(
    (video: HTMLVideoElement) => {
      if (!switching) return false;
      silenced.current = video;
      video.pause();
      return true;
    },
    [switching, silenced],
  );

  return { isSwitchPause, holdIfSwitching };
}

/** 切换状态变化时对旧流做什么（纯函数，用例见 use-switch-silence.test.ts）。 */
export function switchSilenceAction(v: {
  switching: boolean;
  /** 元素存在且正在播 */
  playing: boolean;
  /** 之前被本 hook 暂停过、还没还 */
  owed: boolean;
  /** 现在的元素就是当初被暂停的那个 */
  sameElement: boolean;
}): 'pause' | 'resume' | 'forget' | 'none' {
  if (v.switching) return v.playing ? 'pause' : 'none';
  if (!v.owed) return 'none';
  // 切换收口：流没换（切回原集）就把欠的那次还上；换了元素说明新流已自动起播
  return v.sameElement ? 'resume' : 'forget';
}
