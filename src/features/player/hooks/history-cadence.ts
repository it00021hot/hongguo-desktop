/**
 * 观看历史上报节拍（纯判定）。不挂在 5 秒落盘节流上——那会让首报量化到 5-10 秒、60 秒节拍恒不成立。
 * 现有桌面规则见 docs/hongguo-api-endpoints.md §9.4.2。
 */

/** 桌面历史首报门槛（毫秒）。 */
export const FIRST_REPORT_MS = 5_000;

/** 中途更新节拍（毫秒）：按播放位置计，暂停不计时。 */
export const PERIODIC_REPORT_MS = 60_000;

/** 位置回跳容差（毫秒）：小于它视作读表噪声，不算用户 seek。 */
const SEEK_BACK_TOLERANCE_MS = 3_000;

/** 一次上报候选：身份 + 当前播放位置。 */
export interface EpisodePosition {
  /** 集身份（`seriesId:vidIndex`），跨集即换值。 */
  episodeKey: string;
  /** 剧身份：历史行按剧聚合，同剧换集可以立即翻页。 */
  seriesId: string;
  positionMs: number;
}

/** 上报节拍的记账器：一次播放生命周期一个实例。 */
export class HistoryCadence {
  private reported: EpisodePosition | null = null;

  /** 本集是否已经上报过（force 收尾上报的建行门槛）。 */
  hasReported(episodeKey: string): boolean {
    return this.reported?.episodeKey === episodeKey;
  }

  /** 记一条收尾位置；同一集的同一个位置只发送一次。 */
  note(next: EpisodePosition): boolean {
    if (
      this.reported?.episodeKey === next.episodeKey &&
      this.reported.positionMs === next.positionMs
    ) {
      return false;
    }
    this.reported = next;
    return true;
  }

  /** 现在该不该上报；true 时内部已记账，调用方负责真的发出去。 */
  due(next: EpisodePosition): boolean {
    if (next.positionMs <= 0) return false;
    const last = this.reported;
    if (!last) {
      if (next.positionMs < FIRST_REPORT_MS) return false;
      this.reported = next;
      return true;
    }
    if (last.episodeKey !== next.episodeKey) {
      // 同剧切集立即翻页；换剧仍守首报门槛（防快滑噪点行）
      if (last.seriesId === next.seriesId || next.positionMs >= FIRST_REPORT_MS) {
        this.reported = next;
        return true;
      }
      return false;
    }
    // 同集补 5 秒首报：上一次报在门槛之前（同剧切集的立即翻页只带 0 秒附近位置），跨过 5 秒让历史行进度有意义
    if (last.positionMs < FIRST_REPORT_MS && next.positionMs >= FIRST_REPORT_MS) {
      this.reported = next;
      return true;
    }
    // 回跳 = 进度变了：立即报，否则要等满 60 秒才追上
    if (next.positionMs + SEEK_BACK_TOLERANCE_MS < last.positionMs) {
      this.reported = next;
      return true;
    }
    if (next.positionMs - last.positionMs >= PERIODIC_REPORT_MS) {
      this.reported = next;
      return true;
    }
    return false;
  }
}
