import { useEffect } from 'react';
import { transcode } from '@/service/commands';
import { classifyGrow } from './growing-media';

const MIME_CANDIDATES = [
  'video/mp4; codecs="avc1.64001F, mp4a.40.5"',
  'video/mp4; codecs="avc1.64001F"',
];

/**
 * 把增长中的兼容地址喂给 `<video>`。一次拉取的结束只代表这段字节到了，
 * 整片结束只认 `done`。屏幕是否出画不在这里断言。
 */
export function useGrowingCompat(
  videoRef: React.RefObject<HTMLVideoElement | null>,
  url: string | null,
  seriesId: string | null,
  vidIndex: number | null,
) {
  useEffect(() => {
    if (!url || !seriesId || vidIndex == null) return;
    const video = videoRef.current;
    const MediaSourceCtor = window.MediaSource;
    if (!video || !MediaSourceCtor) return;
    const mime = MIME_CANDIDATES.find((item) => MediaSourceCtor.isTypeSupported(item));
    if (!mime) return;
    const source = new MediaSourceCtor();
    const objectUrl = URL.createObjectURL(source);
    const abort = new AbortController();
    let stopped = false;
    video.src = objectUrl;
    source.addEventListener('sourceopen', () => {
      const buffer = source.addSourceBuffer(mime);
      void pull(url, abort.signal, buffer, source, () => stopped);
    });
    return () => {
      stopped = true;
      abort.abort();
      URL.revokeObjectURL(objectUrl);
      void transcode.cancelCompat({ seriesId, vidIndex });
    };
  }, [url, seriesId, vidIndex, videoRef]);
}

async function pull(
  url: string,
  signal: AbortSignal,
  buffer: SourceBuffer,
  source: MediaSource,
  stopped: () => boolean,
) {
  let offset = 0;
  for (;;) {
    if (stopped() || signal.aborted) return;
    const response = await fetch(url, { headers: { Range: `bytes=${offset}-` }, signal });
    const bytes = new Uint8Array(await response.arrayBuffer());
    const step = classifyGrow(
      response.status,
      response.headers.get('x-hongguo-grow'),
      bytes.byteLength,
      offset,
    );
    if (step.kind === 'cancelled' || step.kind === 'failed') {
      if (source.readyState === 'open') source.endOfStream('network');
      return;
    }
    if (step.kind === 'finished') {
      if (source.readyState === 'open') source.endOfStream();
      return;
    }
    await append(buffer, bytes);
    offset = step.nextOffset;
    if (step.finished) {
      if (source.readyState === 'open') source.endOfStream();
      return;
    }
  }
}

function append(buffer: SourceBuffer, bytes: Uint8Array): Promise<void> {
  return new Promise((resolve, reject) => {
    const done = () => {
      buffer.removeEventListener('updateend', done);
      resolve();
    };
    buffer.addEventListener('updateend', done);
    const copy = new Uint8Array(bytes.byteLength);
    copy.set(bytes);
    try {
      buffer.appendBuffer(copy);
    } catch (error) {
      buffer.removeEventListener('updateend', done);
      reject(error instanceof Error ? error : new Error(String(error)));
    }
  });
}
