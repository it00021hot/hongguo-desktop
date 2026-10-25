import { useCallback } from 'react';
import { toast } from 'sonner';
import { useUiStore } from '@/stores/ui';
import { applyWindowFullscreen } from '@/hooks/use-window-fullscreen';
import { app as appApi } from '@/service/commands';
import { exitPortraitScreen } from '@/features/player/hooks/use-portrait-window';

/**
 * 小屏播放：同一窗口缩成 480×270 落到屏幕
 * 右下角，侧栏隐藏、控件换紧凑条——video 元素原地不动，播放零中断
 * （不暂停、不落库、不换页）。
 *
 * 播放器只有首页沉浸流一个宿主（/player 已并回首页）：小屏进出全程
 * 原地完成，信息流的顶部 tab/悬浮层在 miniScreen 下各自收敛不渲染。
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
  const setMiniScreen = useUiStore((s) => s.setMiniScreen);

  const enterMini = useCallback(() => {
    // 全屏窗口的尺寸归系统管（mac 全屏态 set_size 无效）：进小屏先退全屏
    if (useUiStore.getState().windowFullscreen) {
      void applyWindowFullscreen(false).catch(() => undefined);
    }
    // 竖屏几何快照是单份的：先还原大窗再进小屏，否则小屏快照记的是
    // 竖条几何，退出小屏会停在竖条尺寸（exitPortraitScreen 幂等，非竖屏 no-op）
    exitPortraitScreen();
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
  }, [
    setMiniScreen,
    setCommentPanelOpen,
    setDanmakuPanelOpen,
    setVolumeOpen,
    setSeriesPanelOpen,
    setControlsHovered,
  ]);

  /** 退出小屏：恢复窗口几何。播放器在首页原地，无需任何导航。 */
  const exitMini = useCallback(() => {
    setMiniScreen(false);
    setControlsHovered(false); // 同 enterMini：悬停态别跨大小屏残留
    void appApi.exitMiniScreen().catch(() => undefined);
  }, [setMiniScreen, setControlsHovered]);

  return { miniScreen, enterMini, exitMini };
}
