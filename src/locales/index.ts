import zhCN from './zh-CN.json';
import enUS from './en-US.json';

/** 资源结构由中文版决定，英文版必须与之完全一致。 */
type Dict = typeof zhCN;

const resources: Record<string, Dict> = {
  'zh-CN': zhCN,
  'en-US': enUS,
};

export type Locale = keyof typeof resources;

/**
 * 词条路径类型：由中文资源结构递归推导的 `a.b.c` 字面量联合，
 * t() 调用点全部获得编译期检查（key 拼错/词条改名漏改 tsc 当场报）。
 */
export type MessageKey = Paths<typeof zhCN>;

type Paths<T> = {
  [K in keyof T & string]: T[K] extends Record<string, unknown> ? K | `${K}.${Paths<T[K]>}` : K;
}[keyof T & string];

const STORAGE_KEY = 'hongguo-locale';

function detect(): Locale {
  const saved = localStorage.getItem(STORAGE_KEY);
  if (saved && saved in resources) return saved;
  return navigator.language.toLowerCase().startsWith('zh') ? 'zh-CN' : 'en-US';
}

let current: Locale = detect();

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
export function t(path: MessageKey): string {
  const value = path.split('.').reduce<unknown>((acc, key) => {
    if (acc && typeof acc === 'object' && key in acc) {
      return (acc as Record<string, unknown>)[key];
    }
    return undefined;
  }, resources[current]);

  if (typeof value === 'string') return value;
  // 缺词条回落 key 本身；dev 下额外告警——用户看到 `feed.loadFailed` 原文属于事故，不能只靠回落静默
  if (import.meta.env.DEV) {
    console.warn(`[i18n] 缺词条: ${path}（当前语言 ${current}）`);
  }
  return path;
}

/** 取词条并做变量插值：`{count}` 会被替换。 */
export function tf(path: MessageKey, vars: Record<string, string | number>): string {
  return t(path).replace(/\{(\w+)\}/g, (_, key: string) =>
    key in vars ? String(vars[key]) : `{${key}}`,
  );
}

document.documentElement.lang = current;
