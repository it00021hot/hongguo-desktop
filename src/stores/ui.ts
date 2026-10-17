import { create } from 'zustand';
import { persist } from 'zustand/middleware';

interface UiState {
  /** 侧边栏是否折叠 */
  sidebarCollapsed: boolean;
  toggleSidebar: () => void;
  /** 找剧筛选面板是否折叠（第三方同款：可整块收起只留开关行） */
  browseFiltersCollapsed: boolean;
  setBrowseFiltersCollapsed: (collapsed: boolean) => void;
  /** 追更更新提醒的弹出开关（第三方 popupOn 同款）：关 = 只进提醒列表不弹 toast */
  aibotPopupOn: boolean;
  setAibotPopupOn: (on: boolean) => void;
  /** 小屏播放中（对齐 hgplayer：同一窗口缩成 480×270，侧栏隐藏、
   *  播放器换紧凑控件；退出恢复）。瞬态：不持久化，重启即常规窗口。 */
  miniScreen: boolean;
  setMiniScreen: (on: boolean) => void;
  /** 窗口置顶（对齐 hgplayer 的 De.pinned；窗口级状态，跨大小屏保持）。
   *  会话内有效：重启回到不置顶，与第三方一致。 */
  pinned: boolean;
  setPinned: (on: boolean) => void;
  /** 窗口级全屏（hgplayer WindowFullscreen 同款）。瞬态：不持久化，
   *  重启即常规窗口；真值在窗口，store 只是镜像（use-window-fullscreen）。 */
  windowFullscreen: boolean;
  setWindowFullscreen: (on: boolean) => void;
  /** 竖屏播放中（hgplayer portrait：窗口收窄成竖条）。瞬态镜像：真值
   *  （进入前几何快照）在后端 enter/exit_portrait_screen。 */
  portrait: boolean;
  setPortrait: (on: boolean) => void;
  /** 竖屏铺满档（hgplayer portraitFull：撑满工作区高；普通档保持进入时
   *  窗口高）。只在 portrait 为真时有意义。 */
  portraitFull: boolean;
  setPortraitFull: (on: boolean) => void;
  /** 播放模式「窗口化无边框」（hgplayer 播放页 bare 布局同款）：顶栏/
   *  侧栏全藏但**不进 OS 全屏**，窗口保持原几何；macOS 红绿灯一并藏
   *  （setTrafficLightsVisible，后端 FRAMELESS 标记与小屏退出协调）。
   *  瞬态：不持久化，重启即常规窗口；只在播放 surface 上成立。 */
  playerFrameless: boolean;
  setPlayerFrameless: (on: boolean) => void;
}

/** 纯 UI 偏好：折叠状态。不含业务数据。 */
export const useUiStore = create<UiState>()(
  persist(
    (set, get) => ({
      // 默认折叠：图标窄栏是默认形态（用户展开后持久化记住）
      sidebarCollapsed: true,
      toggleSidebar: () => set({ sidebarCollapsed: !get().sidebarCollapsed }),
      browseFiltersCollapsed: false,
      setBrowseFiltersCollapsed: (collapsed) => set({ browseFiltersCollapsed: collapsed }),
      aibotPopupOn: true,
      setAibotPopupOn: (on) => set({ aibotPopupOn: on }),
      miniScreen: false,
      setMiniScreen: (on) => set({ miniScreen: on }),
      pinned: false,
      setPinned: (on) => set({ pinned: on }),
      windowFullscreen: false,
      setWindowFullscreen: (on) => set({ windowFullscreen: on }),
      portrait: false,
      setPortrait: (on) => set({ portrait: on, ...(on ? {} : { portraitFull: false }) }),
      portraitFull: false,
      setPortraitFull: (on) => set({ portraitFull: on }),
      playerFrameless: false,
      setPlayerFrameless: (on) => set({ playerFrameless: on }),
    }),
    // miniScreen/pinned/windowFullscreen/portrait 不持久化：窗口几何的存/恢复
    // 在后端（enter/exit_mini_screen、enter/exit_portrait_screen），置顶与
    // 全屏是会话态（hgplayer 同款）；重启后状态若残留会呈现「大窗 + 紧凑
    // 控件」的错配。
    // lastCategory/lastGenre 是旧版官网嗅探浏览的遗留字段：新版找剧走
    // 官方筛选面板（组件内本地 state），持久化里的旧键靠 merge 自然废弃
    {
      name: 'hongguo-ui',
      partialize: (s) => ({
        sidebarCollapsed: s.sidebarCollapsed,
        browseFiltersCollapsed: s.browseFiltersCollapsed,
        aibotPopupOn: s.aibotPopupOn,
      }),
    },
  ),
);
