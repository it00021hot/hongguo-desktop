import { useCallback, useEffect, useRef } from 'react';

/**
 * 舞台三套交互裁决：滚轮切换、点击画面、键盘快捷键。
 *
 * 滚轮/↑↓ 的语义由宿主注入的 `onWheelStep` 与连播锁定共同决定：
 * 信息流（沉浸流）= 上一部/下一部剧，从详情/历史等**选定**剧进来
 * （或锁定连播后）= 切上一集/下一集。
 */
export function usePlayerInteractions(params: {
  videoRef: React.RefObject<HTMLVideoElement | null>;
  seriesId: string | null;
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
    seriesId,
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

  // ---- 滚轮切换（hgplayer 同款）：沉浸流=上一部/下一部剧，播放页=切集 ----
  // 选集浮层/下载面板打开时不抢滚动；冷却 400ms 防一次惯性滚动连跳。
  const wheelLock = useRef(0);
  const onStageWheel = useCallback(
    (e: React.WheelEvent) => {
      // 浮层（评论面板/选集/弹幕设置/倍速清晰度菜单）里的滚动是它自己在滚，
      // 不冒泡成「切集」。控制栏与面板用 data-wheel-block 标记；Radix 菜单
      // portal 到 body，真实 DOM 里不是舞台子孙，必须各自带标记才拦得住。
      if ((e.target as HTMLElement | null)?.closest?.('[data-wheel-block]')) return;
      // 滚轮也是「用户在场」：切剧/切集时唤醒悬浮层，让新一部的信息亮 3 秒
      wakeChrome();
      if (seriesPanelOpen || downloading) return;
      const now = Date.now();
      if (now - wheelLock.current < 400 || Math.abs(e.deltaY) < 15) return;
      wheelLock.current = now;
      const dir: 1 | -1 = e.deltaY > 0 ? 1 : -1;
      setSlideDir(dir);
      // 连播锁定时滚轮语义变为切集，不跟随宿主页换剧
      if (inBinge) stepEpisode(dir);
      else if (onWheelStep) onWheelStep(dir);
      else stepEpisode(dir);
    },
    [
      seriesPanelOpen,
      downloading,
      onWheelStep,
      stepEpisode,
      wakeChrome,
      inBinge,
      setSlideDir,
      wheelLock,
    ],
  );

  // ---- 点击画面：信息流里=选中本剧（进入切集模式）；选中后/播放页=播放/暂停 ----
  // 第三方同款交互：未选中时单击视频=「选中这部剧」，此后滚轮/↑↓ 切集，
  // Esc 退出选中回到换剧；选中状态下的单击回归传统的播放/暂停。
  // 控件/面板/互动栏（data-wheel-block 标记区）里的点击是它们自己的事，
  // 不冒泡成选中/暂停。
  const onStageClick = useCallback(
    (e: React.MouseEvent) => {
      if ((e.target as HTMLElement | null)?.closest?.('[data-wheel-block]')) return;
      wakeChrome();
      if (onWheelStep && !inBinge) {
        setBinge(seriesId);
        return;
      }
      const video = videoRef.current;
      if (!video) return;
      if (video.paused) void video.play().catch(() => undefined);
      else video.pause();
    },
    [wakeChrome, onWheelStep, inBinge, setBinge, seriesId, videoRef],
  );

  // 键盘快捷键：空格 / ←→ / ↑↓。依赖显式列出，避免每次渲染重绑。
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const video = videoRef.current;
      if (!video) return;
      // 输入框里打字时不劫持按键：表单控件之外还要算上富文本输入
      // （弹幕/评论输入是 contentEditable 的 div，←→/空格/↑↓ 是移动
      // 光标和输入的一部分，不是快进快退/播放暂停/切集）
      const el = e.target as HTMLElement | null;
      if (el && (['INPUT', 'TEXTAREA', 'SELECT'].includes(el.tagName) || el.isContentEditable)) {
        return;
      }

      switch (e.key) {
        case 'Escape':
          // 面板（评论/选集/弹幕设置）开着时它们的 Esc 只管关面板；
          // 都没开而处于选中态时，Esc = 退出选中（滚轮/↑↓ 回到换剧）
          if (
            inBinge &&
            !seriesPanelOpen &&
            !commentPanelOpen &&
            !danmakuPanelOpen &&
            !volumeOpen
          ) {
            setBinge(null);
          }
          break;
        case ' ':
          e.preventDefault();
          if (video.paused) void video.play();
          else video.pause();
          break;
        case 'ArrowLeft':
          e.preventDefault();
          wakeChrome();
          video.currentTime = Math.max(0, video.currentTime - 5);
          break;
        case 'ArrowRight':
          e.preventDefault();
          wakeChrome();
          // metadata 未加载时 duration 是 NaN，WebIDL 对 currentTime 赋
          // NaN 会抛 TypeError——没有时长就先不钳制右边界
          video.currentTime = Number.isFinite(video.duration)
            ? Math.min(video.duration, video.currentTime + 5)
            : video.currentTime + 5;
          break;
        case 'ArrowUp':
        case 'ArrowDown': {
          e.preventDefault();
          wakeChrome();
          // ↑↓ 的语义与滚轮同源：沉浸流（未选定剧）= 切上一部/下一部剧，
          // 从详情/历史等**选定**剧进来 = 切上一集/下一集
          const dir: 1 | -1 = e.key === 'ArrowDown' ? 1 : -1;
          setSlideDir(dir);
          if (inBinge) stepEpisode(dir);
          else if (onWheelStep) onWheelStep(dir);
          else stepEpisode(dir);
          break;
        }
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
    // ref/setState 形参按仓库惯例写进依赖（身份恒定，见 use-playback-progress）
  }, [
    stepEpisode,
    wakeChrome,
    onWheelStep,
    inBinge,
    setBinge,
    seriesPanelOpen,
    commentPanelOpen,
    danmakuPanelOpen,
    volumeOpen,
    videoRef,
    setSlideDir,
  ]);

  return { onStageWheel, onStageClick };
}
