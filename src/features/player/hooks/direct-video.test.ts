import { afterEach, describe, expect, it, vi } from 'vitest';
import { DirectVideoPlayer, directVideoSource } from './direct-video';

const { isWindows } = vi.hoisted(() => ({ isWindows: vi.fn(() => true) }));
vi.mock('@/utils/platform', () => ({ isWindows, usesSchemeHostForm: isWindows }));

function frameBytes(times: number[]) {
  const buffer = new ArrayBuffer(22 * times.length);
  const view = new DataView(buffer);
  for (const [index, pts] of times.entries()) {
    const offset = 22 * index;
    view.setUint32(offset, 2, true);
    view.setUint32(offset + 4, 2, true);
    view.setFloat64(offset + 8, pts, true);
  }
  return buffer;
}

function playerHarness(reply = (_url: string): ArrayBuffer | null => null) {
  const bridge = Object.assign(new EventTarget(), { releaseBuffer: vi.fn() });
  const audio = Object.assign(new EventTarget(), {
    paused: true,
    currentTime: 0,
    pause: vi.fn(),
    play: vi.fn(() => Promise.resolve()),
  });
  const drawImage = vi.fn();
  const canvas = Object.assign(new EventTarget(), {
    width: 2,
    height: 2,
    dataset: {},
    getContext: () => ({ drawImage }),
  });
  const fetchFrame = vi.fn((url: string, _init: RequestInit) =>
    Promise.resolve(new Response(reply(url), { status: url.includes('/stop/') ? 204 : 200 })),
  );
  const failed = vi.fn();
  const firstFrame = vi.fn();
  let tick: FrameRequestCallback | undefined;
  vi.stubGlobal('window', isWindows() ? { chrome: { webview: bridge } } : {});
  vi.stubGlobal(
    'VideoFrame',
    class {
      close() {}
    },
  );
  vi.stubGlobal('fetch', fetchFrame);
  vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) => {
    tick = callback;
    return 1;
  });
  vi.stubGlobal('cancelAnimationFrame', vi.fn());

  const player = new DirectVideoPlayer(
    'local/video',
    audio as unknown as HTMLVideoElement,
    canvas as unknown as HTMLCanvasElement,
    { firstFrame, waiting: vi.fn(), failed },
    0,
  );
  function deliver(url: string, ...times: number[]) {
    const id = Number(new URL(url).pathname.split('/')[3]);
    const buffer = frameBytes(times);
    bridge.dispatchEvent(
      Object.assign(new Event('sharedbufferreceived'), {
        additionalData: { directSession: id },
        getBuffer: () => buffer,
      }),
    );
    return buffer;
  }
  return {
    player,
    audio,
    bridge,
    fetchFrame,
    failed,
    firstFrame,
    drawImage,
    deliver,
    tick: () => tick?.(0),
  };
}

afterEach(() => {
  isWindows.mockReturnValue(true);
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

it('routes macOS online and local sources to the direct player', () => {
  isWindows.mockReturnValue(false);
  expect(directVideoSource('hongguo-stream://localhost/s/720/v1')).toBe('720/v1');
  expect(directVideoSource('hongguo-local://localhost/f/bW92aWUubXA0')).toBe('local/bW92aWUubXA0');
});

it('presents binary frames on macOS without a shared-buffer bridge', async () => {
  isWindows.mockReturnValue(false);
  const harness = playerHarness((url) =>
    url.includes('/start/') ? frameBytes([0, 1 / 30]) : null,
  );
  try {
    await vi.waitFor(() => {
      harness.tick();
      expect(harness.firstFrame).toHaveBeenCalledWith(2, 2);
    });
    expect(harness.fetchFrame.mock.calls[0]?.[0]).toMatch(
      /^hongguo-stream:\/\/localhost\/direct\//,
    );
    expect(harness.drawImage).toHaveBeenCalledOnce();
  } finally {
    harness.player.dispose();
  }
});

describe('Windows direct playback', () => {
  it('routes app-owned online and local playback URLs into the same player', () => {
    expect(directVideoSource('http://hongguo-stream.localhost/s/720/v1')).toBe('720/v1');
    expect(directVideoSource('http://hongguo-local.localhost/f/bW92aWUubXA0')).toBe(
      'local/bW92aWUubXA0',
    );
    expect(directVideoSource(null)).toBeNull();
    expect(directVideoSource('https://other.example/f/bW92aWUubXA0')).toBeNull();
  });

  it('releases superseded shared buffers when seeking rapidly', () => {
    const harness = playerHarness();
    try {
      const first = harness.fetchFrame.mock.calls[0]!;
      harness.audio.currentTime = 1;
      harness.audio.dispatchEvent(new Event('seeking'));
      const second = harness.fetchFrame.mock.calls[2]!;
      harness.audio.currentTime = 2;
      harness.audio.dispatchEvent(new Event('seeking'));
      const latest = harness.fetchFrame.mock.calls[4]!;

      const outdated = [harness.deliver(first[0], 0), harness.deliver(second[0], 1)];
      harness.tick();
      expect(first[1].signal?.aborted && second[1].signal?.aborted).toBe(true);
      expect(harness.drawImage).not.toHaveBeenCalled();
      for (const buffer of outdated)
        expect(harness.bridge.releaseBuffer).toHaveBeenCalledWith(buffer);

      const current = harness.deliver(latest[0], 2);
      harness.tick();
      expect(harness.firstFrame).toHaveBeenCalledWith(2, 2);
      expect(harness.bridge.releaseBuffer).toHaveBeenCalledWith(current);
    } finally {
      harness.player.dispose();
    }
  });

  it('reports a missing shared buffer so playback can recover', async () => {
    vi.useFakeTimers();
    const harness = playerHarness();
    try {
      await vi.runAllTimersAsync();
      expect(harness.failed).toHaveBeenCalledOnce();
      expect(harness.drawImage).not.toHaveBeenCalled();
    } finally {
      harness.player.dispose();
    }
  });

  it('keeps playing while the last frame is current, then buffers if the next frame is late', () => {
    const harness = playerHarness();
    try {
      harness.deliver(
        harness.fetchFrame.mock.calls[0]![0],
        ...Array.from({ length: 9 }, (_, i) => i / 30),
      );
      harness.tick();
      expect(harness.player.buffering).toBe(false);

      harness.audio.paused = false;
      harness.audio.currentTime = 8 / 30;
      harness.tick();
      expect(harness.player.buffering).toBe(false);

      harness.audio.currentTime = 9 / 30;
      harness.tick();
      expect(harness.player.buffering).toBe(true);
    } finally {
      harness.player.dispose();
    }
  });
});
