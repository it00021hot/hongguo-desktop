/** 增长中的兼容成片：一次 206 的结束只是这一段字节到了，不是整片结束。 */

export type GrowStep =
  | { kind: 'data'; nextOffset: number; finished: boolean }
  | { kind: 'finished' }
  | { kind: 'cancelled' }
  | { kind: 'failed' };

export function classifyGrow(
  status: number,
  signal: string | null,
  byteLength: number,
  offset: number,
): GrowStep {
  if (status === 409 || signal === 'cancelled') return { kind: 'cancelled' };
  if (status === 500 || signal === 'failed') return { kind: 'failed' };
  if (status === 204 || (signal === 'done' && byteLength === 0)) return { kind: 'finished' };
  if (status === 206 && byteLength > 0) {
    return {
      kind: 'data',
      nextOffset: offset + byteLength,
      finished: signal === 'done',
    };
  }
  return { kind: 'failed' };
}
