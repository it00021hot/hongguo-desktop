/** 渐进播放失败后的画面地址。增长中或已取消的同一路径不能直接交给 video。 */

export type ProducerState = 'running' | 'completed' | 'failed' | 'cancelled';

export function wholeFileSrc(input: {
  episodeKey: string;
  eventKey: string | null;
  producer: ProducerState;
  completeUrl: string | null;
  waiting: boolean;
}): string | null {
  if (input.eventKey && input.eventKey !== input.episodeKey) return null;
  if (input.producer === 'failed' || input.producer === 'cancelled') return null;
  if (!input.waiting || input.producer !== 'completed' || !input.completeUrl) return null;
  return input.completeUrl;
}

/** 离开这一集才取消生产任务。MSE 失败只停消费，不能取消后再等同一任务。 */
export function cancelProducerOn(reason: 'leave' | 'mse-failed'): boolean {
  return reason === 'leave';
}

/** MSE 效果清理是否应取消后台：仅换集/卸载，不因交接整片或 MSE 失败。 */
export function cancelProducerOnMseCleanup(reason: 'episode-leave' | 'mse-effect'): boolean {
  return reason === 'episode-leave';
}

/** 同集短暂 remount（StrictMode / harness 重导航）时推迟取消，避免误杀仍在写的 progressive producer。 */
const PENDING_CANCEL_MS = 150;
const pendingCancels = new Map<string, ReturnType<typeof setTimeout>>();

export function compatPlaybackKey(episodeKey: string, definition?: number): string {
  return definition === undefined ? episodeKey : `${episodeKey}:${definition}`;
}

/** 卸载/换集时调度取消；同 key 在窗口内 remount 可 retain 掉。 */
export function scheduleProducerCancel(
  seriesId: string,
  vidIndex: number,
  cancel: (args: { seriesId: string; vidIndex: number; definition?: number }) => void,
  definition?: number,
): void {
  const key = compatPlaybackKey(`${seriesId}:${vidIndex}`, definition);
  const prev = pendingCancels.get(key);
  if (prev !== undefined) clearTimeout(prev);
  pendingCancels.set(
    key,
    setTimeout(() => {
      pendingCancels.delete(key);
      cancel({ seriesId, vidIndex, definition });
    }, PENDING_CANCEL_MS),
  );
}

/** 同集 remount 时撤销尚未触发的取消。 */
export function retainProducerOnRemount(
  seriesId: string,
  vidIndex: number,
  definition?: number,
): void {
  const key = compatPlaybackKey(`${seriesId}:${vidIndex}`, definition);
  const prev = pendingCancels.get(key);
  if (prev === undefined) return;
  clearTimeout(prev);
  pendingCancels.delete(key);
}

/** 丢弃挂起的推迟取消（不触发）。显式 restart 前调用，避免与立即 cancelCompat 叠杀。 */
export function flushPendingProducerCancel(seriesId: string, vidIndex: number): void {
  retainProducerOnRemount(seriesId, vidIndex);
}

/** 测试用：清空挂起的取消定时器。 */
export function resetPendingProducerCancelsForTests(): void {
  for (const t of pendingCancels.values()) clearTimeout(t);
  pendingCancels.clear();
}
