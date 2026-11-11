import { isWindows, usesSchemeHostForm } from '@/utils/platform';
import { LOCAL_SCHEME, STREAM_SCHEME } from '@/utils/scheme';

const PREBUFFER_FRAMES = 8;
const LOW_WATER_FRAMES = 16;
const DISPLAY_LEAD_SECONDS = 0.008;
const DEFAULT_FRAME_INTERVAL = 1 / 30;
const HEADER_BYTES = 16;
const SHARED_BUFFER_TIMEOUT_MS = 5000;
export const DIRECT_BYTEVC2_FRAME = 'hongguo-bytevc2-frame';

function schemeBase(scheme: string): string {
  return usesSchemeHostForm() ? `http://${scheme}.localhost` : `${scheme}://localhost`;
}

function directRoot(): string {
  return `${schemeBase(STREAM_SCHEME)}/bvc2`;
}

export function directBytevc2Source(src: string | null, isBytevc2: boolean): string | null {
  if (!src || !isBytevc2) return null;
  const streamPrefix = `${schemeBase(STREAM_SCHEME)}/s/`;
  if (src.startsWith(streamPrefix)) {
    const source = src.slice(streamPrefix.length);
    return /^[1-9]\d*\/[a-zA-Z0-9_-]+$/.test(source) ? source : null;
  }
  const localPrefix = `${schemeBase(LOCAL_SCHEME)}/f/`;
  if (!src.startsWith(localPrefix)) return null;
  const file = src.slice(localPrefix.length);
  return /^[a-zA-Z0-9_-]+$/.test(file) ? `local/${file}` : null;
}

export function directBytevc2Audio(source: string): string {
  return `${directRoot()}/audio/${source}`;
}

type BufferEvent = Event & {
  additionalData?: { bytevc2Session?: number };
  getBuffer: () => ArrayBuffer;
};

type WebViewBridge = EventTarget & { releaseBuffer: (buffer: ArrayBuffer) => void };

const receivers = new Map<number, (buffer: ArrayBuffer) => void>();
let installed: WebViewBridge | undefined;
let nextId = Date.now() * 1024;

function getBridge(): WebViewBridge {
  const host = (window as unknown as { chrome?: { webview?: WebViewBridge } }).chrome?.webview;
  if (!host || typeof host.releaseBuffer !== 'function') {
    throw new Error('WebView2 shared buffers are unavailable');
  }
  if (installed !== host) {
    host.addEventListener('sharedbufferreceived', (event) => {
      const incoming = event as BufferEvent;
      const id = incoming.additionalData?.bytevc2Session;
      if (id === undefined) return;
      const buffer = incoming.getBuffer();
      const receive = receivers.get(id);
      if (receive) receive(buffer);
      else host.releaseBuffer(buffer);
    });
    installed = host;
  }
  return host;
}

type Frame = {
  width: number;
  height: number;
  pts: number;
  pixels: Uint8Array<ArrayBuffer>;
  release: () => void;
};

function framesFrom(buffer: ArrayBuffer, dispose: () => void): Frame[] {
  const view = new DataView(buffer);
  const frames: Frame[] = [];
  let remaining = 0;
  const release = () => {
    remaining--;
    if (!remaining) dispose();
  };
  for (let offset = 0; offset < buffer.byteLength;) {
    if (buffer.byteLength - offset < HEADER_BYTES)
      throw new Error('Incomplete ByteVC2 frame header');
    const width = view.getUint32(offset, true);
    const height = view.getUint32(offset + 4, true);
    const pts = view.getFloat64(offset + 8, true);
    const start = offset + HEADER_BYTES;
    const length = (width * height * 3) / 2;
    if (
      !width ||
      !height ||
      width % 2 ||
      height % 2 ||
      !Number.isFinite(pts) ||
      length > buffer.byteLength - start
    ) {
      throw new Error('Invalid ByteVC2 frame');
    }
    frames.push({ width, height, pts, pixels: new Uint8Array(buffer, start, length), release });
    offset = start + length;
    remaining++;
  }
  if (!remaining) dispose();
  return frames;
}

