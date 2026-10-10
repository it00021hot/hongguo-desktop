import { create } from 'zustand';
import { persist } from 'zustand/middleware';

/** 播放器可自定义的动作（对齐 hgplayer：默认与抖音网页版一致，
 *  快进快退默认 ← →） */
export type ShortcutAction = 'playPause' | 'seekBack' | 'seekForward' | 'stepPrev' | 'stepNext';

export const DEFAULT_SHORTCUTS: Record<ShortcutAction, string> = {
  playPause: ' ',
  seekBack: 'ArrowLeft',
  seekForward: 'ArrowRight',
  stepPrev: 'ArrowUp',
  stepNext: 'ArrowDown',
};

interface ShortcutsState {
  keys: Record<ShortcutAction, string>;
  /** 捕获到新按键时写入；不查重，重复键在设置面板里当场拦截 */
  setKey: (action: ShortcutAction, key: string) => void;
  reset: () => void;
}

/** 纯 UI 偏好：按键映射。与播放解耦，播放器 hook 从这里读。 */
export const useShortcutsStore = create<ShortcutsState>()(
  persist(
    (set, get) => ({
      keys: { ...DEFAULT_SHORTCUTS },
      setKey: (action, key) => set({ keys: { ...get().keys, [action]: key } }),
      reset: () => set({ keys: { ...DEFAULT_SHORTCUTS } }),
    }),
    {
      name: 'hongguo-shortcuts',
      partialize: (s) => ({ keys: s.keys }),
      // 旧持久化缺新动作时回退默认键，避免 undefined 匹配不上任何键
      merge: (persisted, current) => ({
        ...current,
        keys: { ...DEFAULT_SHORTCUTS, ...(persisted as Partial<ShortcutsState>).keys },
      }),
    },
  ),
);

/** e.key 值转可读键名（设置面板展示用） */
export function shortcutKeyLabel(key: string, spaceLabel: string): string {
  switch (key) {
    case ' ':
      return spaceLabel;
    case 'ArrowLeft':
      return '←';
    case 'ArrowRight':
      return '→';
    case 'ArrowUp':
      return '↑';
    case 'ArrowDown':
      return '↓';
    case 'Escape':
      return 'Esc';
    default:
      return key.length === 1 ? key.toUpperCase() : key;
  }
}
