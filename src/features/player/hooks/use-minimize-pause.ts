import { useEffect, useRef } from 'react';

/**
 * 最小化自动暂停（对齐 hgplayer：窗口最小化时自动暂停，恢复后继续播放）。
 *
 * 判定用 document visibilitychange：WebView2/Chromium 在窗口最小化时把
 * document 置为 hidden（切到别的窗口不会触发，正好就是「最小化」语义）。
 * macOS WKWebView 对最小化不一定派发该事件——那时不自动暂停，静默降级。
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
      return;
    }
    const onVisibility = () => {
      const video = videoRef.current;
      if (!video) return;
      if (document.visibilityState === 'hidden') {
        // 只在「正在播」时欠：已经暂停着说明是用户自己按停的
        owedPlay.current = !video.paused;
        if (owedPlay.current) video.pause();
      } else if (owedPlay.current) {
        owedPlay.current = false;
        void video.play().catch(() => undefined);
      }
    };
    document.addEventListener('visibilitychange', onVisibility);
    return () => document.removeEventListener('visibilitychange', onVisibility);
  }, [enabled, videoRef]);
}
