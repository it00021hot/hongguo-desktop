import { create } from 'zustand';
import { persist } from 'zustand/middleware';

export type Theme = 'auto' | 'light' | 'dark';

interface ThemeState {
  theme: Theme;
  setTheme: (t: Theme) => void;
  /** 实际生效的主题（auto 时按系统偏好解析）。 */
  resolved: () => 'light' | 'dark';
}

function systemTheme(): 'light' | 'dark' {
  return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
}

/**
 * 亮/暗主题。
 *
 * 桌面应用没有 cookie，偏好只能落在 localStorage——用 zustand 的 persist
 * 中间件而不是手写 localStorage 读写，避免各处忘记同步。
 */
export const useThemeStore = create<ThemeState>()(
  persist(
    (set, get) => ({
      theme: 'auto',
      setTheme: (theme) => {
        set({ theme });
        applyTheme(theme);
      },
      resolved: (): 'light' | 'dark' => {
        const current = get().theme;
        return current === 'auto' ? systemTheme() : current;
      },
    }),
    {
      name: 'hongguo-theme',
      // 只持久化用户选择，resolved 是派生值
      partialize: (s) => ({ theme: s.theme }),
      onRehydrateStorage: () => (state) => {
        if (state) applyTheme(state.theme);
      },
    },
  ),
);

/** 把主题写到 `<html class="dark">` 上。 */
export function applyTheme(theme: Theme): void {
  const resolved: 'light' | 'dark' = theme === 'auto' ? systemTheme() : theme;
  document.documentElement.classList.toggle('dark', resolved === 'dark');
}
