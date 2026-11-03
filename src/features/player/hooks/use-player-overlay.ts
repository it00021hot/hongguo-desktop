import { useCallback, useEffect, useState } from 'react';

/** 播放器悬浮层（信息/互动栏）静止多久后淡出。与控制栏的 3 秒同款。 */
const CHROME_HIDE_MS = 3_000;

/**
 * 沉浸流悬浮层显隐状态机：画面内移动/点击显示、移出立即隐藏、静止 3 秒隐藏；暂停与控件悬停常驻。
 * 规则走舞台根容器的 React 事件——effect + addEventListener 在「先挂载后到位」的信息流里绑定会永久缺席。
 */
export function usePlayerOverlay(params: {
  seriesPanelOpen: boolean;
  commentPanelOpen: boolean;
  danmakuPanelOpen: boolean;
  volumeOpen: boolean;
  /** 窄屏倍速/清晰度底部 Sheet 打开时，悬浮层常显且不被静默期关掉 */
  phoneMoreOpen?: boolean;
  /** 窄屏选集 / 增强 Sheet */
  phoneSeriesOpen?: boolean;
  phoneEnhanceOpen?: boolean;
  /** 窄屏发弹幕底栏打开时，悬浮层常显 */
  phoneDanmakuSendOpen?: boolean;
  /**
   * 挂载时悬浮层是否点亮。自动连播切集传 false：静默起步；paused 初值同步视同
   * 「在播」，否则 paused=true 的常显规则会让操作栏照样亮出来（「自动下一集抢占鼠标」的来源）。
   */
  startVisible?: boolean;
  /** 静默期硬开关（自动连播打点 → 新集首播/用户真实输入之间）：true 时 chromeShown 直接判 false，显式打开的面板不受影响。 */
  silenced?: boolean;
}) {
  const {
    seriesPanelOpen,
    commentPanelOpen,
    danmakuPanelOpen,
    volumeOpen,
    phoneMoreOpen = false,
    phoneSeriesOpen = false,
    phoneEnhanceOpen = false,
    phoneDanmakuSendOpen = false,
    startVisible = true,
    silenced = false,
  } = params;
  const [paused, setPaused] = useState(!startVisible);
  const [chromeVisible, setChromeVisible] = useState(startVisible);
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
  // 隐藏倒计时：播放中静止 3 秒即收；暂停 / 指针悬在控制栏上时常显不倒计时
  useEffect(() => {
    if (paused || controlsHovered) return;
    const timer = setTimeout(() => setChromeVisible(false), CHROME_HIDE_MS);
    return () => clearTimeout(timer);
  }, [paused, controlsHovered, chromeTick]);

  // 悬浮层整体可见性：任一面板打开或暂停时常显，其余由上面的倒计时裁决；
  // 简介/互动栏/控制栏全部吃这一个值（控制栏弹出时简介同步抬升也靠它）
  const panelOpen =
    seriesPanelOpen ||
    commentPanelOpen ||
    danmakuPanelOpen ||
    volumeOpen ||
    phoneMoreOpen ||
    phoneSeriesOpen ||
    phoneEnhanceOpen ||
    phoneDanmakuSendOpen;
  const chromeShown = silenced
    ? panelOpen
    : paused || chromeVisible || controlsHovered || panelOpen;

  return { paused, setPaused, chromeShown, wakeChrome, hideChrome, setControlsHovered };
}
