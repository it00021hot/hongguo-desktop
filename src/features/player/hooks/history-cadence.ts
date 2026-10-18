/**
 * 观看历史上报节拍（纯判定，由播放器的上报心跳按播放位置调用）。
 *
 * 为什么单独成判定、不挂在 5 秒落盘节流上：那套写法实测出两个错位
 * （2026-10-18 用户复现）——首报要等下一个落盘窗口，实际落在 5–10 秒；
 * 「每 60 秒一拍」的计数只在真正上报时自增，恒定卡在 1，`% 12` 永远不成立，
 * 长集播放时云端进度整集停在首报值（第七集播完、第八集开播，历史里还是
 * 上一集 + 5 秒）。
 *
 * 规则（1/3/4 条对齐 `docs/hongguo-api-endpoints.md` §9.4.2 的参考端实测；
 * 第 2 条是本项目补的——历史行按剧聚合，同剧换集后还显示上一集就是错的）：
 * 1. 新剧：播满 5 秒才建行——信息流快滑一路不落 1 秒噪点行；
 * 2. 同剧切集（本会话已报过这部）：新集起播立刻报一条，历史当场翻页；
 * 3. 同一集：每播满 60 秒一条；回跳超过容差立即补（进度已经变了），
 *    前跳由 60 秒节拍在下一拍追上；
 * 4. 暂停不计时：心跳只在播放中调用，暂停/切集/退出由调用方 force 上报。
 */

/** 首报门槛（毫秒）：参考端 v1.1.6 起播 5 秒首报。 */
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

  /** 记一条已上报的位置（force 上报后调用），节拍从它重新计时。 */
  note(next: EpisodePosition): void {
    this.reported = next;
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
    // 同集补 5 秒首报：上一次报在门槛之前（同剧切集的立即翻页只带了 0 秒
    // 附近的位置），跨过 5 秒时要让历史行的进度有意义
    if (last.positionMs < FIRST_REPORT_MS && next.positionMs >= FIRST_REPORT_MS) {
      this.reported = next;
      return true;
    }
    // 回跳也是「进度变了」：立即报，否则要等满 60 秒才追上
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
