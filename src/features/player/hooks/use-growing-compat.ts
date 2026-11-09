import { useEffect } from 'react';
import { transcode } from '@/service/commands';
import { classifyGrow, decidePullAction } from './growing-media';
import {
  cancelProducerOn,
  cancelProducerOnMseCleanup,
  retainProducerOnRemount,
  scheduleProducerCancel,
} from './growing-fallback';

const MIME_CANDIDATES = [
  'video/mp4; codecs="avc1.64001F, mp4a.40.5"',
  'video/mp4; codecs="avc1.64001F"',
];

/**
 * 把增长中的兼容地址喂给 `<video>`。一次拉取的结束只代表这段字节到了，
 * 整片结束只认 `done`。屏幕是否出画不在这里断言。
 *
 * MSE 不支持/初始化/fetch/append 失败只停消费并通知上层等整片；
 * grow cancelled/failed 同样必须 onUnavailable，不能只靠进度事件或换集。
 * 换集/卸载才取消生产任务，避免取消后再等待同一任务。
 */
export function useGrowingCompat(
  videoRef: React.RefObject<HTMLVideoElement | null>,
  url: string | null,
  seriesId: string | null,
  vidIndex: number | null,
  onUnavailable: () => void,
) {
  useEffect(() => {
    if (seriesId && vidIndex != null) {
      retainProducerOnRemount(seriesId, vidIndex);
    }
    return () => {
      if (
        seriesId &&
        vidIndex != null &&
        cancelProducerOnMseCleanup('episode-leave') &&
        cancelProducerOn('leave')
      ) {
        // 推迟取消：同集 remount 会 retain，避免 harness/StrictMode 误杀 progressive
        scheduleProducerCancel(seriesId, vidIndex, (args) => {
          void transcode.cancelCompat(args);
        });
      }
    };
  }, [seriesId, vidIndex]);

  useEffect(() => {
    if (!url || !seriesId || vidIndex == null) return;
    const video = videoRef.current;
    const MediaSourceCtor = window.MediaSource;
    const mime = MediaSourceCtor
      ? MIME_CANDIDATES.find((item) => MediaSourceCtor.isTypeSupported(item))
      : undefined;
    if (!video || !MediaSourceCtor || !mime) {
      onUnavailable();
      return;
    }
    const source = new MediaSourceCtor();
    const objectUrl = URL.createObjectURL(source);
    const abort = new AbortController();
    let stopped = false;
    const failConsumer = () => {
      if (stopped) return;
      abort.abort();
      if (!cancelProducerOn('mse-failed')) onUnavailable();
    };
    video.src = objectUrl;
    const onSourceOpen = () => {
      let buffer: SourceBuffer;
      try {
        buffer = source.addSourceBuffer(mime);
      } catch {
        failConsumer();
        return;
      }
      buffer.addEventListener('error', failConsumer);
      void pull(url, abort.signal, buffer, source, () => stopped, onUnavailable, video).catch(
        failConsumer,
      );
    };
    source.addEventListener('sourceopen', onSourceOpen);
    return () => {
      stopped = true;
      abort.abort();
      source.removeEventListener('sourceopen', onSourceOpen);
      URL.revokeObjectURL(objectUrl);
      if (video.src === objectUrl) {
        video.removeAttribute('src');
        video.load();
      }
      // 交接整片或 MSE 失败清理时不取消生产任务
      void cancelProducerOnMseCleanup('mse-effect');
    };
  }, [url, seriesId, vidIndex, videoRef, onUnavailable]);
}

async function pull(
  url: string,
  signal: AbortSignal,
  buffer: SourceBuffer,
  source: MediaSource,
  stopped: () => boolean,
  onUnavailable: () => void,
  video?: HTMLVideoElement | null,
) {
  let offset = 0;
  let kickedPlay = false;
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
    const decision = decidePullAction(step);
    if (decision.action === 'unavailable') {
      if (source.readyState === 'open') source.endOfStream('network');
      // grow cancelled/failed：必须通知上层，不能只 return 或只靠 Failed 进度/换集
      if (!stopped()) onUnavailable();
      return;
    }
    if (decision.action === 'finish') {
      if (source.readyState === 'open') source.endOfStream();
      return;
    }
    await append(buffer, bytes);
    // 冷 progressive：首段 append 后 autoPlay 常仍 paused，显式 play 才能计入可见首帧
    if (!kickedPlay && video) {
      kickedPlay = true;
      void video.play().catch(() => undefined);
    }
    offset = decision.nextOffset;
    if (decision.thenFinish) {
      if (source.readyState === 'open') source.endOfStream();
      return;
    }
  }
}

function append(buffer: SourceBuffer, bytes: Uint8Array): Promise<void> {
  return new Promise((resolve, reject) => {
    const done = () => {
      buffer.removeEventListener('updateend', done);
      buffer.removeEventListener('error', onError);
      resolve();
    };
    const onError = () => {
      buffer.removeEventListener('updateend', done);
      buffer.removeEventListener('error', onError);
      reject(new Error('SourceBuffer error'));
    };
    buffer.addEventListener('updateend', done);
    buffer.addEventListener('error', onError);
    const copy = new Uint8Array(bytes.byteLength);
    copy.set(bytes);
    try {
      buffer.appendBuffer(copy);
    } catch (error) {
      buffer.removeEventListener('updateend', done);
      buffer.removeEventListener('error', onError);
      reject(error instanceof Error ? error : new Error(String(error)));
    }
  });
}
