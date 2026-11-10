import { isWindows } from '@/utils/platform';
import { STREAM_SCHEME } from '@/utils/scheme';

const STREAM_ORIGIN = `http://${STREAM_SCHEME}.localhost`;
const ROOT = `${STREAM_ORIGIN}/bvc2`;
const PREBUFFER_FRAMES = 8;
const LOW_WATER_FRAMES = 16;
const DISPLAY_LEAD_SECONDS = 0.008;
const HEADER_BYTES = 16;

export function directBytevc2Source(src: string | null, needsCompat: boolean): string | null {
  if (!src || !needsCompat || !isWindows()) return null;
  const prefix = `${STREAM_ORIGIN}/s/`;
  if (!src.startsWith(prefix)) return null;
  const source = src.slice(prefix.length);
  return /^\d+\/[a-zA-Z0-9_-]+$/.test(source) ? source : null;
}

export function directBytevc2Audio(source: string): string {
  return `${ROOT}/audio/${source}`;
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
  if (!host || typeof host.releaseBuffer !== 'function' || typeof VideoFrame !== 'function') {
    throw new Error('WebView2 shared buffers or VideoFrame are unavailable');
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

function framesFrom(buffer: ArrayBuffer, bridge: WebViewBridge): Frame[] {
  const view = new DataView(buffer);
  const frames: Frame[] = [];
  let remaining = 0;
  const release = () => {
    remaining--;
    if (!remaining) bridge.releaseBuffer(buffer);
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
  if (!remaining) bridge.releaseBuffer(buffer);
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
};

type Callbacks = {
  firstFrame: (width: number, height: number) => void;
  waiting: (waiting: boolean) => void;
  failed: (error: unknown) => void;
};

export class DirectBytevc2Player {
  private readonly bridge = getBridge();
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
    void fetch(`${ROOT}/stop/${run.id}`, { method: 'POST' }).catch(() => undefined);
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
    };
    this.run = run;
    this.setBuffering(true);
    receivers.set(run.id, (buffer) => {
      try {
        run.frames.push(...framesFrom(buffer, this.bridge));
      } catch (error) {
        this.bridge.releaseBuffer(buffer);
        this.fail(error);
      } finally {
        run.wakeRequest?.();
      }
    });
    void this.request(run, `start/${run.id}/${this.source}/${Math.round(target * 1000)}`);
  }

  private async request(run: Run, path: string) {
    run.requesting = true;
    try {
      const shared = new Promise<void>((resolve) => {
        run.wakeRequest = resolve;
      });
      const response = await fetch(`${ROOT}/${path}`, {
        method: 'POST',
        signal: run.stop.signal,
      });
      if (!response.ok) throw new Error(await response.text());
      if (response.status === 204) {
        run.ended = true;
        if (!run.first) throw new Error('ByteVC2 returned no frames');
      } else {
        await shared;
      }
    } catch (error) {
      if (!run.stop.signal.aborted) this.fail(error);
    } finally {
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
    this.canvas.dataset.renderedFrames = String(++this.drawn);
    this.canvas.dataset.droppedFrames = String(this.dropped);
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
      if (!run.frames.length && !run.ended) {
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
