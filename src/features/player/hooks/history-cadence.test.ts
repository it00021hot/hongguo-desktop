import { describe, expect, it } from 'vitest';

import { FIRST_REPORT_MS, HistoryCadence, PERIODIC_REPORT_MS } from './history-cadence';

const at = (episodeKey: string, seriesId: string, positionMs: number) => ({
  episodeKey,
  seriesId,
  positionMs,
});

describe('HistoryCadence', () => {
  it('新剧：播满 5 秒才建行，之后每 60 秒一拍', () => {
    const cadence = new HistoryCadence();
    expect(cadence.due(at('s1:1', 's1', FIRST_REPORT_MS - 1))).toBe(false);
    expect(cadence.due(at('s1:1', 's1', FIRST_REPORT_MS))).toBe(true);
    // 5 秒到 60 秒之间不再上报（旧的 %12 计数就是一拍之后再也不抬头）
    expect(cadence.due(at('s1:1', 's1', 10_000))).toBe(false);
    expect(cadence.due(at('s1:1', 's1', 5_000 + PERIODIC_REPORT_MS - 1))).toBe(false);
    expect(cadence.due(at('s1:1', 's1', 5_000 + PERIODIC_REPORT_MS))).toBe(true);
    // 第二拍从第一拍的位置重新计，而不是从 0 点算
    expect(cadence.due(at('s1:1', 's1', 5_000 + PERIODIC_REPORT_MS + 30_000))).toBe(false);
    expect(cadence.due(at('s1:1', 's1', 5_000 + PERIODIC_REPORT_MS * 2))).toBe(true);
  });

  it('同剧切集：新集起播立刻报一条，历史当场翻到新集', () => {
    const cadence = new HistoryCadence();
    expect(cadence.due(at('s1:7', 's1', 5_000))).toBe(true);
    // 第七集播完切第八集：0.2 秒也要报（历史行属于剧，不能还停在第七集）
    expect(cadence.due(at('s1:8', 's1', 200))).toBe(true);
    expect(cadence.hasReported('s1:8')).toBe(true);
    // 新集刚报过，同位置的后续心跳不重复
    expect(cadence.due(at('s1:8', 's1', 1_000))).toBe(false);
    // 翻页只带了 0 秒附近的位置：跨过 5 秒补一次首报，历史行的进度才有意义
    expect(cadence.due(at('s1:8', 's1', 5_000))).toBe(true);
    expect(cadence.due(at('s1:8', 's1', 6_000))).toBe(false);
  });

  it('换剧：仍守 5 秒门槛，快滑一路不落噪点行', () => {
    const cadence = new HistoryCadence();
    expect(cadence.due(at('s1:1', 's1', 5_000))).toBe(true);
    expect(cadence.due(at('s2:1', 's2', 1_000))).toBe(false);
    expect(cadence.due(at('s2:1', 's2', 4_999))).toBe(false);
    expect(cadence.due(at('s2:1', 's2', 5_000))).toBe(true);
  });

  it('回跳 / 前跳立刻补一条——进度已经变了，不能等 60 秒', () => {
    const cadence = new HistoryCadence();
    expect(cadence.due(at('s1:1', 's1', 5_000))).toBe(true);
    expect(cadence.due(at('s1:1', 's1', 40_000))).toBe(false);
    expect(cadence.due(at('s1:1', 's1', 1_000))).toBe(true);
    expect(cadence.due(at('s1:1', 's1', 1_500))).toBe(false);
    expect(cadence.due(at('s1:1', 's1', 200_000))).toBe(true);
  });

  it('位置不前进（暂停/缓冲）永远不报，force 由调用方另走', () => {
    const cadence = new HistoryCadence();
    expect(cadence.due(at('s1:1', 's1', 5_000))).toBe(true);
    for (let i = 0; i < 5; i += 1) {
      expect(cadence.due(at('s1:1', 's1', 5_000))).toBe(false);
    }
    // 片头 0 秒不建行
    expect(new HistoryCadence().due(at('s1:1', 's1', 0))).toBe(false);
  });

  it('note 记下 force 上报的位置，节拍与「本集已报」都从它起算', () => {
    const cadence = new HistoryCadence();
    cadence.note(at('s1:8', 's1', 2_000));
    expect(cadence.hasReported('s1:8')).toBe(true);
    expect(cadence.hasReported('s1:7')).toBe(false);
    expect(cadence.due(at('s1:8', 's1', 2_000 + PERIODIC_REPORT_MS))).toBe(true);
  });
});
