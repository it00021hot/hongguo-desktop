import { create } from 'zustand';
import { persist } from 'zustand/middleware';

interface UiState {
  /** 侧边栏是否折叠 */
  sidebarCollapsed: boolean;
  toggleSidebar: () => void;
  /** 浏览页上次选的分类与题材，切页后保持 */
  lastCategory: string;
  lastGenre: string;
  setBrowseFilter: (category: string, genre: string) => void;
}

/** 纯 UI 偏好：折叠状态、筛选记忆。不含业务数据。 */
export const useUiStore = create<UiState>()(
  persist(
    (set, get) => ({
      sidebarCollapsed: false,
      toggleSidebar: () => set({ sidebarCollapsed: !get().sidebarCollapsed }),
      lastCategory: 'real-drama',
      lastGenre: '',
      setBrowseFilter: (lastCategory, lastGenre) => set({ lastCategory, lastGenre }),
    }),
    { name: 'hongguo-ui' },
  ),
);
