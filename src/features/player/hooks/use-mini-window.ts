import { useCallback } from 'react';
import { toast } from 'sonner';
import { useUiStore } from '@/stores/ui';
import { enterMiniScreen, exitMiniScreen } from '@/hooks/window-presentation';

/**
 * 小屏播放：同一窗口缩成 480×270 落屏幕右下角，侧栏隐藏、控件换紧凑条——
 * video 元素原地不动，播放零中断（不暂停、不落库、不换页）。播放器只有首页沉浸流一个宿主，
 * 小屏进出全程原地完成。
 */

export function useMiniWindow(params: {
  /** 进出小屏都要清悬停态：组件卸载不触发 mouseleave，残留会让控制条常显不收 */
  setControlsHovered: (hovered: boolean) => void;
  setCommentPanelOpen: (open: boolean) => void;
  setDanmakuPanelOpen: (open: boolean) => void;
  setVolumeOpen: (open: boolean) => void;
  setSeriesPanelOpen: (open: boolean) => void;
}) {
  const {
    setControlsHovered,
    setCommentPanelOpen,
    setDanmakuPanelOpen,
    setVolumeOpen,
    setSeriesPanelOpen,
  } = params;
  const miniScreen = useUiStore((s) => s.miniScreen);

  const enterMini = useCallback(() => {
    // 大屏的浮层面板带不进 480×270 的小窗：进小屏前一并收掉
    setCommentPanelOpen(false);
    setDanmakuPanelOpen(false);
    setVolumeOpen(false);
    setSeriesPanelOpen(false);
    // 悬停态跟着旧控制栏一起卸载（组件卸载不触发 mouseleave）
    setControlsHovered(false);
    void enterMiniScreen().catch((e: Error) => toast.error(e.message));
  }, [
    setCommentPanelOpen,
    setDanmakuPanelOpen,
    setVolumeOpen,
    setSeriesPanelOpen,
    setControlsHovered,
  ]);

  /** 退出小屏：恢复窗口几何。播放器在首页原地，无需任何导航。 */
  const exitMini = useCallback(() => {
    setControlsHovered(false); // 同 enterMini：悬停态别跨大小屏残留
    void exitMiniScreen().catch((e: Error) => toast.error(e.message));
  }, [setControlsHovered]);

  return { miniScreen, enterMini, exitMini };
}
