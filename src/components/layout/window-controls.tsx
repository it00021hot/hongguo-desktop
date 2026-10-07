import { useEffect, useState } from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { Minus, Square, Copy, X } from 'lucide-react';
import { t } from '@/i18n';
import { cn } from '@/lib/utils';

/**
 * 窗口控制按钮组。
 *
 * 窗口已是 `decorations: false`（见 tauri.conf.json），系统白条不再画，
 * 缩小/最大化/关闭由这里提供。
 *
 * **不单独占一行**：无处边框下再压一条只写着应用名的标题栏纯属浪费高度，
 * 而应用名侧边栏顶部已经有一份。所以两组按钮分别寄生在既有的结构上——
 * Windows 挂主区顶栏右端，macOS 交通灯挂侧边栏左上角——
 * 拖拽区则由这两处宿主的父容器提供（见 app-shell / app-sidebar）。
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

/**
 * macOS 交通灯：三个圆点，顺序为 关闭 / 最小 / 全屏。
 *
 * 挂在侧边栏左上角而不是主区：mac 的交通灯历来在窗口左上，
 * 放到内容区右侧既不符合平台习惯，也会和页面内的按钮抢位置。
 */
export function MacTrafficLights() {
  const appWindow = getCurrentWindow();
  const maximized = useMaximized();
  const [hover, setHover] = useState<string | null>(null);

  return (
    <div
      className="flex shrink-0 items-center gap-2"
      onMouseEnter={() => setHover('bar')}
      onMouseLeave={() => setHover(null)}
    >
      <MacDot
        label={t('window.close')}
        color="bg-[#ff5f57]"
        active={hover === 'close'}
        onClick={() => void appWindow.close()}
        glyph="×"
      />
      <MacDot
        label={t('window.minimize')}
        color="bg-[#febc2e]"
        active={hover === 'min'}
        onClick={() => void appWindow.minimize()}
        glyph="−"
      />
      <MacDot
        label={maximized ? t('window.restore') : t('window.zoom')}
        color="bg-[#28c840]"
        active={hover === 'zoom'}
        onClick={() => void appWindow.toggleMaximize()}
        glyph="+"
      />
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

interface MacDotProps {
  /** 无障碍标签与 tooltip，走 i18n */
  label: string;
  /** 交通灯的固定色，三键靠色相区分 */
  color: string;
  /** hover 到本键时才显示符号 */
  active: boolean;
  onClick: () => void;
  /** hover 时浮出的符号。与 label 分开：文案要翻译，符号是固定字形 */
  glyph: string;
}

/** macOS 交通灯：纯色圆点，hover 才浮出符号。 */
function MacDot({ label, color, active, onClick, glyph }: MacDotProps) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      onClick={onClick}
      className={cn(
        'grid size-3 place-items-center rounded-full',
        // 背景色必须显式给：写成 currentColor 会取到文字色，圆点直接变黑
        color,
        active && 'brightness-95',
      )}
    >
      {/* 符号常驻 DOM，靠 opacity 控制显隐：hover 切换才不会重新挂载节点 */}
      <span
        className={cn(
          'text-[9px] leading-none font-bold text-black/60 transition-opacity',
          active ? 'opacity-100' : 'opacity-0',
        )}
      >
        {glyph}
      </span>
    </button>
  );
}
