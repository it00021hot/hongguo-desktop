import { create } from 'zustand';
import { locale, setLocale, type Locale } from '@/locales';

interface LocaleState {
  locale: Locale;
  set: (l: Locale) => void;
}

/** 语言状态：让 React 感知语言变化重渲染，实际取值仍在 `i18n`（locales/index）。 */
export const useLocaleStore = create<LocaleState>((set) => ({
  locale: locale(),
  set: (l) => {
    setLocale(l);
    set({ locale: l });
  },
}));
