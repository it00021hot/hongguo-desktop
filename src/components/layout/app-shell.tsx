import { useCallback, useEffect, useState } from 'react';
import { useRouterState } from '@tanstack/react-router';
import { invoke } from '@tauri-apps/api/core';
import { AppSidebar, NAV_ITEMS } from './app-sidebar';
import { ThemeSwitch } from './theme-switch';
import { WindowButtons } from './window-controls';
import { ConfirmDialog } from '@/components/ui/confirm-dialog';
import { useEvent } from '@/lib/ipc/events';
import { EVENTS } from '@/lib/ipc/types';
import { isMac } from '@/lib/platform';
import { t } from '@/i18n';

/** 由当前路径反查导航 key，用于顶栏标题。 */
function navKeyFor(pathname: string): string {
  if (pathname === '/') return 'browse';
  // 播放页不在侧栏菜单里（只能由播放入口跳转），标题单独指认
  if (pathname.startsWith('/player')) return 'player';
  const hit = NAV_ITEMS.find((item) => item.to !== '/' && pathname.startsWith(item.to));
  return hit?.key ?? 'browse';
}

export function AppShell({ children }: { children: React.ReactNode }) {
  const pathname = useRouterState({ select: (s) => s.location.pathname });
  const navKey = navKeyFor(pathname);
  // 沉浸流（/）：顶栏只留浮动的窗口按钮，56px 标题行还给画面
  const immersive = pathname === '/';
  // mac 的交通灯在侧边栏左上（见 app-sidebar），主区不重复给一套
  const mac = isMac();

  // 退出确认：后端把窗口关闭请求（自绘 ×/Alt+F4/任务栏）拦下来转成事件，
  // 这里弹框问一声——退出会掐断正在跑的下载任务，不该一碰就没了。
  // mark_window_ready 要等本组件挂载（监听就位）再调，后端才敢开始拦截。
  const [exitOpen, setExitOpen] = useState(false);
  useEffect(() => {
    void invoke('mark_window_ready').catch(() => {});
  }, []);
  useEvent(EVENTS.closeRequested, useCallback(() => setExitOpen(true), []));

  return (
    <div className="flex h-screen w-screen overflow-hidden">
      <a href="#content" className="skip-to-content">
        {t('common.skipToContent')}
      </a>

      <AppSidebar />

      <div className="relative flex min-w-0 flex-1 flex-col">
        {/* 窗口无边框，这行顶栏兼作拖拽区。按钮与主题切换在右侧成组，
            不额外占一行标题栏——应用名侧边栏顶部已经有了。

            ⚠️ 必须写 `deep`：Tauri 2.x 的 `data-tauri-drag-region` 裸属性**只认
            自己**，点子元素会被判成「不是拖拽区」而直接返回 false（见 tauri 的
            src/window/scripts/drag.js）。所以整棵子树都要能拖，就得显式写 deep。

            沉浸流（/）：标题整行撤掉，窗口按钮浮在画面右上——56px 还给画面。 */}
        {immersive ? (
          <header
            data-tauri-drag-region="deep"
            className="absolute right-0 top-0 z-50 flex h-14 items-center justify-end pr-2"
          >
            {/* 悬浮在视频画面上：无论应用是什么主题，这组按钮永远按暗色
                配色渲染（局部强制 dark token）——亮色主题下前景色是近黑，
                直接隐身在一帧黑画面/暗场景上（实测踩过）。text-foreground
                必须显式给：ghost 按钮没有自己的文字色，光有 .dark 变量
                不够——color 属性还是会从根节点继承亮色文字。 */}
            <div className="dark flex shrink-0 items-center text-foreground">
              <ThemeSwitch />
              {!mac && <WindowButtons />}
            </div>
          </header>
        ) : (
          <header
            data-tauri-drag-region="deep"
            className="flex h-14 shrink-0 items-center justify-between gap-4 border-b pl-6"
          >
            <h1 className="truncate text-base font-semibold">{t(`nav.${navKey}.title`)}</h1>
            <div className="flex shrink-0 items-center">
              <ThemeSwitch />
              {!mac && <WindowButtons />}
            </div>
          </header>
        )}

        <main id="content" className="min-h-0 flex-1 scrollbar-thin overflow-y-auto">
          {children}
        </main>
      </div>

      <ConfirmDialog
        open={exitOpen}
        onOpenChange={setExitOpen}
        title={t('window.exitTitle')}
        description={t('window.exitBody')}
        confirmLabel={t('window.exitConfirm')}
        onConfirm={() => void invoke('exit_app').catch(() => {})}
      />
    </div>
  );
}
