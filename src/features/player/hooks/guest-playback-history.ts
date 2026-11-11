import type { watchHistory } from '@/service/commands';
import type { GuestPlaybackReport } from '@/service/schema';

type HistoryItem = Parameters<typeof watchHistory.record>[0];
type ProgressItem = GuestPlaybackReport['progress'][number];

const EXIT_MIN_PLAYED_MS = 5_000;

/** 累计时长跨播放器重开保留，按剧分开。 */
export class GuestPlaybackHistory {
  private active: HistoryItem | null = null;
  private lastPositionMs: number | null = null;
  private observedAtMs = 0;
  private playedMs = 0;
  private pending = new Map<string, ProgressItem>();

  constructor(private totals: Map<string, number>) {}

  begin(item: HistoryItem, observedAtMs: number): HistoryItem[] {
    const active = this.active;
    const switchingSeries = active !== null && active.seriesId !== item.seriesId;
    const previous =
      switchingSeries && this.playedMs >= EXIT_MIN_PLAYED_MS ? this.withReport(active, []) : null;
    const previousProgress = previous ? this.recentProgress() : [];
    if (switchingSeries) {
      this.pending.clear();
      this.playedMs = 0;
    }
    const first = !this.totals.has(item.seriesId);
    if (first) this.totals.set(item.seriesId, 0);
    this.active = item;
    this.lastPositionMs = item.positionMs;
    this.observedAtMs = observedAtMs;
    const progress =
      first || previous ? [this.progressItem(item, observedAtMs), ...previousProgress] : [];
    const current = this.withReport(item, progress);
    return previous ? [previous, current] : [current];
  }

  isActive(seriesId: string, vid: string): boolean {
    return this.active?.seriesId === seriesId && this.active.vid === vid;
  }

  seek(): void {
    this.lastPositionMs = null;
  }

  observe(positionMs: number, durationMs: number, observedAtMs: number): void {
    if (!this.active) return;
    const delta = this.lastPositionMs == null ? 0 : Math.max(0, positionMs - this.lastPositionMs);
    this.lastPositionMs = positionMs;
    const item = { ...this.active, positionMs, durationMs };
    this.active = item;
    if (delta <= 0) return;
    this.observedAtMs = observedAtMs;
    this.playedMs += delta;
    this.totals.set(item.seriesId, this.cumulative(item) + delta);
    const key = `${item.seriesId}:${item.vid}`;
    this.pending.delete(key);
    this.pending.set(key, this.progressItem(item, observedAtMs));
  }

  finish(): HistoryItem | null {
    const report =
      this.active && this.playedMs >= EXIT_MIN_PLAYED_MS
        ? this.withReport(this.active, this.recentProgress())
        : null;
    this.active = null;
    this.lastPositionMs = null;
    this.pending.clear();
    this.playedMs = 0;
    return report;
  }

  private cumulative(item: HistoryItem): number {
    return this.totals.get(item.seriesId) ?? 0;
  }

  private recentProgress(): ProgressItem[] {
    return [...this.pending.values()].reverse();
  }

  private progressItem(item: HistoryItem, observedAtMs: number): ProgressItem {
    return {
      seriesId: item.seriesId,
      vid: item.vid,
      positionMs: item.positionMs,
      cumulativeMs: this.cumulative(item),
      observedAtMs,
    };
  }

  private withReport(item: HistoryItem, progress: ProgressItem[]): HistoryItem {
    return {
      ...item,
      meta: {
        ...item.meta,
        guest: { cumulativeMs: this.cumulative(item), observedAtMs: this.observedAtMs, progress },
      },
    };
  }
}
