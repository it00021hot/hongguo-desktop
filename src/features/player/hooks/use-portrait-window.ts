import { useCallback, useEffect } from 'react';
import { toast } from 'sonner';
import { useUiStore } from '@/stores/ui';
import { applyWindowFullscreen } from '@/hooks/use-window-fullscreen';
import { app as appApi } from '@/service/commands';

/**
 * 竖屏播放（对齐 hgplayer 1.1.8 的 rE/kc/Nf/oE/Fc，几何算法在后端
 * window_cmd）：窗口收窄成竖条，比例带手机形地板（见 PHONE_LIKE_ASPECT）
 * ——像在热门手机上观看，而不是按内容比例出的矮胖条。store 是后端
 * 快照的镜像（同 miniScreen 口径）。
 */

/** 宽高比 ≥0.9（近方形/横屏）不算竖屏比：竖屏只对竖版内容有意义
 *  （hgplayer Em/cu 同款口径）。 */
const PORTRAIT_ASPECT_MAX = 0.9;
/**
 * 手机形比例地板（w/h）：窗口比例 = min(视频比例, 本值)。热门手机屏幕
 * 是 19.5:9~20:9（iPhone/三星 2340×1080=19.5:9，国产安卓多为 20:9），
 * 而短剧内容几乎都是 9:16——按内容包窗会得到一条矮胖竖条，不像手机。
 * 取 19.5:9：9:16 内容在手机形窗口上下留边（与真手机上观看 9:16 视频
 * 一致），比手机更长的内容（如 9:21）仍按自身比例包住不留边。
 */
const PHONE_LIKE_ASPECT = 9 / 19.5;
/** 宽高比变化超过 1% 才重适配——浮点抖动与同比例换流不触发（hgplayer Em）。 */
const REFIT_DELTA = 0.01;

/** 竖屏用的宽高比：竖屏比就与手机比例取更「长」的一方，否则回落手机
 * 比例（无元数据/横屏内容下窗口同样是手机形竖条）。 */
export function portraitAspectOf(video: HTMLVideoElement | null): number {
  const ar =
    video && video.videoWidth > 0 && video.videoHeight > 0
      ? video.videoWidth / video.videoHeight
      : 0;
  return ar > 0 && ar < PORTRAIT_ASPECT_MAX ? Math.min(ar, PHONE_LIKE_ASPECT) : PHONE_LIKE_ASPECT;
}

/** 进竖屏先退全屏：全屏窗口尺寸归系统管，收窄会被吞掉（进小屏同款守卫）。 */
async function enterPortrait(aspect: number): Promise<void> {
  if (useUiStore.getState().windowFullscreen) {
    void applyWindowFullscreen(false).catch(() => undefined);
  }
  useUiStore.getState().setPortrait(true);
  await appApi.enterPortraitScreen(aspect).catch((e: Error) => {
    useUiStore.getState().setPortrait(false);
    throw e;
  });
}

/** 退出竖屏并清镜像态。进小屏也走这里——竖屏的几何快照是单份的，进小屏前
 *  必须先还原，否则小屏的快照记的是竖条几何。（离开播放区的收尾在
 *  app-shell 的路由监听里，与本函数同款调用。） */
export function exitPortraitScreen(): void {
  if (!useUiStore.getState().portrait) return;
  useUiStore.getState().setPortrait(false);
  void appApi.exitPortraitScreen().catch(() => undefined);
}

/**
 * 竖屏开关与铺满档切换。切集/换清晰度导致视频尺寸变化时自动重适配：
 * video 元素在 videoWidth/videoHeight 变化时发 `resize` 事件，比在
 * onMeta 里埋回调干净——竖屏真值判定与重适配都收在本 hook 内。
 */
export function usePortraitWindow(params: { videoRef: React.RefObject<HTMLVideoElement | null> }) {
  const { videoRef } = params;
  const portrait = useUiStore((s) => s.portrait);
  const portraitFull = useUiStore((s) => s.portraitFull);

  const togglePortrait = useCallback(() => {
    if (useUiStore.getState().portrait) {
      exitPortraitScreen();
    } else {
      void enterPortrait(portraitAspectOf(videoRef.current)).catch((e: Error) =>
        toast.error(e.message),
      );
    }
  }, [videoRef]);

  /** 竖屏中全屏键变成铺满切换（hgplayer wu：portrait 时全屏语义让位）。
   *  非竖屏时是 no-op——调用方负责在竖屏时才换语义。 */
  const togglePortraitFull = useCallback(() => {
    const st = useUiStore.getState();
    if (!st.portrait) return;
    const next = !st.portraitFull;
    st.setPortraitFull(next);
    void appApi
      .setPortraitFull(portraitAspectOf(videoRef.current), next)
      .catch((e: Error) => toast.error(e.message));
  }, [videoRef]);

  // 切集/换清晰度 → 视频尺寸变化 → 自动重适配（hgplayer Em：变化 >1% 才
  // 动；铺满与否跟随当前档）。比例一律取钳制后的：9:16→9:16 换集钳到同一
  // 手机比例不触发；真换比例（如 9:21 素材）或换成横屏（回落手机比例）
  // 才重算窗口。
  useEffect(() => {
    const video = videoRef.current;
    if (!video) return;
    let last = portraitAspectOf(video);
    const refit = () => {
      const st = useUiStore.getState();
      if (!st.portrait) return;
      const ar = portraitAspectOf(video);
      if (Math.abs(ar - last) <= REFIT_DELTA) return;
      last = ar;
      void appApi.fitPortraitScreen(ar, st.portraitFull).catch(() => undefined);
    };
    video.addEventListener('resize', refit);
    video.addEventListener('loadedmetadata', refit);
    return () => {
      video.removeEventListener('resize', refit);
      video.removeEventListener('loadedmetadata', refit);
    };
  }, [videoRef]);

  return { portrait, portraitFull, togglePortrait, togglePortraitFull };
}
