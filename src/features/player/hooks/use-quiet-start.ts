import { useCallback, useLayoutEffect, useRef, useState } from 'react';
import { isAutoAdvanceStart } from '../playback-signals';

/** 自动连播静默硬上限：主闸门是「首播之前不唤醒」，这只兜起播失败的死局。 */
const QUIET_WINDOW_MS = 8_000;

/**
 * 自动连播静默起步：handleEnded 切集打了点则挂载以「无人操作」起步；
 * 首播前移动类事件不唤醒悬浮层（幻影 mousemove）。
 */
export function useQuietStart() {
  const [quietStart] = useState(isAutoAdvanceStart());
  /** 静默期开关（喂给 usePlayerOverlay 的 silenced，一票否决） */
  const [quietArmed, setQuietArmed] = useState(quietStart);
  /**
   * 静默窗硬上限兜底：布防放 useLayoutEffect，先于浏览器派发幻影事件的任务。
   */
  const quietUntilRef = useRef(0);
  useLayoutEffect(() => {
    if (quietStart) quietUntilRef.current = Date.now() + QUIET_WINDOW_MS;
  }, [quietStart]);
  /**
   * 本集是否已真正播起来过：首播前的 pause 是新元素装载噪声不算暂停；
   * 归零点在 handleEnded（同步、事件处理器内）。
   */
  const playedOnceRef = useRef(false);

  /** 自动连播切集时续上静默窗 + playedOnce 同步归零（PlayerView 复用不重挂载）。 */
  const armQuiet = useCallback(() => {
    playedOnceRef.current = false;
    quietUntilRef.current = Date.now() + QUIET_WINDOW_MS;
    setQuietArmed(true);
  }, []);

  /** 首播出画：解除静默。 */
  const markPlayed = useCallback(() => {
    playedOnceRef.current = true;
    setQuietArmed(false);
  }, []);

  /** 静默期：首播前移动不唤醒；超时后照常。 */
  const wakeOnMove = useCallback((wakeChrome: () => void) => {
    if (playedOnceRef.current || Date.now() >= quietUntilRef.current) wakeChrome();
  }, []);

  /** 点击/滚轮/按键是铁证：解除静默并唤醒。 */
  const wakeOnInput = useCallback(
    (wakeChrome: () => void) => {
      setQuietArmed(false);
      wakeChrome();
    },
    [],
  );

  return {
    quietStart,
    quietArmed,
    playedOnceRef,
    armQuiet,
    markPlayed,
    wakeOnMove,
    wakeOnInput,
  };
}
