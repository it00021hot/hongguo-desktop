import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { app } from '@/service/commands';
import { EVENTS } from '@/service/tauri/types';
import { isMac } from '@/utils/platform';
import { t } from '@/locales';

const FULLSCREEN_EXIT_TIMEOUT_MS = 10_000;

export async function setWindowFullscreen(on: boolean): Promise<void> {
  const nativeWindow = getCurrentWindow();
  if (on || !isMac()) {
    await nativeWindow.setFullscreen(on);
    return;
  }
  if (!(await app.getWindowPresentation()).isFullscreen) return;
  const exited = Promise.withResolvers<void>();
  const unlisten = await listen(EVENTS.windowFullscreenExited, (event) => {
    if (event.payload === null) exited.resolve();
  });
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    if (!(await app.getWindowPresentation()).isFullscreen) return;
    timer = setTimeout(
      () => exited.reject(new Error(t('player.fullscreenExitTimeout'))),
      FULLSCREEN_EXIT_TIMEOUT_MS,
    );
    await Promise.all([nativeWindow.setFullscreen(false), exited.promise]);
  } finally {
    clearTimeout(timer);
    unlisten();
  }
}
