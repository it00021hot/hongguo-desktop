import { call } from '../tauri/invoke';

// ---------------------------------------------------------------- 应用

export const app = {
  selectFolder: () => call('select_folder'),
  openFolder: (seriesId: string) => call('open_folder', { seriesId }),
  // 白名单网址（Rust 侧校验），设置页 ffmpeg 安装指引用
  openExternalPage: (page: string) => call('open_external_page', { page }),
  /**
   * 小屏播放（对齐 hgplayer）：**同一窗口**缩成 480×270 落到屏幕右下角，
   * 播放不断；进入前的窗口几何由后端保存，exitMiniScreen 原样恢复
   */
  enterMiniScreen: () => call('enter_mini_screen'),
  /** 退出小屏：恢复进入前的窗口几何与最小尺寸约束 */
  exitMiniScreen: () => call('exit_mini_screen'),
  /** 窗口置顶（对齐 hgplayer 的置顶按钮；窗口级状态，跨大小屏保持） */
  setAlwaysOnTop: (enabled: boolean) => call('set_always_on_top', { enabled }),
  /**
   * 竖屏播放（对齐 hgplayer）：窗口收窄成按视频宽高比算宽的竖条，竖屏
   * 视频无黑边包住画面；进入前的窗口几何由后端保存，exitPortraitScreen
   * 原样恢复。aspect = 视频宽高比（w/h），后端按它算竖条宽
   */
  enterPortraitScreen: (aspect: number) => call('enter_portrait_screen', { aspect }),
  /** 退出竖屏：恢复进入前的窗口几何与最小尺寸约束 */
  exitPortraitScreen: () => call('exit_portrait_screen'),
  /** 竖屏中视频宽高比变化后的重适配（full = 是否铺满档） */
  fitPortraitScreen: (aspect: number, full: boolean) =>
    call('fit_portrait_screen', { aspect, full }),
  /** 铺满档切换（对齐 hgplayer Nf）：铺满→普通还原快照，普通→铺满先快照 */
  setPortraitFull: (aspect: number, full: boolean) => call('set_portrait_full', { aspect, full }),
  /**
   * macOS 原生红绿灯藏显（无边框播放模式用；Tauri 同款只 setHidden 三颗
   * 圆点不动 styleMask）。Windows/Linux 本就 decorations:false，命令无害
   */
  setTrafficLightsVisible: (visible: boolean) => call('set_traffic_lights_visible', { visible }),
  /**
   * 隐身模式开关：开 = 后端起系统级光标轮询（鼠标脱离窗口隐藏、回到窗口
   * 区域自动重现）；关 = 停轮询并带回可能隐藏中的窗口
   */
  setIncognito: (enabled: boolean) => call('set_incognito', { enabled }),
  /** 关闭拦截就位（监听挂上后调用，后端才开始拦 CloseRequested） */
  markWindowReady: () => call('mark_window_ready'),
  /** 退出确认后的真退出（CloseRequested 被拦后唯一的退出路径） */
  exitApp: () => call('exit_app'),
};
