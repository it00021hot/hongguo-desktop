import { useEffect, useState } from 'react';

/**
 * 订阅 `window.matchMedia`。SSR / 首帧无 window 时回落 `false`，
 * 挂载后再对齐真实匹配（避免 hydration 分叉）。
 */
export function useMatchMedia(query: string): boolean {
  const [matches, setMatches] = useState(false);

  useEffect(() => {
    const mql = window.matchMedia(query);
    const sync = () => setMatches(mql.matches);
    sync();
    mql.addEventListener('change', sync);
    return () => mql.removeEventListener('change', sync);
  }, [query]);

  return matches;
}

/** 与 Tailwind `md`（768px）对齐：低于此宽度视为手机端 / 窄舞台。 */
export function useNarrowScreen(): boolean {
  return useMatchMedia('(max-width: 767px)');
}
