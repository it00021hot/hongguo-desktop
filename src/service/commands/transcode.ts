import { call } from '../tauri/invoke';

/** 转码能力（兼容合并用；播放不转码）。 */
export const transcode = {
  capability: () => call('decode_capability'),
  redetect: () => call('redetect_capability'),
};
