import { create } from 'zustand';
import { persist } from 'zustand/middleware';

interface UiState {
  /** 侧边栏是否折叠 */
  sidebarCollapsed: boolean;
  toggleSidebar: () => void;
  /** 找剧筛选面板是否折叠（第三方同款：可整块收起只留开关行） */
  browseFiltersCollapsed: boolean;
  setBrowseFiltersCollapsed: (collapsed: boolean) => void;
}

/** 纯 UI 偏好：折叠状态。不含业务数据。 */
export const useUiStore = create<UiState>()(
  persist(
    (set, get) => ({
      sidebarCollapsed: false,
      toggleSidebar: () => set({ sidebarCollapsed: !get().sidebarCollapsed }),
      browseFiltersCollapsed: false,
      setBrowseFiltersCollapsed: (collapsed) => set({ browseFiltersCollapsed: collapsed }),
    }),
    // lastCategory/lastGenre 是旧版官网嗅探浏览的遗留字段：新版找剧走
    // 官方筛选面板（组件内本地 state），持久化里的旧键靠 merge 自然废弃
    { name: 'hongguo-ui' },
  ),
);
