import { app } from '@/service/commands';
import type { WindowPresentation } from '@/service/schema/window';
import { useUiStore } from '@/stores/ui';
import { setWindowFullscreen } from './window-fullscreen-transition';

let pending = Promise.resolve();

function apply(presentation: WindowPresentation) {
  useUiStore.getState().applyWindowPresentation(presentation);
}

function transition(action: () => Promise<WindowPresentation>): Promise<void> {
  const next = pending.then(async () => {
    try {
      apply(await action());
    } catch (error) {
      apply(await app.getWindowPresentation());
      throw error;
    }
  });
  pending = next.then(
    () => undefined,
    () => undefined,
  );
  return next;
}

export function syncWindowPresentation(): Promise<void> {
  return transition(app.getWindowPresentation);
}

export function enterMiniScreen(): Promise<void> {
  return transition(async () => {
    await setWindowFullscreen(false);
    return app.enterMiniScreen();
  });
}

export function exitMiniScreen(): Promise<void> {
  return transition(app.exitMiniScreen);
}

export function enterPortraitScreen(aspect: number): Promise<void> {
  return transition(async () => {
    await setWindowFullscreen(false);
    return app.enterPortraitScreen(aspect);
  });
}

export function exitPortraitScreen(): Promise<void> {
  return transition(app.exitPortraitScreen);
}

export function setPortraitFull(aspect: number, full: boolean): Promise<void> {
  return transition(() => app.setPortraitFull(aspect, full));
}

export function restoreNormalWindow(): Promise<void> {
  return transition(async () => {
    const presentation = await app.getWindowPresentation();
    if (presentation.isFullscreen) await setWindowFullscreen(false);
    if (presentation.mode === 'mini') await app.exitMiniScreen();
    if (presentation.mode === 'portrait') await app.exitPortraitScreen();
    if (presentation.isFrameless) await app.setTrafficLightsVisible(true);
    return app.getWindowPresentation();
  });
}

export function applyWindowFullscreen(on: boolean): Promise<void> {
  return transition(async () => {
    await setWindowFullscreen(on);
    return app.getWindowPresentation();
  });
}