type Run = {
  id: number;
  target: number;
  stop: AbortController;
  frames: Frame[];
  ended: boolean;
  requesting: boolean;
  wakeRequest?: () => void;
  first: boolean;
  lastPts: number;
  frameInterval: number;
};

type Callbacks = {
  firstFrame: (width: number, height: number) => void;
  waiting: (waiting: boolean) => void;
  failed: (error: unknown) => void;
};

export class DirectBytevc2Player {
  private readonly bridge = isWindows() ? getBridge() : undefined;
  private readonly root = directRoot();
  private readonly surface: CanvasRenderingContext2D;
  private readonly events = new AbortController();
  private run?: Run;
  private animation = 0;
  private wantsPlay = true;
  private drawn = 0;
  private dropped = 0;
  private rebuffers = 0;
  buffering = false;

  constructor(
    private readonly source: string,
    private readonly audio: HTMLVideoElement,
    private readonly canvas: HTMLCanvasElement,
    private readonly callbacks: Callbacks,
    target: number,
  ) {
    if (typeof VideoFrame !== 'function') throw new Error('VideoFrame is unavailable');
    const surface = canvas.getContext('2d');
    if (!surface) throw new Error('ByteVC2 canvas is unavailable');
    this.surface = surface;
    const { signal } = this.events;
    audio.addEventListener('seeking', this.seek, { signal });
    audio.addEventListener('loadedmetadata', this.metadata, { signal });
    audio.addEventListener('play', this.play, { signal });
    audio.addEventListener('pause', this.pause, { signal });
    this.begin(target);
    this.animation = requestAnimationFrame(this.tick);
  }

  dispose() {
    this.events.abort();
    cancelAnimationFrame(this.animation);
    this.stopRun();
  }

  private readonly seek = () => {
    const target = this.audio.currentTime;
    if (this.run && this.buffering && Math.abs(this.run.target - target) < 0.001) return;
    if (!this.buffering) this.wantsPlay = !this.audio.paused;
    this.begin(target);
  };

  private readonly metadata = () => {
    if (this.run && this.run.target >= this.audio.duration) this.begin(this.audio.currentTime);
  };

  private readonly play = () => {
    this.wantsPlay = true;
    if (this.buffering) this.audio.pause();
  };

  private readonly pause = () => {
    if (!this.buffering) this.wantsPlay = false;
  };

  private setBuffering(value: boolean) {
    if (this.buffering === value) return;
    this.buffering = value;
    this.canvas.dataset.buffering = String(value);
    this.callbacks.waiting(value);
    if (value) this.audio.pause();
  }

  private stopRun() {
    const run = this.run;
    if (!run) return;
    this.run = undefined;
    receivers.delete(run.id);
    run.stop.abort();
    run.wakeRequest?.();
    for (const frame of run.frames) frame.release();
    void fetch(`${this.root}/stop/${run.id}`, { method: 'POST' }).catch(() => undefined);
  }

  private begin(target: number) {
    this.stopRun();
    const run: Run = {
      id: ++nextId,
      target,
      stop: new AbortController(),
      frames: [],
      ended: false,
      requesting: false,
      first: false,
      lastPts: target,
      frameInterval: DEFAULT_FRAME_INTERVAL,
    };
    this.run = run;
    this.setBuffering(true);
    const bridge = this.bridge;
    if (bridge) {
      receivers.set(run.id, (buffer) => {
        const dispose = () => bridge.releaseBuffer(buffer);
        try {
          run.frames.push(...framesFrom(buffer, dispose));
        } catch (error) {
          dispose();
          this.fail(error);
        } finally {
          run.wakeRequest?.();
        }
      });
    }
    void this.request(run, `start/${run.id}/${this.source}/${Math.round(target * 1000)}`);
  }

