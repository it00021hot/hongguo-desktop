import { create } from 'zustand';
import { locale, setLocale, type Locale } from '@/i18n';

interface LocaleState {
  locale: Locale;
  set: (l: Locale) => void;
}

/**
 * 语言状态。
 *
 * 与 `i18n` 模块的纯函数分开：这里负责让 React 能感知语言变化并重渲染，
 * 实际取值仍在 `i18n` 里。
 */
export const useLocaleStore = create<LocaleState>((set) => ({
  locale: locale(),
  set: (l) => {
    setLocale(l);
    set({ locale: l });
  },
}));
