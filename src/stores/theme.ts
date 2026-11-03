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

/** 亮/暗主题：偏好落 localStorage（persist 中间件，避免手写读写各处不同步）。 */
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

export function applyTheme(theme: Theme): void {
  const resolved: 'light' | 'dark' = theme === 'auto' ? systemTheme() : theme;
  document.documentElement.classList.toggle('dark', resolved === 'dark');
}
