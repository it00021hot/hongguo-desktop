import { useCallback, useEffect, useState } from 'react';

/** 播放器悬浮层（信息/互动栏）静止多久后淡出。与控制栏的 3 秒同款。 */
const CHROME_HIDE_MS = 3_000;

/**
 * 沉浸流悬浮层显隐状态机（B站方案）。
 *
 * 在画面内移动 → 显示；移出画面 → 立即隐藏；移入画面 → 显示；
 * 在画面内静止超 3 秒 → 隐藏。点击（含控制栏按钮）同样算「在场」，
 * 重启 3 秒倒计时后自动隐藏。
 * 四条规则全走舞台 div 上的 React 事件（onMouseEnter/Move/Leave/PointerDown）。
 * 不能用 effect + addEventListener：信息流是**同一个 PlayerView 先挂载、
 * 播放目标后到位**的（首屏 store 空 → 先渲染中性空态，舞台还不存在），
 * effect 挂载后读 stageRef.current 拿到 null 就直接返回，依赖又不再变，
 * 绑定永久缺席——表现就是沉浸流里鼠标怎么动控制栏都不出来（播放页挂载时
 * 已有目标所以是好的）。React 事件挂在根容器上，舞台什么时候出现都接得住。
 * 两个 B站同款例外：暂停态控制栏常驻（暂停就是用来看进度条的）；
 * 指针悬在控制栏本体上不倒计时（音量/进度条拖动中不许收）。
 * 暂停状态跟 <video> 走（onPlay/onPause），悬浮层的「常显」语义在这里统一裁决。
 */
export function usePlayerOverlay(params: {
  seriesPanelOpen: boolean;
  commentPanelOpen: boolean;
  danmakuPanelOpen: boolean;
  volumeOpen: boolean;
}) {
  const { seriesPanelOpen, commentPanelOpen, danmakuPanelOpen, volumeOpen } = params;
  const [paused, setPaused] = useState(true);
  const [chromeVisible, setChromeVisible] = useState(true);
  /** 隐藏倒计时的代际号：每次唤醒递增，倒计时 effect 随之重启 */
  const [chromeTick, setChromeTick] = useState(0);
  /** 指针悬在控制栏本体上：控件不许收（悬在控件上操作时静止超时收起=抢走） */
  const [controlsHovered, setControlsHovered] = useState(false);
  const wakeChrome = useCallback(() => {
    setChromeVisible(true);
    setChromeTick((n) => n + 1);
  }, []);
  /** 指针离开画面：立即收起，不等倒计时（B站同款）。 */
  const hideChrome = useCallback(() => setChromeVisible(false), []);
  // 隐藏倒计时：播放中静止 3 秒即收（不再因光标悬停画面而常显）；
  // 暂停 / 指针悬在控制栏上时常显不倒计时。
  useEffect(() => {
    if (paused || controlsHovered) return;
    const timer = setTimeout(() => setChromeVisible(false), CHROME_HIDE_MS);
    return () => clearTimeout(timer);
  }, [paused, controlsHovered, chromeTick]);

  // 悬浮层整体可见性：任一面板（选集/评论/弹幕设置/音量条）打开或暂停时常显，
  // 其余由上面的倒计时裁决。简介/互动栏/控制栏/顶部杂物全部吃这一个值，
  // 不再各养一套定时器——控制栏弹出时简介同步抬升也是靠它。
  const chromeShown =
    paused ||
    chromeVisible ||
    controlsHovered ||
    seriesPanelOpen ||
    commentPanelOpen ||
    danmakuPanelOpen ||
    volumeOpen;

  return { paused, setPaused, chromeShown, wakeChrome, hideChrome, setControlsHovered };
}
