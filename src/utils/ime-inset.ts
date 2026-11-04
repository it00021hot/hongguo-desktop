/**
 * Android IME 底边高度（CSS px）：MainActivity 经 `window.__hgSetImeBottom` 推送。
 * 桌面 / 无桥环境恒为 0。发弹幕输入框用它做 `position:fixed; bottom`，不依赖
 * 不可靠的 visualViewport（华为等 OEM WebView 开键盘时常不缩）。
 */
import { useEffect, useState } from 'react';

type Listener = (bottomCssPx: number) => void;

const listeners = new Set<Listener>();
let current = 0;

declare global {
  interface Window {
    __hgSetImeBottom?: (bottomCssPx: number) => void;
  }
}

/** 启动时挂一次：原生 insets 回调进这里。 */
export function installImeInsetBridge(): void {
  window.__hgSetImeBottom = (bottomCssPx: number) => {
    const next = Number.isFinite(bottomCssPx) ? Math.max(0, bottomCssPx) : 0;
    if (next === current) return;
    current = next;
    for (const listener of listeners) listener(current);
  };
}

/** 当前键盘相对 WebView 底边的抬升量（CSS px）。 */
export function useImeBottom(): number {
  const [bottom, setBottom] = useState(current);
  useEffect(() => {
    const onChange: Listener = (value) => setBottom(value);
    listeners.add(onChange);
    setBottom(current);
    return () => {
      listeners.delete(onChange);
    };
  }, []);
  return bottom;
}
