import { useCallback, useEffect, useState } from 'react';
import { Pin } from 'lucide-react';
import { useLocation } from '@tanstack/react-router';
import { AppSidebar } from './app-sidebar';
import { ThemeSwitch } from './theme-switch';
import { WindowButtons } from './window-controls';
import { ConfirmDialog } from '@/components/ui/confirm-dialog';
import { Button } from '@/components/ui/button';
import { useWindowFullscreenSync } from '@/hooks/use-window-fullscreen';
import { app as appApi } from '@/service/commands';
import { useEvent } from '@/service/tauri/events';
import { EVENTS } from '@/service/tauri/types';
import { useFollowNoticesInvalidator } from '@/service/queries';
import { useUiStore } from '@/stores/ui';
import { usePinWindow } from '@/hooks/use-pin-window';
import { isMac } from '@/utils/platform';
import { t } from '@/locales';
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
    void appApi.markWindowReady().catch(() => {});
  }, []);
  useEvent(
    EVENTS.closeRequested,
    useCallback(() => setExitOpen(true), []),
  );

  // 追更更新提醒到达（后端定时检查出新集）：弹 **OS 系统通知**（非应用内
  // toast）+ 刷新角标/列表缓存 + 出新集剧的分集缓存（档案已随通知刷新）。
  // 弹出开关（aibotPopupOn）控制是否弹。
  const refreshFollow = useFollowNoticesInvalidator();
  const aibotPopupOn = useUiStore((s) => s.aibotPopupOn);
  useEvent(
    EVENTS.followNotices,
    useCallback(
      (payload) => {
        refreshFollow(payload.notices);
        if (!aibotPopupOn) return;
        for (const n of payload.notices) {
          try {
            const nn = new Notification(n.title, { body: n.detail });
            nn.onclick = () => window.focus();
          } catch {
            // Notification 构造失败（权限等）静默——列表里仍有记录
          }
        }
      },
      [refreshFollow, aibotPopupOn],
    ),
  );

  // 老板键隐藏/恢复：隐藏即暂停全部视频（声音不漏，老板键的意义所在），
  // 恢复窗口时续播刚才在播的。video 元素只活在播放器页，全局兜底查询即可。
  useEvent(
    EVENTS.bossKeyHidden,
    useCallback(({ hidden }) => {
      document.querySelectorAll('video').forEach((v) => {
        if (hidden) {
          v.pause();
        } else if (v.paused) {
          void v.play().catch(() => undefined);
        }
      });
    }, []),
  );

  // 小屏播放（对齐 hgplayer De.mini 的 bare 布局）：侧栏与顶栏**全部**
  // 藏掉，整个窗口只剩播放器（第三方小屏连标题栏都没有，窗口按钮由
  // 紧凑控制条承担）；拖拽由播放页的顶部拖拽条负责。
  const miniScreen = useUiStore((s) => s.miniScreen);
  // 竖屏播放：窗口收窄成竖条，侧栏藏掉——窄条里侧栏会占掉近 1/5 宽，
  // 第三方竖屏（hgplayer）也没有侧栏；顶栏保留（分类 tab 与窗口钮照常）
  const portrait = useUiStore((s) => s.portrait);
  // 全屏真值对齐系统侧（Esc/绿按钮经 resize 回流 store）。沉浸流全屏时
  // 同样藏掉顶栏与侧栏——整窗只剩画面，对齐 hgplayer 与旧元素全屏的视觉；
  // 其他页面被系统绿按钮全屏时界面照常。
  useWindowFullscreenSync();
  const windowFullscreen = useUiStore((s) => s.windowFullscreen);
  // 播放器只有首页沉浸流一个宿主（独立 /player 页已并回首页）
  const playerSurface = useLocation({
    select: (s) => s.pathname === '/',
  });
  const playerFullscreen = windowFullscreen && playerSurface;
  // 播放模式「窗口化无边框」（hgplayer 播放页 bare 布局同款）：chrome 全藏
  // 但不进 OS 全屏，整窗只剩画面——与沉浸全屏同一视觉，窗口保持原几何。
  const frameless = useUiStore((s) => s.playerFrameless);
  const playerFrameless = frameless && playerSurface;
  // 竖屏只在沉浸流成立：离开即自动退出，
  // 窄条窗口配常规页面布局没法用（hgplayer 路由监听同款语义；退出实现
  // 收在 features/player 的 use-portrait-window，这里只做路由侧触发）
  useEffect(() => {
    if (!playerSurface && useUiStore.getState().portrait) {
      useUiStore.getState().setPortrait(false);
      void appApi.exitPortraitScreen().catch(() => undefined);
    }
  }, [playerSurface]);
  // 无边框同竖屏：只在播放 surface 成立，离开即退出并把 macOS 红绿灯
  // 显回（chrome 条件翻面就回不来了，必须在这里收尾）
  useEffect(() => {
    if (!playerSurface && useUiStore.getState().playerFrameless) {
      useUiStore.getState().setPlayerFrameless(false);
      void appApi.setTrafficLightsVisible(true).catch(() => undefined);
    }
  }, [playerSurface]);
  // 置顶开关与播放器（大屏顶栏/小屏紧凑条两个入口）共用同一份实现
  const { pinned, togglePinned } = usePinWindow();

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
      {!miniScreen && !playerFullscreen && !playerFrameless && (
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
                tab 内容自己在 portal wrapper 里开 auto。

                竖屏时不渲染插槽：窄条（~390px）里整排 tab 与右侧置顶/
                主题/窗口钮必然叠压（绝对定位 w-fit 不收缩）；竖屏=沉浸
                看剧，切 tab 退出竖屏再说。容器缺席时 portal 侧
                useSyncExternalStore 自动渲染 null，退出竖屏容器回来
                MutationObserver 即时跟上，tab 无需自己感知竖屏。 */}
          {!portrait && (
            <div
              id={TOP_BAR_SLOT_ID}
              className="pointer-events-none absolute inset-x-0 mx-auto flex w-fit items-center justify-center"
            />
          )}
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
        {!miniScreen && !playerFullscreen && !playerFrameless && !portrait && <AppSidebar />}

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
        onConfirm={() => void appApi.exitApp().catch(() => {})}
      />
    </div>
  );
}
