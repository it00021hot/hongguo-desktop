/** MediaError 分类：播放失败恢复策略的输入（裁决见 recovery-policy）。 */

export type MediaErrorKind = 'decode' | 'network' | 'other';

/**
 * 按 MediaError.code 分类（MDN 语义）：3/4 → decode（确定性失败与瞬时管线故障都落这，
 * 区分两者的重试判别在 recovery-policy）；2 → network（断供换新流可自愈）；其余 → other。
 */
export function classifyMediaError(code: number | null | undefined): MediaErrorKind {
  if (code === 3 || code === 4) return 'decode';
  if (code === 2) return 'network';
  return 'other';
}
