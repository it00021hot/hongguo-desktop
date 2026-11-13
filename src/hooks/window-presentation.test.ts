import { beforeEach, expect, it, vi } from 'vitest';
import type { WindowPresentation } from '@/service/schema';

const { app, applyPresentation, setWindowFullscreen } = vi.hoisted(() => ({
  app: {
    getWindowPresentation: vi.fn(),
    enterMiniScreen: vi.fn(),
    exitMiniScreen: vi.fn(),
    enterPortraitScreen: vi.fn(),
    exitPortraitScreen: vi.fn(),
    setPortraitFull: vi.fn(),
    setTrafficLightsVisible: vi.fn(),
  },
  applyPresentation: vi.fn(),
  setWindowFullscreen: vi.fn(),
}));

vi.mock('@/service/commands', () => ({ app }));
vi.mock('./window-fullscreen-transition', () => ({ setWindowFullscreen }));
vi.mock('@/stores/ui', () => ({
  useUiStore: { getState: () => ({ applyWindowPresentation: applyPresentation }) },
}));

const normal: WindowPresentation = {
  mode: 'normal',
  isPortraitFull: false,
  isFrameless: false,
  isPinned: false,
  isFullscreen: false,
};
const mini: WindowPresentation = { ...normal, mode: 'mini' };

beforeEach(() => {
  vi.resetAllMocks();
  app.getWindowPresentation.mockResolvedValue(normal);
  setWindowFullscreen.mockResolvedValue(undefined);
});

it('restores mini layout from native state after the renderer reloads', async () => {
  app.getWindowPresentation.mockResolvedValue(mini);
  const { syncWindowPresentation } = await import('./window-presentation');
  await syncWindowPresentation();
  expect(applyPresentation).toHaveBeenCalledExactlyOnceWith(mini);
});

it('serializes enter and exit without publishing an unconfirmed layout', async () => {
  const entered = Promise.withResolvers<WindowPresentation>();
  app.enterMiniScreen.mockReturnValue(entered.promise);
  app.exitMiniScreen.mockResolvedValue(normal);
  const { enterMiniScreen, exitMiniScreen } = await import('./window-presentation');
  const entering = enterMiniScreen();
  const exiting = exitMiniScreen();
  await vi.waitFor(() => expect(app.enterMiniScreen).toHaveBeenCalledOnce());
  expect(app.exitMiniScreen).not.toHaveBeenCalled();
  expect(applyPresentation).not.toHaveBeenCalled();
  entered.resolve(mini);
  await Promise.all([entering, exiting]);
  expect(applyPresentation.mock.calls).toEqual([[mini], [normal]]);
});

it('resynchronizes after a failed native operation and still allows the next action', async () => {
  const failure = new Error('native resize failed');
  app.enterMiniScreen.mockRejectedValue(failure);
  app.exitMiniScreen.mockResolvedValue(normal);
  const { enterMiniScreen, exitMiniScreen } = await import('./window-presentation');
  await expect(enterMiniScreen()).rejects.toBe(failure);
  expect(applyPresentation).toHaveBeenCalledExactlyOnceWith(normal);
  await exitMiniScreen();
  expect(app.exitMiniScreen).toHaveBeenCalledOnce();
});

it('restores a mini window entered from frameless playback to normal presentation', async () => {
  app.getWindowPresentation.mockResolvedValueOnce({ ...mini, isFrameless: true });
  app.exitMiniScreen.mockResolvedValue({ ...normal, isFrameless: true });
  app.setTrafficLightsVisible.mockResolvedValue(undefined);
  const { restoreNormalWindow } = await import('./window-presentation');
  await restoreNormalWindow();
  expect(app.exitMiniScreen).toHaveBeenCalledOnce();
  expect(app.setTrafficLightsVisible).toHaveBeenCalledExactlyOnceWith(true);
  expect(app.exitMiniScreen.mock.invocationCallOrder[0]).toBeLessThan(
    app.setTrafficLightsVisible.mock.invocationCallOrder[0]!,
  );
  expect(applyPresentation).toHaveBeenCalledExactlyOnceWith(normal);
});

it('keeps mode changes in the same queue as an unfinished fullscreen exit', async () => {
  const exited = Promise.withResolvers<void>();
  setWindowFullscreen.mockReturnValueOnce(exited.promise);
  app.enterMiniScreen.mockResolvedValue(mini);
  const { applyWindowFullscreen, enterMiniScreen } = await import('./window-presentation');
  const fullscreenExit = applyWindowFullscreen(false);
  const enteringMini = enterMiniScreen();
  await vi.waitFor(() => expect(setWindowFullscreen).toHaveBeenCalledOnce());
  expect(app.enterMiniScreen).not.toHaveBeenCalled();
  exited.resolve();
  await Promise.all([fullscreenExit, enteringMini]);
  expect(app.enterMiniScreen).toHaveBeenCalledOnce();
  expect(applyPresentation.mock.calls).toEqual([[normal], [mini]]);
});
