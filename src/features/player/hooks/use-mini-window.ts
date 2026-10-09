import { useCallback } from 'react';
import { useNavigate } from '@tanstack/react-router';
import { toast } from 'sonner';
import { useUiStore } from '@/stores/ui';
import { app as appApi } from '@/service/commands';

/**
 * 小屏播放（对齐 hgplayer ng()/Op()）：同一窗口缩成 480×270 落到屏幕
 * 右下角，侧栏隐藏、控件换紧凑条——video 元素原地不动，播放零中断
 * （不暂停、不落库、不换页）。
 */
export function useMiniWindow(params: {
  videoRef: React.RefObject<HTMLVideoElement | null>;
  /** 信息流上下文进小屏前强落一次进度，/player 挂载后凭 resumeAt 接上 */
  persist: (time: number, force?: boolean) => void;
  onWheelStep?: (dir: 1 | -1) => void;
  /** 进出小屏都要清悬停态：组件卸载不触发 mouseleave，残留会让控制条常显不收 */
  setControlsHovered: (hovered: boolean) => void;
  setCommentPanelOpen: (open: boolean) => void;
  setDanmakuPanelOpen: (open: boolean) => void;
  setVolumeOpen: (open: boolean) => void;
  setSeriesPanelOpen: (open: boolean) => void;
}) {
  const {
    videoRef,
    persist,
    onWheelStep,
    setControlsHovered,
    setCommentPanelOpen,
    setDanmakuPanelOpen,
    setVolumeOpen,
    setSeriesPanelOpen,
  } = params;
  const navigate = useNavigate();
  const miniScreen = useUiStore((s) => s.miniScreen);
  const setMiniScreen = useUiStore((s) => s.setMiniScreen);

  const enterMini = useCallback(() => {
    // 大屏的浮层面板带不进 480×270 的小窗：进小屏前一并收掉
    setCommentPanelOpen(false);
    setDanmakuPanelOpen(false);
    setVolumeOpen(false);
    setSeriesPanelOpen(false);
    // 悬停态跟着旧控制栏一起卸载：组件卸载不触发 mouseleave，残留 true 会让
    // 小屏里鼠标已经离开画面、控制条却常显不收
    setControlsHovered(false);
    setMiniScreen(true);
    void appApi.enterMiniScreen().catch((e: Error) => toast.error(e.message));
    // 信息流上下文（首页沉浸流内嵌本组件）：小窗里只装播放器——先强落
    // 一次进度再跳纯播放页，/player 挂载后凭 resumeAt 精准接上
    if (onWheelStep) {
      const video = videoRef.current;
      if (video) persist(video.currentTime, true);
      void navigate({ to: '/player' });
    }
  }, [
    setMiniScreen,
    setCommentPanelOpen,
    setDanmakuPanelOpen,
    setVolumeOpen,
    setSeriesPanelOpen,
    setControlsHovered,
    onWheelStep,
    navigate,
    persist,
    videoRef,
  ]);

  /** 退出小屏：恢复窗口几何，留在播放页继续看。 */
  const exitMini = useCallback(() => {
    setMiniScreen(false);
    setControlsHovered(false); // 同 enterMini：悬停态别跨大小屏残留
    void appApi.exitMiniScreen().catch(() => undefined);
  }, [setMiniScreen, setControlsHovered]);

  /** 结束播放：退出小屏并回首页（小屏里唯一的「关掉」出口）。 */
  const stopMini = useCallback(() => {
    const video = videoRef.current;
    if (video && !video.paused) {
      video.pause(); // onPause 里会强制落一次进度
    }
    setMiniScreen(false);
    setControlsHovered(false); // 同 enterMini：悬停态别跨大小屏残留
    void appApi.exitMiniScreen().catch(() => undefined);
    void navigate({ to: '/' });
  }, [navigate, setMiniScreen, setControlsHovered, videoRef]);

  return { miniScreen, enterMini, exitMini, stopMini };
}
