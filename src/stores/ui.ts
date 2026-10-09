import { create } from 'zustand';
import { persist } from 'zustand/middleware';

interface UiState {
  /** 侧边栏是否折叠 */
  sidebarCollapsed: boolean;
  toggleSidebar: () => void;
  /** 找剧筛选面板是否折叠（第三方同款：可整块收起只留开关行） */
  browseFiltersCollapsed: boolean;
  setBrowseFiltersCollapsed: (collapsed: boolean) => void;
  /** 小屏播放中（对齐 hgplayer：同一窗口缩成 480×270，侧栏隐藏、
   *  播放器换紧凑控件；退出恢复）。瞬态：不持久化，重启即常规窗口。 */
  miniScreen: boolean;
  setMiniScreen: (on: boolean) => void;
  /** 窗口置顶（对齐 hgplayer 的 De.pinned；窗口级状态，跨大小屏保持）。
   *  会话内有效：重启回到不置顶，与第三方一致。 */
  pinned: boolean;
  setPinned: (on: boolean) => void;
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
      miniScreen: false,
      setMiniScreen: (on) => set({ miniScreen: on }),
      pinned: false,
      setPinned: (on) => set({ pinned: on }),
    }),
    // miniScreen/pinned 不持久化：窗口几何的存/恢复在后端
    // （enter/exit_mini_screen），置顶与会话绑定（hgplayer 同款）；重启后
    // 状态若残留会呈现「大窗 + 紧凑控件」的错配。
    // lastCategory/lastGenre 是旧版官网嗅探浏览的遗留字段：新版找剧走
    // 官方筛选面板（组件内本地 state），持久化里的旧键靠 merge 自然废弃
    {
      name: 'hongguo-ui',
      partialize: (s) => ({
        sidebarCollapsed: s.sidebarCollapsed,
        browseFiltersCollapsed: s.browseFiltersCollapsed,
      }),
    },
  ),
);
