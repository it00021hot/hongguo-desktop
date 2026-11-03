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

/**
 * 与 Tailwind `md`（768px）对齐：短边也算窄。
 * 手机横屏后 CSS 宽常 >768，若不看高度会掉进桌面控制栏、丢「退出全屏」。
 */
export function useNarrowScreen(): boolean {
  return useMatchMedia('(max-width: 767px), (orientation: landscape) and (max-height: 500px)');
}
