/** 增长中的兼容成片：一次 206 的结束只是这一段字节到了，不是整片结束。 */

export type GrowStep =
  | { kind: 'data'; nextOffset: number; finished: boolean }
  | { kind: 'finished' }
  | { kind: 'cancelled' }
  | { kind: 'failed' };

/**
 * 把 grow HTTP 响应收成一步。
 * 416 Range Not Satisfiable（成片已 Done、偏移越界等）按 HTTP 语义表示没有更多字节，
 * 不能一律判 failed，否则会误触发失败回退。
 */
export function classifyGrow(
  status: number,
  signal: string | null,
  byteLength: number,
  offset: number,
): GrowStep {
  if (status === 409 || signal === 'cancelled') return { kind: 'cancelled' };
  if (status === 500 || signal === 'failed') return { kind: 'failed' };
  if (status === 204 || (signal === 'done' && byteLength === 0)) return { kind: 'finished' };
  // 合法 416：Done 后 Range 无法满足 → 已无更多可读字节，视为 finished。
  if (status === 416) return { kind: 'finished' };
  if (status === 206 && byteLength > 0) {
    return {
      kind: 'data',
      nextOffset: offset + byteLength,
      finished: signal === 'done',
    };
  }
  return { kind: 'failed' };
}

/** pull 对 grow cancelled/failed 必须通知上层，不能只靠 Failed 进度或换集。 */
export function shouldNotifyUnavailable(
  step: GrowStep,
): step is Extract<GrowStep, { kind: 'cancelled' | 'failed' }> {
  return step.kind === 'cancelled' || step.kind === 'failed';
}

export type PullDecision =
  | { action: 'unavailable' }
  | { action: 'finish' }
  | { action: 'append'; nextOffset: number; thenFinish: boolean };

/** 把 classifyGrow 的一步收成 pull 循环决策，便于离线断言接线。 */
export function decidePullAction(step: GrowStep): PullDecision {
  if (shouldNotifyUnavailable(step)) return { action: 'unavailable' };
  if (step.kind === 'finished') return { action: 'finish' };
  return {
    action: 'append',
    nextOffset: step.nextOffset,
    thenFinish: step.finished,
  };
}
