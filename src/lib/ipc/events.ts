import { useEffect } from 'react';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type { EventName } from './types';

/**
 * 订阅后端事件。
 *
 * 返回取消函数，由 `useEvent` 在 effect 清理时调用——外部不该直接用它，
 * 那样很容易漏掉退订。
 */
async function on<T>(name: EventName, handler: (payload: T) => void): Promise<UnlistenFn> {
  return listen<T>(name, (event) => handler(event.payload));
}

/**
 * 订阅并在组件卸载时自动清理。
 *
 * `handler` 请用 `useCallback` 包裹，否则每次渲染都会退订重订。
 */
export function useEvent<T>(name: EventName, handler: (payload: T) => void): void {
  useEffect(() => {
    let unlisten: UnlistenFn | undefined;
    let cancelled = false;

    on<T>(name, handler).then((fn) => {
      if (cancelled) {
        fn();
      } else {
        unlisten = fn;
      }
    });

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [name, handler]);
}
