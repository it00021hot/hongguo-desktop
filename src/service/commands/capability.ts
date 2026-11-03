import { call } from '../tauri/invoke';

export const capability = {
  /** 本机 WebView2 能否解 HEVC（探测缓存在后端进程，会话内不变） */
  playbackCapability: () => call('playback_capability'),
  /** 按需下载并安装 HEVC 视频扩展，返回安装后的能力重探结果 */
  installHevcExtension: () => call('install_hevc_extension'),
};
