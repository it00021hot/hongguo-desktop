import { call } from '../tauri/invoke';

export const app = {
  selectFolder: () => call('select_folder'),
  openFolder: (seriesId: string) => call('open_folder', { seriesId }),
  // 白名单网址（Rust 侧校验），设置页 ffmpeg 安装指引用
  openExternalPage: (page: string) => call('open_external_page', { page }),
  getWindowPresentation: () => call('get_window_presentation'),
  /** 小屏播放：同一窗口缩成 480×270 落屏幕右下角，播放不断；几何由后端保存，exitMiniScreen 原样恢复 */
  enterMiniScreen: () => call('enter_mini_screen'),
  exitMiniScreen: () => call('exit_mini_screen'),
  /** 窗口置顶（窗口级状态，跨大小屏保持） */
  setAlwaysOnTop: (enabled: boolean) => call('set_always_on_top', { enabled }),
  /** 竖屏播放：窗口收窄成按视频宽高比算宽的竖条（aspect = w/h），无黑边包住画面；几何由后端保存，exitPortraitScreen 原样恢复 */
  enterPortraitScreen: (aspect: number) => call('enter_portrait_screen', { aspect }),
  exitPortraitScreen: () => call('exit_portrait_screen'),
  /** 竖屏中视频宽高比变化后的重适配（full = 是否铺满档） */
  fitPortraitScreen: (aspect: number, full: boolean) =>
    call('fit_portrait_screen', { aspect, full }),
  /** 铺满档切换：铺满→普通还原快照，普通→铺满先快照 */
  setPortraitFull: (aspect: number, full: boolean) => call('set_portrait_full', { aspect, full }),
  /** macOS 原生红绿灯藏显（无边框播放模式用；非 macOS 命令无害） */
  setTrafficLightsVisible: (visible: boolean) => call('set_traffic_lights_visible', { visible }),
  /** 隐身模式：开后端起系统级光标轮询（离窗隐藏/回窗重现），关则停轮询并带回隐藏中的窗口 */
  setIncognito: (enabled: boolean) => call('set_incognito', { enabled }),
  /** 关闭拦截就位（监听挂上后调用，后端才开始拦 CloseRequested） */
  markWindowReady: () => call('mark_window_ready'),
  /** 退出确认后的真退出（CloseRequested 被拦后唯一的退出路径） */
  exitApp: () => call('exit_app'),
};
