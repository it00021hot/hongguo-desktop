import { useCallback } from 'react';
import { useUiStore } from '@/stores/ui';
import { app as appApi } from '@/service/commands';

/**
 * 窗口置顶开关（hgplayer De.pinned）：窗口级状态，大小屏共用同一个
 * 开关——大屏顶栏与小屏紧凑条两个入口，切换的是同一个 set_always_on_top。
 *
 * 先调后端再落 store：后端失败时前端不同步翻转，两边不会各说各话。
 */
export function usePinWindow() {
  const pinned = useUiStore((s) => s.pinned);
  const setPinned = useUiStore((s) => s.setPinned);
  const togglePinned = useCallback(() => {
    const next = !pinned;
    void appApi
      .setAlwaysOnTop(next)
      .then(() => setPinned(next))
      .catch(() => undefined);
  }, [pinned, setPinned]);
  return { pinned, togglePinned };
}
