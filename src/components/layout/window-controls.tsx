import { useEffect, useState } from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { Minus, Square, Copy, X } from 'lucide-react';
import { t } from '@/locales';
import { cn } from '@/lib/utils';

/**
 * 窗口控制按钮组（Windows / Linux 专用）。
 *
 * 这两个平台窗口是 `decorations: false`（见 tauri.conf.json），系统白条
 * 不再画，缩小/最大化/关闭由这里提供。
 *
 * **macOS 不走这里**：自绘红绿灯会把原生能力全部丢掉——绿键 hover 的
 * 「移动并调整大小 / 填充与排列 / 全屏」菜单、标题栏双击缩放、原生的
 * 拖拽手感和圆角阴影，都是圆点按钮画不出来的。mac 的窗口是原生红绿灯
 * （tauri.macos.conf.json：decorations + titleBarStyle Overlay +
 * hiddenTitle），小屏时由后端摘掉再还原（见 app_cmd 的 enter/exit_mini_screen）。
 *
 * Windows 组**不单独占一行**：无边框下再压一条只写着应用名的标题栏
 * 纯属浪费高度，应用名侧边栏顶部已经有一份。按钮寄生在顶栏右端
 * （见 app-shell），拖拽区由宿主的父容器提供。
 *
 * ⚠️ 平台配置合并是 RFC 7396（json_patch::merge）：**数组整体替换、
 * 不按下标深合并**。所以 tauri.macos.conf.json 的 windows 数组必须带
 * 完整窗口定义；改 tauri.conf.json 的窗口字段时要同步过去，否则 mac
 * 上会静默回退旧值。
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

/**
 * Windows 风格窗口按钮。
 *
 * 刻意不设 `data-tauri-drag-region`：宿主的顶栏有拖拽区，
 * 但按钮必须排除，否则点击会被拖拽吞掉，关闭键点不动。
 */
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
