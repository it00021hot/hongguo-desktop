/** 与 Rust `HONGGUO_OFFLINE_QA=1` 同一开关。未设置时是空串，正式行为不变。 */
export function offlineQa(): boolean {
  return import.meta.env.VITE_HONGGUO_OFFLINE_QA === '1';
}
