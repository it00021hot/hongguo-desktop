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
