import { useCallback, useEffect, useRef } from 'react';
import { useEvent } from '@/service/tauri/events';
import { EVENTS } from '@/service/tauri/types';

/**
 * 最小化自动暂停：窗口最小化时自动暂停，恢复后继续播放。
 *
 * 判定走 Rust 侧：WebView2 **最小化时不派发 visibilitychange**（dev 实测
 * 最小化后 visibilityState 恒 "visible"），前端无从感知——lib.rs 在窗口
 * Resized 时查 is_minimized，状态翻转才 emit `window-minimize`（true=最小化，
 * false=恢复），两端平台语义一致。
 *
 * 续播只续被最小化暂停的那一次（隐身模式同款欠账模型）：最小化瞬间
 * 本来就在播才记欠账；用户自己按停的恢复后仍停着。与 incognito 的
 * owedPlay 各自独立，互不接管。
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
