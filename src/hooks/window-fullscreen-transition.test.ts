import { afterEach, beforeEach, expect, it, vi } from 'vitest';

const { nativeWindow, app, isMac, unlisten, listener } = vi.hoisted(() => ({
  nativeWindow: { setFullscreen: vi.fn() },
  app: { getWindowPresentation: vi.fn() },
  isMac: vi.fn(() => true),
  unlisten: vi.fn(),
  listener: { exited: undefined as ((event: { payload: unknown }) => void) | undefined },
}));

vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: () => nativeWindow }));
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn((_name, handler: (event: { payload: unknown }) => void) => {
    listener.exited = handler;
    return Promise.resolve(unlisten);
  }),
}));
vi.mock('@/service/commands', () => ({ app }));
vi.mock('@/utils/platform', () => ({ isMac }));
vi.mock('@/locales', () => ({ t: (key: string) => key }));

beforeEach(() => {
  vi.clearAllMocks();
  isMac.mockReturnValue(true);
  nativeWindow.setFullscreen.mockResolvedValue(undefined);
  app.getWindowPresentation.mockResolvedValue({ isFullscreen: true });
  listener.exited = undefined;
});
afterEach(() => vi.useRealTimers());

it('waits for the native macOS exit notification after the command was accepted', async () => {
  const { setWindowFullscreen } = await import('./window-fullscreen-transition');
  let finished = false;
  const exiting = setWindowFullscreen(false).then(() => {
    finished = true;
  });
  await vi.waitFor(() => expect(nativeWindow.setFullscreen).toHaveBeenCalledWith(false));
  expect(listener.exited).toBeDefined();
  expect(finished).toBe(false);
  listener.exited?.({ payload: null });
  await exiting;
  expect(unlisten).toHaveBeenCalledOnce();
});

it('cleans up the notification listener when the native command fails', async () => {
  const failure = new Error('native fullscreen failed');
  nativeWindow.setFullscreen.mockRejectedValue(failure);
  const { setWindowFullscreen } = await import('./window-fullscreen-transition');
  await expect(setWindowFullscreen(false)).rejects.toBe(failure);
  expect(unlisten).toHaveBeenCalledOnce();
});

it('reports an uncompleted exit instead of allowing a resize after the deadline', async () => {
  vi.useFakeTimers();
  const { setWindowFullscreen } = await import('./window-fullscreen-transition');
  const exiting = expect(setWindowFullscreen(false)).rejects.toThrow(
    'player.fullscreenExitTimeout',
  );
  await vi.runAllTimersAsync();
  await exiting;
  expect(unlisten).toHaveBeenCalledOnce();
});

it('uses the native command directly on Windows without waiting for a macOS event', async () => {
  isMac.mockReturnValue(false);
  const { setWindowFullscreen } = await import('./window-fullscreen-transition');
  await setWindowFullscreen(false);
  expect(nativeWindow.setFullscreen).toHaveBeenCalledExactlyOnceWith(false);
  expect(listener.exited).toBeUndefined();
});

it('accepts an exit completed while the notification listener was being installed', async () => {
  app.getWindowPresentation
    .mockResolvedValueOnce({ isFullscreen: true })
    .mockResolvedValueOnce({ isFullscreen: false });
  const { setWindowFullscreen } = await import('./window-fullscreen-transition');
  await setWindowFullscreen(false);
  expect(nativeWindow.setFullscreen).not.toHaveBeenCalled();
  expect(unlisten).toHaveBeenCalledOnce();
});
