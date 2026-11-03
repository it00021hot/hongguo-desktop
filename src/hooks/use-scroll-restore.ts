import { useEffect, useRef, useState } from 'react';

/** 浏览位置保留（找剧/榜单/新剧去播放再回来）：会话级 sessionStorage 承载，重开应用不复旧位。 */

/** 挂载时读 sessionStorage 的 useState；变更即时写回（支持函数式更新）。 */
export function useSessionState<T>(
  key: string,
  initial: T,
): [T, (v: T | ((prev: T) => T)) => void] {
  const [value, setValue] = useState<T>(() => {
    try {
      const raw = sessionStorage.getItem(key);
      return raw === null ? initial : (JSON.parse(raw) as T);
    } catch {
      return initial;
    }
  });
  const set = (v: T | ((prev: T) => T)) => {
    setValue((prev) => {
      const next = typeof v === 'function' ? (v as (p: T) => T)(prev) : v;
      try {
        sessionStorage.setItem(key, JSON.stringify(next));
      } catch {
        // 配额/隐私模式失败不打扰功能，仅本会话不记忆
      }
      return next;
    });
  };
  return [value, set];
}

/**
 * 滚动位置记忆：卸载时把 `getEl()` 的 scrollTop 存 sessionStorage，`ready` 翻真后一次性恢复；
 * 用户没滚动过不覆盖已存位置（避免 StrictMode 重挂载把存档清零）。列表页滚动容器各异，所以收 getter。
 */
export function useScrollRestore(key: string, getEl: () => HTMLElement | null, ready: boolean) {
  const scrolledRef = useRef(false);
  const restoredRef = useRef(false);

  // 恢复：ready 后等一帧（列表行已挂载）再设 scrollTop
  useEffect(() => {
    if (!ready || restoredRef.current) return;
    restoredRef.current = true;
    const saved = Number(sessionStorage.getItem(key) ?? 0);
    if (!(saved > 0)) return;
    requestAnimationFrame(() => {
      const el = getEl();
      if (el && saved <= el.scrollHeight) {
        el.scrollTop = saved;
        scrolledRef.current = true;
      }
    });
    // getEl 是稳定闭包（ref getter），不进依赖
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ready, key]);

  // 记录「用户滚动过」：只听滚动、不存值（存值在卸载时一次性做）
  useEffect(() => {
    const onScroll = () => {
      const el = getEl();
      if (el && el.scrollTop > 0) scrolledRef.current = true;
    };
    // 捕获阶段监听 document：内部容器与 #content 都能命中
    document.addEventListener('scroll', onScroll, true);
    return () => {
      document.removeEventListener('scroll', onScroll, true);
      if (scrolledRef.current) {
        const el = getEl();
        if (el) {
          try {
            sessionStorage.setItem(key, String(el.scrollTop));
          } catch {
            // 同上：存不进就算了
          }
        }
      }
    };
    // getEl 稳定；key 变化（换 tab 重挂）时以新 key 存取
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);
}
