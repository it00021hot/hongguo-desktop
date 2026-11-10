import { useCallback, useEffect, useState } from 'react';
import { Pin } from 'lucide-react';
import { useLocation } from '@tanstack/react-router';
import { useQueryClient } from '@tanstack/react-query';
import { AppSidebar } from './app-sidebar';
import { BottomNav } from './bottom-nav';
import { ThemeSwitch } from './theme-switch';
import { TitlebarNav } from './titlebar-nav';
import { WindowButtons } from './window-controls';
import { ConfirmDialog } from '@/components/ui/confirm-dialog';
import { Button } from '@/components/ui/button';
import { useWindowFullscreenSync } from '@/hooks/use-window-fullscreen';
import { app as appApi } from '@/service/commands';
import { useEvent } from '@/service/tauri/events';
import { EVENTS } from '@/service/tauri/types';
import { useFollowNoticesInvalidator, keys } from '@/service/queries';
import { useUiStore } from '@/stores/ui';
import { usePinWindow } from '@/hooks/use-pin-window';
import { isMac } from '@/utils/platform';
import { setAndroidLandscape } from '@/utils/android-shell';
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
  // Android 返回到路由栈底：MainActivity 经前端桥（main.tsx）转来的退出
  // 意图，弹同一份确认框（桌面是关窗拦下来的同一入口）
  useEffect(() => {
    const onExit = () => setExitOpen(true);
    window.addEventListener('hongguo:android-exit', onExit);
    return () => window.removeEventListener('hongguo:android-exit', onExit);
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

  // 设备注册换档完成（启动自动注册/健康探针轮换）：旧设备身份拉过的缓存
  // 全部失效——顶栏 tab、推荐流、榜单（与 GPUI 壳的 load_tabs 对齐）。
  const queryClient = useQueryClient();
  useEvent(
    EVENTS.deviceRegistered,
    useCallback(() => {
      void queryClient.invalidateQueries({ queryKey: keys.feedTabs });
      void queryClient.invalidateQueries({ queryKey: keys.feedRoot });
      void queryClient.invalidateQueries({ queryKey: keys.rankRoot });
    }, [queryClient]),
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

  // 小屏播放：侧栏与顶栏**全部**
  // 藏掉，整个窗口只剩播放器（小屏窗口连标题栏都没有，窗口按钮由
  // 紧凑控制条承担）；拖拽由播放页的顶部拖拽条负责。
  const miniScreen = useUiStore((s) => s.miniScreen);
  // 竖屏播放：窗口收窄成竖条，侧栏藏掉——窄条里侧栏会占掉近 1/5 宽，
  // 窄条窗口也没有侧栏的余地；顶栏保留（分类 tab 与窗口钮照常）
  const portrait = useUiStore((s) => s.portrait);
  // 全屏真值对齐系统侧（Esc/绿按钮经 resize 回流 store）。沉浸流全屏时
  // 同样藏掉顶栏与侧栏——整窗只剩画面，与旧元素全屏同一视觉；
  // 其他页面被系统绿按钮全屏时界面照常。
  useWindowFullscreenSync();
  const windowFullscreen = useUiStore((s) => s.windowFullscreen);
  // 播放器只有首页沉浸流一个宿主（独立 /player 页已并回首页）
  const playerSurface = useLocation({
    select: (s) => s.pathname === '/',
  });
  const playerFullscreen = windowFullscreen && playerSurface;
  // 播放模式「窗口化无边框」：chrome 全藏
  // 但不进 OS 全屏，整窗只剩画面——与沉浸全屏同一视觉，窗口保持原几何。
  const frameless = useUiStore((s) => s.playerFrameless);
  const playerFrameless = frameless && playerSurface;
  // 竖屏只在沉浸流成立：离开即自动退出，
  // 窄条窗口配常规页面布局没法用（退出实现
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
      // 离开播放页时解开 Android 横屏锁（全屏观看中切走路由）
      setAndroidLandscape(false);
      void appApi.setTrafficLightsVisible(true).catch(() => undefined);
    }
  }, [playerSurface]);
  // 置顶开关与播放器（大屏顶栏/小屏紧凑条两个入口）共用同一份实现
  const { pinned, togglePinned } = usePinWindow();

  const showChrome = !miniScreen && !playerFullscreen && !playerFrameless;
  const showSidebar = showChrome && !portrait;

  return (
    <div className="flex h-full w-full flex-col overflow-hidden">
      <a href="#content" className="skip-to-content">
        {t('common.skipToContent')}
      </a>

      {/* 连贯顶栏：横贯全宽 44px；mac 左 84 给红绿灯（灯 origin x=16，三颗占 60，留 8 白）。
          顶栏钮一律 items-center（心 y=22）；红绿灯由 Rust 在获焦/缩放/显回时按
          y=floor((44-灯高)/2) 钉 origin，不在前端挤 padding。
          侧栏在顶栏下方。⚠️ drag-region 必须 deep：Tauri 2 裸属性只认自身节点 */}
      {showChrome && (
        <header
          data-tauri-drag-region="deep"
          className={cn(
            'bg-titlebar relative flex h-11 shrink-0 items-center gap-3 border-b text-[14px]',
            mac ? 'pr-4 pl-[84px]' : 'px-3',
          )}
        >
          <TitlebarNav />
          {!portrait && (
            <div
              id={TOP_BAR_SLOT_ID}
              className="pointer-events-none absolute inset-y-0 right-0 left-0 mx-auto flex w-fit items-center justify-center"
            />
          )}
          <div className="ml-auto flex h-full shrink-0 items-center gap-1">
            <Button
              variant="ghost"
              size="icon-sm"
              aria-label={t('player.pin')}
              title={t('player.pin')}
              onClick={togglePinned}
              className={cn('hidden size-7 md:inline-flex', pinned && 'text-primary')}
            >
              <Pin className={cn('size-4', pinned && 'fill-current')} />
            </Button>
            <div className="hidden h-full items-center md:flex">
              <ThemeSwitch />
            </div>
            {!mac && (
              <div className="hidden md:flex">
                <WindowButtons />
              </div>
            )}
          </div>
        </header>
      )}

      <div className="flex min-h-0 flex-1">
        {showSidebar && <AppSidebar />}
        <main id="content" className="bg-background min-h-0 flex-1 scrollbar-thin overflow-y-auto">
          {children}
        </main>
      </div>

      {!miniScreen && !playerFullscreen && !playerFrameless && !portrait && <BottomNav />}

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
