import { create } from 'zustand';
import { writeLastTarget } from '@/utils/playback-prefs';

interface PlayerState {
  seriesId: string | null;
  vidIndex: number | null;
  /**
   * 选中剧连播（binge）：信息流里主动选了某集 = 要追这部，滚轮/↑↓切集而不是换剧。
   * null = 跟随信息流；跟着剧走，setTarget 切到别的剧自动解除。
   */
  bingeSeriesId: string | null;
  setBinge: (seriesId: string | null) => void;
  /**
   * 外部点名信号（详情/历史/收藏/搜索卡片、切季、剧终接下一季）：首页信息流消费它把游标对到目标剧。
   * 自增计数，同一部剧连续点名也生效；区别于信息流自己的 setTarget（游标驱动，不许触发对齐）。
   */
  gotoNonce: number;
  requestGoto: (seriesId: string, vidIndex: number) => void;
  /**
   * 外部点名已消费到的 nonce（首页写回）：跨页跳转会卸载重挂首页，存组件 state 的话重挂载会
   * 拿「当前 nonce」初始化成已消费，点名信号死在挂载帧。
   */
  gotoConsumed: number;
  markGotoConsumed: (nonce: number) => void;
  /**
   * 进入播放前的来源路由：播放器「返回」钮回到来源页而不是停在推荐流；
   * null = 无外部来源，不显示返回钮。会话态，不持久化。
   */
  returnTo: string | null;
  setReturnTo: (href: string | null) => void;
  /**
   * 跨客户端续播提示：云端历史有进度、本地播放档案没有时，信息流把「该看哪集 + 集内位置」放这里，
   * 播放器起播无本地 resumeAt 时消费，消费即清；不匹配当前集的残留提示会被忽略。
   */
  resumeHint: { seriesId: string; vidIndex: number; positionMs: number } | null;
  setResumeHint: (hint: { seriesId: string; vidIndex: number; positionMs: number } | null) => void;
  /** 弹幕设置面板开合（播放页/沉浸流共享，切集切剧不重置） */
  danmakuPanelOpen: boolean;
  setDanmakuPanelOpen: (open: boolean) => void;
  /** 画质增强面板开合（与弹幕面板同款：播放页/沉浸流共享，切集切剧不重置） */
  enhancePanelOpen: boolean;
  setEnhancePanelOpen: (open: boolean) => void;
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
 * 播放器状态：只放跨组件共享的播放上下文，易失数据归 TanStack Query；
 * 兼容模式/连播等持久化设置归 data.json（useSettings），不在 store 抄一份（抄了两边对不上会「改了没反应」）。
 */
export const usePlayerStore = create<PlayerState>((set, get) => ({
  seriesId: null,
  vidIndex: null,
  bingeSeriesId: null,
  setBinge: (seriesId) => set({ bingeSeriesId: seriesId }),
  gotoNonce: 0,
  requestGoto: (seriesId, vidIndex) => {
    get().setTarget(seriesId, vidIndex);
    set((s) => ({ gotoNonce: s.gotoNonce + 1 }));
  },
  gotoConsumed: 0,
  markGotoConsumed: (nonce) => set({ gotoConsumed: nonce }),
  returnTo: null,
  setReturnTo: (href) => set({ returnTo: href }),
  resumeHint: null,
  setResumeHint: (hint) => set({ resumeHint: hint }),
  danmakuPanelOpen: false,
  setDanmakuPanelOpen: (open) => set({ danmakuPanelOpen: open }),
  enhancePanelOpen: false,
  setEnhancePanelOpen: (open) => set({ enhancePanelOpen: open }),
  volumeOpen: false,
  setVolumeOpen: (open) => set({ volumeOpen: open }),
  seriesPanelOpen: false,
  commentPanelOpen: false,
  setSeriesPanelOpen: (open) => set({ seriesPanelOpen: open }),
  setCommentPanelOpen: (open) => set({ commentPanelOpen: open }),
  setTarget: (seriesId, vidIndex) => {
    // 目标持久化：重启后由首页种子（云端历史排头）自然续上，进度由本地播放档案的 resumeAt 接上
    writeLastTarget({ seriesId, vidIndex });
    set((s) => ({
      seriesId,
      vidIndex,
      bingeSeriesId: s.bingeSeriesId === seriesId ? s.bingeSeriesId : null,
    }));
  },
  clear: () => {
    writeLastTarget(null);
    set({ seriesId: null, vidIndex: null, bingeSeriesId: null });
  },
}));
