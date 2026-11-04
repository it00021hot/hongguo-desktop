import { useCallback, useEffect, useRef } from 'react';
import { useShortcutsStore } from '@/stores/shortcuts';
import { useUiStore } from '@/stores/ui';
import { app as appApi } from '@/service/commands';
import { portraitAspectOf } from '@/features/player/hooks/use-portrait-window';
import { setAndroidLandscape } from '@/utils/android-shell';

/**
 * 舞台交互裁决：滚轮/触摸滑动切换、点击画面（播放/暂停）、键盘快捷键。
 * 滚轮/↑↓/触摸滑动的语义由宿主注入的 `onWheelStep` 与连播锁定共同决定：
 * 沉浸流 = 上一部/下一部剧，从详情/历史等选定剧进来（或锁定连播后）= 切上一集/下一集。
 */

/** 触摸滑动判定阈值（px）：低于是轻触（tap 播放/暂停），高于且纵向显著强于横向才算一次切换。 */
const SWIPE_THRESHOLD_PX = 60;
export function usePlayerInteractions(params: {
  videoRef: React.RefObject<HTMLVideoElement | null>;
  /** 方向信号（1=下一个/向下滚，-1=上一个）由交互写入，过渡动画在装配层读 */
  setSlideDir: (dir: 1 | -1) => void;
  /** 选集浮层打开时不抢滚动 */
  seriesPanelOpen: boolean;
  /** 下载面板打开时不抢滚动/不切集 */
  downloading: boolean;
  inBinge: boolean;
  stepEpisode: (delta: number) => void;
  onWheelStep?: (dir: 1 | -1) => void;
  wakeChrome: () => void;
  setBinge: (seriesId: string | null) => void;
  commentPanelOpen: boolean;
  danmakuPanelOpen: boolean;
  volumeOpen: boolean;
}) {
  const {
    videoRef,
    setSlideDir,
    seriesPanelOpen,
    downloading,
    inBinge,
    stepEpisode,
    onWheelStep,
    wakeChrome,
    setBinge,
    commentPanelOpen,
    danmakuPanelOpen,
    volumeOpen,
  } = params;

  // 切换分派三端同源（滚轮/触摸滑动/键盘）
  const stepBy = useCallback(
    (dir: 1 | -1) => {
      setSlideDir(dir);
      // 连播锁定时切换语义变为切集，不跟随宿主页换剧
      if (inBinge) stepEpisode(dir);
      else if (onWheelStep) onWheelStep(dir);
      else stepEpisode(dir);
    },
    [inBinge, onWheelStep, stepEpisode, setSlideDir],
  );

  // 滚轮切换：浮层/下载面板打开时不抢滚动；冷却 400ms 防一次惯性滚动连跳
  const stepLock = useRef(0);
  const onStageWheel = useCallback(
    (e: React.WheelEvent) => {
      // 浮层（评论/选集/弹幕设置/菜单）里的滚动是它自己在滚；Radix 菜单 portal 到
      // body 不是舞台子孙，必须各自带 data-wheel-block 标记才拦得住
      if ((e.target as HTMLElement | null)?.closest?.('[data-wheel-block]')) return;
      // 滚轮也是「用户在场」：切剧/切集时唤醒悬浮层，让新一部的信息亮 3 秒
      wakeChrome();
      if (seriesPanelOpen || downloading) return;
      const now = Date.now();
      if (now - stepLock.current < 400 || Math.abs(e.deltaY) < 15) return;
      stepLock.current = now;
      stepBy(e.deltaY > 0 ? 1 : -1);
    },
    [seriesPanelOpen, downloading, stepBy, wakeChrome, stepLock],
  );

  // 触摸滑动（移动端 WebView）：上滑下一个/下滑上一个；横向位移过半是系统返回手势领地不抢；
  // 与滚轮共用冷却锁——部分 WebView 快速滑动后会补发合成 wheel 事件，共用一把锁防双跳
  const touchStartRef = useRef<{ x: number; y: number } | null>(null);
  const onStageTouchStart = useCallback((e: React.TouchEvent) => {
    const t0 = e.touches[0];
    if (t0) touchStartRef.current = { x: t0.clientX, y: t0.clientY };
  }, []);
  const onStageTouchEnd = useCallback(
    (e: React.TouchEvent) => {
      const start = touchStartRef.current;
      touchStartRef.current = null;
      const t = e.changedTouches[0];
      if (!start || !t) return;
      // 浮层/控制栏里开始的滑动是它们自己的手势
      if ((t.target as HTMLElement | null)?.closest?.('[data-wheel-block]')) return;
      const dy = t.clientY - start.y;
      const dx = t.clientX - start.x;
      if (Math.abs(dy) < SWIPE_THRESHOLD_PX || Math.abs(dy) < Math.abs(dx) * 1.5) return;
      wakeChrome();
      if (seriesPanelOpen || downloading) return;
      const now = Date.now();
      if (now - stepLock.current < 400) return;
      stepLock.current = now;
      stepBy(dy < 0 ? 1 : -1);
    },
    [seriesPanelOpen, downloading, stepBy, wakeChrome, stepLock],
  );

  // 点击画面 = 播放/暂停；控件/面板/互动栏（data-wheel-block 区）里的点击是它们自己的事
  const onStageClick = useCallback(
    (e: React.MouseEvent) => {
      if ((e.target as HTMLElement | null)?.closest?.('[data-wheel-block]')) return;
      wakeChrome();
      const video = videoRef.current;
      if (!video) return;
      if (video.paused) void video.play().catch(() => undefined);
      else video.pause();
    },
    [wakeChrome, videoRef],
  );

  // 键盘快捷键：映射可在设置页自定义（默认空格 / ←→ / ↑↓，抖音网页版同款）。
  // 依赖显式列出，避免每次渲染重绑。
  const shortcutKeys = useShortcutsStore((s) => s.keys);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const video = videoRef.current;
      if (!video) return;
      // 输入框与富文本输入（contentEditable）里打字不劫持按键：←→/空格/↑↓ 是移动光标与输入的一部分
      const el = e.target as HTMLElement | null;
      if (el && (['INPUT', 'TEXTAREA', 'SELECT'].includes(el.tagName) || el.isContentEditable)) {
        return;
      }

      // Esc 固定管「退出选中」——面板语义按键，不随自定义映射
      if (e.key === 'Escape') {
        // 顺序：先退竖屏铺满回竖条，再退无边框回常规窗口（面板开着时 Esc 仍归面板）
        const ui = useUiStore.getState();
        if (
          ui.portraitFull &&
          !seriesPanelOpen &&
          !commentPanelOpen &&
          !danmakuPanelOpen &&
          !volumeOpen
        ) {
          ui.setPortraitFull(false);
          void appApi
            .setPortraitFull(portraitAspectOf(videoRef.current), false)
            .catch(() => undefined);
          return;
        }
        // 无边框播放先退回常规窗口（红绿灯显回）；面板开着时 Esc 仍归面板
        if (
          ui.playerFrameless &&
          !seriesPanelOpen &&
          !commentPanelOpen &&
          !danmakuPanelOpen &&
          !volumeOpen
        ) {
          ui.setPlayerFrameless(false);
          // 与窄屏「全屏观看」成对：Esc 退出时一并解开 Android 横屏锁
          setAndroidLandscape(false);
          void appApi.setTrafficLightsVisible(true).catch(() => undefined);
          return;
        }
        // 面板开着时 Esc 只管关面板；都没开且选中态时 Esc = 退出选中（滚轮/↑↓ 回到换剧）
        if (inBinge && !seriesPanelOpen && !commentPanelOpen && !danmakuPanelOpen && !volumeOpen) {
          setBinge(null);
        }
        return;
      }

      const step = (dir: 1 | -1) => {
        e.preventDefault();
        wakeChrome();
        // ↑↓ 语义与滚轮/触摸滑动同源：未选定剧 = 切剧，选定剧 = 切集
        stepBy(dir);
      };

      if (e.key === shortcutKeys.playPause) {
        e.preventDefault();
        // 空格没有 pointerdown/mousemove 这类天然唤醒路径，须显式算「用户在场」
        wakeChrome();
        if (video.paused) void video.play();
        else video.pause();
      } else if (e.key === shortcutKeys.seekBack) {
        e.preventDefault();
        wakeChrome();
        video.currentTime = Math.max(0, video.currentTime - 5);
      } else if (e.key === shortcutKeys.seekForward) {
        e.preventDefault();
        wakeChrome();
        // metadata 未加载时 duration 是 NaN，赋 NaN 会抛 TypeError——没有时长就不钳制右边界
        video.currentTime = Number.isFinite(video.duration)
          ? Math.min(video.duration, video.currentTime + 5)
          : video.currentTime + 5;
      } else if (e.key === shortcutKeys.stepNext) {
        step(1);
      } else if (e.key === shortcutKeys.stepPrev) {
        step(-1);
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
    // ref/setState 形参按仓库惯例写进依赖（身份恒定，见 use-playback-progress）
  }, [
    shortcutKeys,
    stepBy,
    wakeChrome,
    setBinge,
    inBinge,
    seriesPanelOpen,
    commentPanelOpen,
    danmakuPanelOpen,
    volumeOpen,
    videoRef,
  ]);

  return { onStageWheel, onStageTouchStart, onStageTouchEnd, onStageClick };
}