  private async request(run: Run, path: string) {
    run.requesting = true;
    let timeout: ReturnType<typeof setTimeout> | undefined;
    try {
      const shared = this.bridge
        ? new Promise<void>((resolve) => {
            run.wakeRequest = resolve;
          })
        : undefined;
      const response = await fetch(`${this.root}/${path}`, {
        method: 'POST',
        signal: run.stop.signal,
      });
      if (!response.ok) throw new Error(await response.text());
      if (response.status === 204) {
        run.ended = true;
        if (!run.first) throw new Error('ByteVC2 returned no frames');
      } else if (shared) {
        await Promise.race([
          shared,
          new Promise<never>((_, reject) => {
            timeout = setTimeout(
              () => reject(new Error('Timed out waiting for the ByteVC2 shared buffer')),
              SHARED_BUFFER_TIMEOUT_MS,
            );
          }),
        ]);
      } else {
        const bytes = await response.arrayBuffer();
        if (!run.stop.signal.aborted) run.frames.push(...framesFrom(bytes, () => undefined));
      }
    } catch (error) {
      if (!run.stop.signal.aborted) this.fail(error);
    } finally {
      if (timeout !== undefined) clearTimeout(timeout);
      run.wakeRequest = undefined;
      run.requesting = false;
    }
  }

  private fail(error: unknown) {
    this.setBuffering(true);
    this.stopRun();
    this.callbacks.failed(error);
  }

  private draw(frame: Frame, run: Run) {
    try {
      const picture = new VideoFrame(frame.pixels, {
        format: 'I420',
        codedWidth: frame.width,
        codedHeight: frame.height,
        timestamp: Math.round(frame.pts * 1e6),
      });
      try {
        if (this.canvas.width !== frame.width || this.canvas.height !== frame.height) {
          this.canvas.width = frame.width;
          this.canvas.height = frame.height;
        }
        this.surface.drawImage(picture, 0, 0);
      } finally {
        picture.close();
      }
    } finally {
      frame.release();
    }
    this.canvas.dataset.videoPts = String(frame.pts);
    const next = run.frames[0];
    if (next && next.pts > frame.pts) run.frameInterval = next.pts - frame.pts;
    run.lastPts = frame.pts;
    this.canvas.dataset.renderedFrames = String(++this.drawn);
    this.canvas.dataset.droppedFrames = String(this.dropped);
    this.canvas.dispatchEvent(new Event(DIRECT_BYTEVC2_FRAME));
    if (!run.first) {
      run.first = true;
      this.callbacks.firstFrame(frame.width, frame.height);
    }
  }

  private readonly tick = () => {
    const run = this.run;
    if (run) {
      try {
        this.present(run);
      } catch (error) {
        this.fail(error);
      }
    }
    this.animation = requestAnimationFrame(this.tick);
  };

  private present(run: Run) {
    if (!run.first && run.frames.length) this.draw(run.frames.shift()!, run);
    if (run.first && !run.requesting && !run.ended && run.frames.length < LOW_WATER_FRAMES) {
      void this.request(run, `next/${run.id}`);
    }
    if (!this.audio.paused && !this.audio.seeking) {
      let frame: Frame | undefined;
      const deadline = this.audio.currentTime + DISPLAY_LEAD_SECONDS;
      while (run.frames.length && run.frames[0]!.pts <= deadline) {
        if (frame) {
          frame.release();
          this.dropped++;
        }
        frame = run.frames.shift();
      }
      if (frame) this.draw(frame, run);
      if (
        !run.frames.length &&
        !run.ended &&
        this.audio.currentTime >= run.lastPts + run.frameInterval
      ) {
        this.canvas.dataset.rebuffers = String(++this.rebuffers);
        this.setBuffering(true);
      }
    }
    if (this.buffering && (run.frames.length >= PREBUFFER_FRAMES || run.ended)) {
      this.setBuffering(false);
      if (this.wantsPlay && !this.audio.ended) {
        void this.audio.play().catch((error: unknown) => {
          if (this.run === run && !(error instanceof DOMException && error.name === 'AbortError')) {
            this.fail(error);
          }
        });
      }
    }
  }
}
