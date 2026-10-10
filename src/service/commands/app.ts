import { call } from '../tauri/invoke';

// ---------------------------------------------------------------- 应用

export const app = {
  selectFolder: () => call<string | null>('select_folder'),
  openFolder: (seriesId: string) => call<void>('open_folder', { seriesId }),
  // 白名单网址（Rust 侧校验），设置页 ffmpeg 安装指引用
  openExternalPage: (page: string) => call<void>('open_external_page', { page }),
  /**
   * 小屏播放（对齐 hgplayer）：**同一窗口**缩成 480×270 落到屏幕右下角，
   * 播放不断；进入前的窗口几何由后端保存，exitMiniScreen 原样恢复
   */
  enterMiniScreen: () => call<void>('enter_mini_screen'),
  /** 退出小屏：恢复进入前的窗口几何与最小尺寸约束 */
  exitMiniScreen: () => call<void>('exit_mini_screen'),
  /** 窗口置顶（对齐 hgplayer 的置顶按钮；窗口级状态，跨大小屏保持） */
  setAlwaysOnTop: (enabled: boolean) => call<void>('set_always_on_top', { enabled }),
  /**
   * 隐身模式开关：开 = 后端起系统级光标轮询（鼠标脱离窗口隐藏、回到窗口
   * 区域自动重现）；关 = 停轮询并带回可能隐藏中的窗口
   */
  setIncognito: (enabled: boolean) => call<void>('set_incognito', { enabled }),
};
