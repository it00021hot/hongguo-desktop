import { create } from 'zustand';
import { writeLastTarget } from '@/lib/playback-prefs';

interface PlayerState {
  /** 当前播放的剧集 id */
  seriesId: string | null;
  /** 当前播放的集号 */
  vidIndex: number | null;
  /**
   * 选中剧连播（binge）：信息流里主动选了某集 = 要追这部，滚轮/↑↓
   * 切集而不是换剧。null = 跟随信息流（滚动换剧）。
   * 跟着剧走：setTarget 切到别的剧自动解除。
   */
  bingeSeriesId: string | null;
  setBinge: (seriesId: string | null) => void;
  /**
   * 跨客户端续播提示：云端历史有进度、本地播放档案没有时，信息流把
   * 「该看哪一集 + 集内位置」放这里，播放器起播无本地 resumeAt 时消费。
   * 消费即清；不匹配当前集的残留提示会被忽略（无害）。
   */
  resumeHint: { seriesId: string; vidIndex: number; positionMs: number } | null;
  setResumeHint: (hint: { seriesId: string; vidIndex: number; positionMs: number } | null) => void;
  /** 弹幕设置面板开合（播放页/沉浸流共享，切集切剧不重置） */
  danmakuPanelOpen: boolean;
  setDanmakuPanelOpen: (open: boolean) => void;
  /** 音量竖条浮层开合 */
  volumeOpen: boolean;
  setVolumeOpen: (open: boolean) => void;
  /** 右侧选集面板开合（沉浸流默认隐藏，按钮呼出） */
  seriesPanelOpen: boolean;
  commentPanelOpen: boolean;
  setSeriesPanelOpen: (open: boolean) => void;
  setCommentPanelOpen: (open: boolean) => void;
  setTarget: (seriesId: string, vidIndex: number) => void;
  clear: () => void;
}

/**
 * 播放器状态。
 *
 * 只放「跨组件共享的播放上下文」，播放进度等易失数据不放在这里——
 * 那类数据由 TanStack Query 负责缓存与失效。
 *
 * 兼容模式、播完连播这些**是持久化设置**，归 data.json 管、由 `useSettings` 取，
 * 不在这里再抄一份。抄的那份既不从后端读也不往回写，两边对不上，
 * 表现就是「改了没反应、重启又变回默认」。
 */
export const usePlayerStore = create<PlayerState>((set) => ({
  seriesId: null,
  vidIndex: null,
  bingeSeriesId: null,
  setBinge: (seriesId) => set({ bingeSeriesId: seriesId }),
  resumeHint: null,
  setResumeHint: (hint) => set({ resumeHint: hint }),
  danmakuPanelOpen: false,
  setDanmakuPanelOpen: (open) => set({ danmakuPanelOpen: open }),
  volumeOpen: false,
  setVolumeOpen: (open) => set({ volumeOpen: open }),
  seriesPanelOpen: false,
  commentPanelOpen: false,
  setSeriesPanelOpen: (open) => set({ seriesPanelOpen: open }),
  setCommentPanelOpen: (open) => set({ commentPanelOpen: open }),
  setTarget: (seriesId, vidIndex) => {
    // 目标持久化：刷新/重启后播放器能恢复到正在看的这部剧这集
    // （进度由本地播放档案的 resumeAt 接上，见 PlayerPage 的恢复逻辑）
    writeLastTarget({ seriesId, vidIndex });
    set((s) => ({
      seriesId,
      vidIndex,
      // 连播跟着剧走：切到别的剧自动解除锁定
      bingeSeriesId: s.bingeSeriesId === seriesId ? s.bingeSeriesId : null,
    }));
  },
  clear: () => {
    writeLastTarget(null);
    set({ seriesId: null, vidIndex: null, bingeSeriesId: null });
  },
}));
