import zhCN from './zh-CN.json';
import enUS from './en-US.json';

/** 资源结构由中文版决定，英文版必须与之完全一致。 */
type Dict = typeof zhCN;

const resources: Record<string, Dict> = {
  'zh-CN': zhCN,
  'en-US': enUS as Dict,
};

export type Locale = keyof typeof resources;

const STORAGE_KEY = 'hongguo-locale';

function detect(): Locale {
  const saved = localStorage.getItem(STORAGE_KEY);
  if (saved && saved in resources) return saved as Locale;
  return navigator.language.toLowerCase().startsWith('zh') ? 'zh-CN' : 'en-US';
}

let current: Locale = detect();

/** 当前语言。 */
export function locale(): Locale {
  return current;
}

/** 切换语言并持久化。 */
export function setLocale(next: Locale): void {
  current = next;
  localStorage.setItem(STORAGE_KEY, next);
  document.documentElement.lang = next;
}

/** 取词条；`a.b.c` 形式查嵌套对象。 */
export function t(path: string): string {
  const value = path.split('.').reduce<unknown>((acc, key) => {
    if (acc && typeof acc === 'object' && key in acc) {
      return (acc as Record<string, unknown>)[key];
    }
    return undefined;
  }, resources[current]);

  if (typeof value === 'string') return value;
  // 缺词条时回落到 key 本身，比显示 undefined 更容易定位问题
  return path;
}

/** 取词条并做变量插值：`{count}` 会被替换。 */
export function tf(path: string, vars: Record<string, string | number>): string {
  return t(path).replace(/\{(\w+)\}/g, (_, key: string) =>
    key in vars ? String(vars[key]) : `{${key}}`,
  );
}

// 初始化文档语言
document.documentElement.lang = current;
