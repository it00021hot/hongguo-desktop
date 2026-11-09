import { call } from '../tauri/invoke';

export const transcode = {
  capability: () => call('decode_capability'),
  redetect: () => call('redetect_capability'),
  transcodeForPlayback: (args: { seriesId: string; vidIndex: number; vid?: string }) =>
    call('transcode_for_playback', args),
  cancelCompat: (args: { seriesId: string; vidIndex: number }) =>
    call('cancel_compat_playback', args),
  /** 仅离线 QA：本地明文样本 → 兼容转码首段/MSE。filePath 空则默认 DATA_DIR/inputs/hg-bytevc2-plain.mp4 */
  qaOfflineLocalPlay: (filePath = '') => call('qa_offline_local_play', { filePath }),
};
