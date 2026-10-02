import { create } from 'zustand';

interface PlayerState {
  /** 当前播放的剧集 id */
  seriesId: string | null;
  /** 当前播放的集号 */
  vidIndex: number | null;
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
  setTarget: (seriesId, vidIndex) => set({ seriesId, vidIndex }),
  clear: () => set({ seriesId: null, vidIndex: null }),
}));
