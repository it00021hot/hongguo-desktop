import { useEffect, useState } from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { Minus, Square, Copy, X } from 'lucide-react';
import { t } from '@/locales';
import { cn } from '@/lib/utils';

/**
 * 窗口控制按钮组（Windows / Linux 专用，`decorations: false` 后系统白条不再画）。
 * macOS 不走这里——自绘红绿灯会丢掉原生能力（绿键菜单/双击缩放/拖拽手感），mac 用原生红绿灯。
 * ⚠️ 平台配置合并是 RFC 7396（数组整体替换不深合并）：改 tauri.conf.json 窗口字段时须同步 tauri.macos.conf.json。
 */

/** 最大化状态。系统侧的变化（拖边缘、Win+↑、任务栏点击）也要跟上。 */
function useMaximized(): boolean {
  const appWindow = getCurrentWindow();
  const [maximized, setMaximized] = useState(false);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;

    void appWindow.isMaximized().then(setMaximized);
    void appWindow
      .onResized(() => {
        void appWindow.isMaximized().then(setMaximized);
      })
      .then((fn) => {
        if (cancelled) fn();
        else unlisten = fn;
      });

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [appWindow]);

  return maximized;
}

/** Windows / Linux 风格：方形按钮，靠右排列。 */
export function WindowButtons() {
  const appWindow = getCurrentWindow();
  const maximized = useMaximized();

  return (
    <div className="flex shrink-0 items-center">
      <WinButton label={t('window.minimize')} onClick={() => void appWindow.minimize()}>
        <Minus className="size-4" />
      </WinButton>
      <WinButton
        label={maximized ? t('window.restore') : t('window.maximize')}
        onClick={() => void appWindow.toggleMaximize()}
      >
        {maximized ? <Copy className="size-3.5" /> : <Square className="size-3.5" />}
      </WinButton>
      <WinButton label={t('window.close')} onClick={() => void appWindow.close()} danger>
        <X className="size-4" />
      </WinButton>
    </div>
  );
}

interface WinButtonProps {
  label: string;
  onClick: () => void;
  /** 关闭键：hover 铺满红底，Windows 惯例 */
  danger?: boolean;
  children: React.ReactNode;
}

/** Windows 风格窗口按钮。刻意不设 data-tauri-drag-region，否则点击会被拖拽吞掉。 */
function WinButton({ label, onClick, danger, children }: WinButtonProps) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      onClick={onClick}
      className={cn(
        'text-foreground/80 hover:bg-accent hover:text-accent-foreground grid size-10 place-items-center transition-colors',
        danger && 'hover:bg-destructive hover:text-destructive-foreground',
      )}
    >
      {children}
    </button>
  );
}
