import { useCallback, useEffect, useRef } from 'react';
import type { MouseEvent as ReactMouseEvent, PointerEvent as ReactPointerEvent } from 'react';

/**
 * 长按阈值：官方 APK 的 `CollectEmoticonHelper` 用的是 500ms 定时器
 * （按下起表，超时未抬手即判定长按），本仓同款。
 */
const LONG_PRESS_DELAY_MS = 500;

/** 长按落点（视口坐标，菜单就地弹在指针旁）。 */
export interface LongPressPoint {
  x: number;
  y: number;
}

export interface LongPressHandlers {
  onPointerDown: (e: ReactPointerEvent) => void;
  onPointerUp: () => void;
  onPointerCancel: () => void;
  onPointerLeave: () => void;
  onClickCapture: (e: ReactMouseEvent) => void;
}

/**
 * 长按识别（官方同款 500ms 阈值）。
 *
 * 长按触发后要吞掉紧随其后的 click——表情格既有点击（插入草稿）又有
 * 长按（收藏），不吞会长按之后又把表情插进草稿。
 */
export function useLongPress(onLongPress: (point: LongPressPoint) => void): LongPressHandlers {
  const timer = useRef<number | null>(null);
  const fired = useRef(false);
  const point = useRef<LongPressPoint>({ x: 0, y: 0 });

  const clear = useCallback(() => {
    if (timer.current !== null) {
      window.clearTimeout(timer.current);
      timer.current = null;
    }
  }, []);

  const onPointerDown = useCallback(
    (e: ReactPointerEvent) => {
      point.current = { x: e.clientX, y: e.clientY };
      fired.current = false;
      clear();
      timer.current = window.setTimeout(() => {
        fired.current = true;
        onLongPress(point.current);
      }, LONG_PRESS_DELAY_MS);
    },
    [clear, onLongPress],
  );

  const release = useCallback(() => {
    clear();
  }, [clear]);

  const onClickCapture = useCallback((e: ReactMouseEvent) => {
    if (!fired.current) return;
    e.preventDefault();
    e.stopPropagation();
    fired.current = false;
  }, []);

  useEffect(() => clear, [clear]);

  return {
    onPointerDown,
    onPointerUp: release,
    onPointerCancel: release,
    onPointerLeave: release,
    onClickCapture,
  };
}
