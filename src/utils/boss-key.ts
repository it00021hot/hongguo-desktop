/** 老板键组合的纯函数（无模块级副作用，可被测试直接引用）。全局组合在应用内按下时
 *  keydown 的 key 就是末段裸键，与播放器快捷键按「末段裸键 vs 播放器键」比对冲突。 */

/** 老板键组合 → 末段裸键名（小写）。空串/残串返回空。 */
export function bossKeyCoreKey(bossKey: string): string {
  const parts = bossKey
    .split('+')
    .map((p) => p.trim().toLowerCase())
    .filter(Boolean);
  return parts[parts.length - 1] ?? '';
}

export interface BossKeyConflict {
  /** 冲突的动作标识（调用方的领域键，如 ShortcutAction） */
  action: string;
  /** 冲突的播放器键（可读形态） */
  label: string;
}

/** 老板键 vs 播放器快捷键冲突检测；`actions` 由调用方从自己的快捷键映射展开，本函数保持无依赖。 */
export function bossKeyConflicts(
  bossKey: string,
  actions: { action: string; label: string; key: string }[],
): BossKeyConflict[] {
  const core = bossKeyCoreKey(bossKey);
  if (!core) return [];
  return actions
    .filter((a) => a.key.trim().toLowerCase() === core)
    .map(({ action, label }) => ({ action, label }));
}

export function bossKeyFromEvent(
  event: Pick<KeyboardEvent, 'ctrlKey' | 'metaKey' | 'altKey' | 'shiftKey' | 'code' | 'key'>,
): string | null {
  const unmodified = !event.ctrlKey && !event.metaKey && !event.altKey && !event.shiftKey;
  if (event.key === 'Backspace' && unmodified) return '';
  if (['Control', 'Shift', 'Alt', 'Meta'].includes(event.key)) return null;
  if (!event.ctrlKey && !event.metaKey && !event.altKey) return null;
  const parts: string[] = [];
  if (event.ctrlKey || event.metaKey) parts.push('CommandOrControl');
  if (event.altKey) parts.push('Alt');
  if (event.shiftKey) parts.push('Shift');
  let key = event.code;
  if (key.startsWith('Key')) key = key.slice(3);
  else if (key.startsWith('Digit')) key = key.slice(5);
  else if (key.startsWith('Arrow')) key = key.slice(5);
  else if (key.startsWith('Numpad')) key = 'Num' + key.slice(6);
  if (!key) return null;
  parts.push(key);
  return parts.join('+');
}
