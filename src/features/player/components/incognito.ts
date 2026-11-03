import { useCallback, useEffect, useRef, useState } from 'react';
import { app as appApi } from '@/service/commands';
import { useEvent } from '@/service/tauri/events';
import { EVENTS } from '@/service/tauri/types';
import { readIncognito, writeIncognito } from '@/utils/playback-prefs';

/**
 * 隐身模式：鼠标脱离窗口 → 隐藏 + 暂停；回到窗口 → 原地重现 + 续播（判定在后端系统级光标轮询）。
 * 续播只续被隐身暂停的那一次；主播放器与小窗共用（后端只盯活动窗口），偏好落 localStorage。
 */
export function useIncognitoMode(videoRef: React.RefObject<HTMLVideoElement | null>) {
  const [on, setOn] = useState(() => readIncognito());
  /** 隐身欠下的那次播放：回来时续播、手动暂停的不欠。 */
  const owedPlay = useRef(false);

  const toggle = useCallback(() => {
    setOn((v) => {
      writeIncognito(!v);
      return !v;
    });
  }, []);

  useEffect(() => {
    void appApi.setIncognito(on).catch(() => undefined);
  }, [on]);

  useEvent(
    EVENTS.incognitoVisibility,
    useCallback(
      (p: { visible: boolean }) => {
        const video = videoRef.current;
        if (!video) return;
        if (!p.visible) {
          // 只在「正在播」时欠：已经暂停着说明是用户自己按停的
          owedPlay.current = !video.paused;
          if (owedPlay.current) video.pause();
        } else if (owedPlay.current) {
          owedPlay.current = false;
          void video.play().catch(() => undefined);
        }
      },
      [videoRef],
    ),
  );

  return { on, toggle };
}
