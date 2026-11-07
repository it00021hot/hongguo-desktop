import { call } from '../tauri/invoke';

export const transcode = {
  capability: () => call('decode_capability'),
  redetect: () => call('redetect_capability'),
  transcodeForPlayback: (args: { seriesId: string; vidIndex: number; vid?: string }) =>
    call('transcode_for_playback', args),
  cancelCompat: (args: { seriesId: string; vidIndex: number }) =>
    call('cancel_compat_playback', args),
};
