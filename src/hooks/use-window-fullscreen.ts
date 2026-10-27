import { useCallback, useEffect } from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { toast } from 'sonner';
import { tf } from '@/locales';
import { useUiStore } from '@/stores/ui';

/**
 * 窗口级全屏：走原生
 * 窗口全屏，不走元素 requestFullscreen——WKWebView 里元素全屏只置 DOM 状态
 * （fullscreenElement 置位、promise 成功），原生呈现永不发生，窗口纹丝不动；
 * 就算能呈现，Portal 到 body 的浮层（倍速/清晰度菜单）在全屏元素子树外
 * 也不可见。窗口全屏这两个坑都不存在，切集重挂载（旧实现换成稳定祖先的
 * 缘由）更与全屏无关。
 *
 * store 只是窗口真值的镜像：applyWindowFullscreen 是唯一写入口（先调后端
 * 再落 store，usePinWindow 同款）；系统侧变化（Esc 退全屏、mac 绿按钮）
 * 经 resize 事件回流（useWindowFullscreenSync，挂应用壳一次）。
 */

/** 设定窗口全屏。后端失败时 store 不翻转，按钮图标不撒谎。 */
export async function applyWindowFullscreen(on: boolean): Promise<void> {
  await getCurrentWindow().setFullscreen(on);
  useUiStore.getState().setWindowFullscreen(on);
}

/** 全屏状态必然伴随窗口尺寸变化，resize 即对齐信号。 */
export function useWindowFullscreenSync() {
  useEffect(() => {
    const appWindow = getCurrentWindow();
    let disposed = false;
    let unlisten: (() => void) | undefined;
    const sync = () => {
      void appWindow.isFullscreen().then((v) => {
        if (!disposed) useUiStore.getState().setWindowFullscreen(v);
      });
    };
    sync();
    void appWindow
      .onResized(() => sync())
      .then((u) => {
        if (disposed) u();
        else unlisten = u;
      });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);
}

/** 播放器控制栏的全屏开关。失败弹 toast「全屏切换失败」
 *  ——静默失败会让用户以为按钮坏了。 */
export function useWindowFullscreen() {
  const fullscreen = useUiStore((s) => s.windowFullscreen);
  const toggleFullscreen = useCallback(() => {
    const next = !useUiStore.getState().windowFullscreen;
    void applyWindowFullscreen(next).catch((e: unknown) => {
      toast.error(tf('player.fullscreenFailed', { error: String(e) }));
    });
  }, []);
  return { fullscreen, toggleFullscreen };
}
