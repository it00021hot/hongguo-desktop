import { useCallback, useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Pin } from 'lucide-react';
import { AppSidebar } from './app-sidebar';
import { ThemeSwitch } from './theme-switch';
import { WindowButtons } from './window-controls';
import { ConfirmDialog } from '@/components/ui/confirm-dialog';
import { Button } from '@/components/ui/button';
import { useEvent } from '@/service/tauri/events';
import { EVENTS } from '@/service/tauri/types';
import { useUiStore } from '@/lib/stores/ui';
import { app as appApi } from '@/service/commands';
import { isMac } from '@/lib/platform';
import { t } from '@/i18n';
import { cn } from '@/lib/utils';

/**
 * 顶栏中部插槽的定位 id：沉浸流/排行榜/新剧把各自的分类 tab 栏 portal
 * 进来（顶栏常驻薄行形态）。其他页面顶部不显示菜单名——导航位置由
 * 侧边栏高亮表达，顶栏中部留空。
 */
export const TOP_BAR_SLOT_ID = 'topbar-slot';

export function AppShell({ children }: { children: React.ReactNode }) {
  // mac 用原生红绿灯（tauri.macos.conf.json：Overlay + hiddenTitle），
  // 顶栏给它们让出左端位置；其余平台自绘窗口按钮挂顶栏右侧
  const mac = isMac();

  // 退出确认：后端把窗口关闭请求（自绘 ×/Alt+F4/任务栏）拦下来转成事件，
  // 这里弹框问一声——退出会掐断正在跑的下载任务，不该一碰就没了。
  // mark_window_ready 要等本组件挂载（监听就位）再调，后端才敢开始拦截。
  const [exitOpen, setExitOpen] = useState(false);
  useEffect(() => {
    void invoke('mark_window_ready').catch(() => {});
  }, []);
  useEvent(
    EVENTS.closeRequested,
    useCallback(() => setExitOpen(true), []),
  );

  // 小屏播放（对齐 hgplayer De.mini 的 bare 布局）：侧栏与顶栏**全部**
  // 藏掉，整个窗口只剩播放器（第三方小屏连标题栏都没有，窗口按钮由
  // 紧凑控制条承担）；拖拽由播放页的顶部拖拽条负责。
  const miniScreen = useUiStore((s) => s.miniScreen);
  const pinned = useUiStore((s) => s.pinned);
  const setPinned = useUiStore((s) => s.setPinned);

  const togglePinned = useCallback(() => {
    const next = !pinned;
    void appApi
      .setAlwaysOnTop(next)
      .then(() => setPinned(next))
      .catch(() => undefined);
  }, [pinned, setPinned]);

  return (
    <div className="flex h-screen w-screen flex-col overflow-hidden">
      <a href="#content" className="skip-to-content">
        {t('common.skipToContent')}
      </a>

      {/* 横贯全宽的顶栏（hgplayer 同款形态，一条 44px 薄行）：原生红绿灯
            （mac）/logo/应用名在左，中部是各页 portal 进来的分类 tab（无则
            留空，页面标题不在此显示——导航位置由侧栏高亮表达），置顶/主题/
            语言/窗口控件钉在右端。
            小屏播放整个顶栏不渲染——第三方小屏是纯播放器，窗口钮/拖拽
            由紧凑控制条与播放页拖拽条承担（mac 的原生红绿灯也由后端在
            enter_mini_screen 里一并摘掉）。

            顶栏在侧边栏**上方**而不是长在侧边栏里——侧边栏折叠不影响顶部，
            播放/沉浸流时控件也固定在顶栏，不会被弹幕或画面内容盖住。

            mac 左内边距给原生红绿灯让位：trafficLightPosition x=12 起排，
            三个 12pt 圆点 + 8pt 间距到 ~64px 收尾，76px 起排 logo 不贴不挤。

            ⚠️ 必须写 `deep`：Tauri 2.x 的 `data-tauri-drag-region` 裸属性**只认
            自己**，点子元素会被判成「不是拖拽区」而直接返回 false（见 tauri 的
            src/window/scripts/drag.js）。所以整棵子树都要能拖，就得显式写 deep。 */}
      {!miniScreen && (
        <header
          data-tauri-drag-region="deep"
          className={cn(
            'bg-sidebar relative flex h-11 shrink-0 items-center gap-3 border-b',
            mac ? 'pr-3 pl-[76px]' : 'px-3',
          )}
        >
          {/* 中部插槽：各页 portal 进来的分类 tab。绝对定位真居中——
                左右组宽度不等（红绿灯位 vs 控件），flex-1 的「剩余空间
                居中」会明显偏右。inset-x-0 + mx-auto + w-fit 居中且不用
                transform（半像素平移会让文字发糊）。
                容器 pointer-events-none：空白带不拦截、仍可拖窗；
                tab 内容自己在 portal wrapper 里开 auto */}
          <div
            id={TOP_BAR_SLOT_ID}
            className="pointer-events-none absolute inset-x-0 mx-auto flex w-fit items-center justify-center"
          />
          <div className="ml-auto flex shrink-0 items-center gap-1">
            {/* 置顶（hgplayer 标题栏同款）：钉住/取消整个窗口 */}
            <Button
              variant="ghost"
              size="icon"
              aria-label={t('player.pin')}
              title={t('player.pin')}
              onClick={togglePinned}
              className={cn(pinned && 'text-primary')}
            >
              <Pin className={cn('size-4', pinned && 'fill-current')} />
            </Button>
            <ThemeSwitch />
            {!mac && <WindowButtons />}
          </div>
        </header>
      )}

      <div className="flex min-h-0 flex-1">
        {!miniScreen && <AppSidebar />}

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
