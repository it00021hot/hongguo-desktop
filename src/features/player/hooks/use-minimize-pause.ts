import { useCallback, useEffect, useRef } from 'react';
import { useEvent } from '@/service/tauri/events';
import { EVENTS } from '@/service/tauri/types';

/**
 * 最小化自动暂停/恢复续播。判定走 Rust 侧：WebView2 最小化不派发 visibilitychange，lib.rs 查 is_minimized 翻转才 emit。
 * 续播只续被最小化暂停的那一次（与 incognito 各自独立）；用户自己按停的恢复后仍停着。
 */
export function useMinimizeAutoPause(params: {
  videoRef: React.RefObject<HTMLVideoElement | null>;
  enabled: boolean;
}) {
  const { videoRef, enabled } = params;
  const owedPlay = useRef(false);

  useEffect(() => {
    if (!enabled) {
      owedPlay.current = false;
    }
  }, [enabled]);

  useEvent(
    EVENTS.windowMinimize,
    useCallback(
      (minimized: boolean) => {
        const video = videoRef.current;
        if (!video || !enabled) return;
        if (minimized) {
          // 只在「正在播」时欠：已经暂停着说明是用户自己按停的
          owedPlay.current = !video.paused;
          if (owedPlay.current) video.pause();
        } else if (owedPlay.current) {
          owedPlay.current = false;
          void video.play().catch(() => undefined);
        }
      },
      [videoRef, enabled],
    ),
  );
}
