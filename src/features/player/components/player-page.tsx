import { useCallback, useEffect, useRef, useState } from 'react';
import { useNavigate } from '@tanstack/react-router';
import { Loader2, MonitorPlay } from 'lucide-react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Progress } from '@/components/ui/progress';
import { DanmakuLayer } from './danmaku-layer';
import { PlayerControls } from './controls/player-controls';
import { MiniScreenControls } from './mini-screen-controls';
import { InteractionRail } from './interaction-rail';
import { CommentPanel } from './comment-panel';
import {
  useDanmaku,
  useSeriesDetailMeta,
  useSeriesEpisodes,
  useSettings,
  useStorageActions,
  useWatchHistory,
} from '@/service/queries';
import { usePlayerStore } from '@/stores/player';
import { useUiStore } from '@/stores/ui';
import { readLastTarget } from '@/utils/playback-prefs';
import { t, tf } from '@/locales';
import { cn } from '@/lib/utils';
import { app as appApi } from '@/service/commands';
import { useIncognitoMode } from './incognito';
import { formatBytes } from '@/utils/format';
import { usePlaybackSource } from '../hooks/use-playback-source';
import { useTranscodeFallback } from '../hooks/use-transcode-fallback';
import { usePlaybackProgress } from '../hooks/use-playback-progress';
import { useDanmakuSettings } from '../hooks/use-danmaku-settings';
import { useBingeRelay } from '../hooks/use-binge-relay';
import { usePlayerOverlay } from '../hooks/use-player-overlay';

export function PlayerPage() {
  const navigate = useNavigate();
  const seriesId = usePlayerStore((s) => s.seriesId);
  const vidIndex = usePlayerStore((s) => s.vidIndex);
  const setTarget = usePlayerStore((s) => s.setTarget);
  // 封面占位图：历史页带封面进来，取流间隙不至于黑屏（信息流入口由 HomePage 自带）
  const { data: history } = useWatchHistory();
  const coverUrl = history?.items.find((i) => i.seriesId === seriesId)?.cover;
  // 弹幕设置面板的开合放在这一层：切集时 PlayerView 整体重挂载，
  // 面板状态在这里才不会一集一开就被吃掉

  // 音量竖条浮层同样在这层持有：切集重挂载不会把正开着的浮层收走

  // 刷新/重启后内存 store 是空的：把上次播放目标读回来，
  // 播放器直接续播（进度由本地播放档案的 resumeAt 接上）。
  // 连上次播放目标都没有（首次启动/清过记录）：直接进推荐沉浸流——
  // 打开就能播（第三方同款），而不是摆一个还要自己去找剧的空态；
  // 推荐流里 ↑↓/滚轮 = 切剧，从详情/历史**选定**剧进来才是 ↑↓ = 切集。
  useEffect(() => {
    if (seriesId) return;
    const last = readLastTarget();
    if (last) {
      setTarget(last.seriesId, last.vidIndex);
      return;
    }
    void navigate({ to: '/' });
  }, [seriesId, setTarget, navigate]);

  // 兜底空态：正常只在重定向生效前闪一帧
  if (!seriesId || !vidIndex) {
    return <PlayerEmptyState />;
  }

  // key 随剧集变化 → 切集时组件整体重建，播放/转码状态自然清零，
  // 不必在 effect 里同步 setState（那会触发级联渲染）。
  return <PlayerView key={`${seriesId}:${vidIndex}`} coverUrl={coverUrl} />;
}

/** 未在播放时的占位：去推荐/历史挑一部剧即可开始。 */
function PlayerEmptyState() {
  const navigate = useNavigate();
  return (
    <div className="text-muted-foreground grid h-full place-items-center p-6">
      <div className="flex flex-col items-center gap-3 text-center">
        <MonitorPlay className="size-10 opacity-40" aria-hidden />
        <p className="text-sm">{t('player.empty')}</p>
        <p className="text-xs opacity-70">{t('player.emptyHint')}</p>
        <div className="mt-2 flex gap-2">
          <Button size="sm" variant="outline" onClick={() => void navigate({ to: '/history' })}>
            {t('nav.history.title')}
          </Button>
          <Button size="sm" variant="outline" onClick={() => void navigate({ to: '/' })}>
            {t('nav.home.title')}
          </Button>
        </div>
      </div>
    </div>
  );
}

export function PlayerView({
  onWheelStep,
  coverUrl,
  overlayMeta,
}: {
  /**
   * 滚轮/↑↓ 的语义由宿主页给：首页（沉浸流）是切**上一部/下一部剧**，
   * 不给则回落为切上一集/下一集（从历史/收藏等页进入时）。
   */
  onWheelStep?: (dir: 1 | -1) => void;
  /**
   * 封面占位图（信息流条目/历史记录都有）：切剧/起播的取流间隙垫在画面上，
   * 首帧真正出画（playing）后交叉淡出——第三方「滚动秒切」的观感一半来自这里，
   * 黑屏等待变成封面常驻，感知延迟只剩取流本身。
   */
  coverUrl?: string;
  /**
   * 信息流条目自带的展示标记（热度文本/季角标/运营角标，档案接口没有
   * 这几个字段，由宿主页按当前 seriesId 从流条目里挑出来递进来）。
   * 热度在剧名上方，角标在剧名前。
   */
  overlayMeta?: { heatText?: string; seasonTag?: string; badge?: string };
}) {
  const navigate = useNavigate();
  const seriesPanelOpen = usePlayerStore((s) => s.seriesPanelOpen);
  const commentPanelOpen = usePlayerStore((s) => s.commentPanelOpen);
  const danmakuPanelOpen = usePlayerStore((s) => s.danmakuPanelOpen);
  const volumeOpen = usePlayerStore((s) => s.volumeOpen);
  const setCommentPanelOpen = usePlayerStore((s) => s.setCommentPanelOpen);
  const setDanmakuPanelOpen = usePlayerStore((s) => s.setDanmakuPanelOpen);
  const setVolumeOpen = usePlayerStore((s) => s.setVolumeOpen);
  const setSeriesPanelOpen = usePlayerStore((s) => s.setSeriesPanelOpen);
  const videoRef = useRef<HTMLVideoElement>(null);
  const stageRef = useRef<HTMLDivElement>(null);
  /**
   * 最近一次切换的方向（1=下一个/向下滚，-1=上一个）。滚轮/↑↓/连播在
   * 触发切换时记下，流地址变化的那次渲染据此决定新内容从下边还是上边
   * 滑入——抖音式「内容跟手」的过渡感全靠这一个方向的符号。
   * 事件写 state 而不是 ref：切换动作到流交换之间必然隔至少一次渲染，
   * 渲染期读到的一定是本次切换的方向（渲染期读 ref 会被 react-hooks 拦）。
   */
  const [slideDir, setSlideDir] = useState<1 | -1>(1);
  const seriesId = usePlayerStore((s) => s.seriesId);
  const vidIndex = usePlayerStore((s) => s.vidIndex);
  const setTarget = usePlayerStore((s) => s.setTarget);
  const episodeKey = seriesId && vidIndex ? `${seriesId}:${vidIndex}` : '';

  const { data: settings } = useSettings();
  const { deleteEpisode } = useStorageActions();
  // 设置没加载完时按默认值走：连播默认开、看完自动删默认关（与后端 Settings::default 一致）
  const autoNext = settings?.autoNextEpisode ?? true;
  const autoDelete = settings?.autoDeleteAfterPlay ?? false;

  // 取流/起播/清晰度切换/断供重试/取流进度订阅与媒体偏好贴回，抽在
  // use-playback-source；srcKeyRef/lastKnownRef 两把竞态防御 ref 的真值
  // 在那边持有，这里转手给进度持久化与兜底转码共用。
  const {
    src,
    setSrc,
    error,
    setError,
    retryTick,
    srcKey,
    setSrcKey,
    srcKeyRef,
    lastKnownRef,
    buffering,
    setDefinition,
    activeDefinition,
    definitions,
    retryOnline,
    handleLoadedMetadata,
    handleRateChange,
    handleVolumeChange,
  } = usePlaybackSource({ videoRef, seriesId, vidIndex, episodeKey });

  /** 下载面板是否打开。开着时不让连播把这一集换掉。 */
  const [downloading, setDownloading] = useState(false);

  // 选集与「下载到本地」都要完整分集表，从剧集档案直接取
  const { data: currentSeries } = useSeriesEpisodes(seriesId);
  // 简介只在沉浸流信息叠加里用（播放页右侧面板自己拉）：
  // video_detail 接口的 series_intro，官方 App 详情页同源
  const { data: meta } = useSeriesDetailMeta(seriesId ?? '');

  // 弹幕：vid 来自剧集档案的分集表，换集自动换一份缓存
  const currentVid = currentSeries?.episodes.find((e) => e.vidIndex === vidIndex)?.vid ?? '';
  // 当前集的公开计数（detail 接口下发；右栏 ♥/💬 数字）
  const currentEpisode = currentSeries?.episodes.find((e) => e.vidIndex === vidIndex);
  const danmakuQuery = useDanmaku(currentVid ? `${currentVid}:${seriesId}` : '');
  const { danmakuOn, danmakuDisplay, updateDanmakuDisplay, toggleDanmaku } = useDanmakuSettings();
  // 弹幕拉取失败不能静默：画面照常播，但用户该知道弹幕为什么没了
  useEffect(() => {
    if (danmakuQuery.isError) {
      toast.error(tf('player.danmakuFailed', { reason: String(danmakuQuery.error) }));
    }
  }, [danmakuQuery.isError, danmakuQuery.error]);

  // 转码兜底与最终供流地址裁决抽在 use-transcode-fallback：
  // videoWidth==0 探测 + H.264 转码轮询 + 重试防缓存钉。
  const { compatSrc, compat, playSrc, streamKey } = useTranscodeFallback({
    videoRef,
    seriesId,
    vidIndex,
    episodeKey,
    error,
    src,
    retryTick,
    srcKeyRef,
    setSrcKey,
    currentSeries,
  });
  /** 切换在途：目标已是新的一集，元素里还是上一条流（play 请求未返回） */
  const switching = srcKey !== episodeKey;

  // 剧终三级接力（下一季→宿主推荐/猜你喜欢）与连播锁定抽在
  // use-binge-relay；setSlideDir 是「内容从哪边滑入」的方向信号，
  // 滚轮/键盘/接力共同写入，state 留在本层给过渡动画读。
  const { inBinge, setBinge, advanceAfterSeriesEnd, stepEpisode } = useBingeRelay({
    seriesId,
    vidIndex,
    setTarget,
    setSlideDir,
    downloading,
    onWheelStep,
    currentSeries,
  });

  const miniScreen = useUiStore((s) => s.miniScreen);
  const setMiniScreen = useUiStore((s) => s.setMiniScreen);

  // ---- 隐身模式（与控制栏的 Eye 按钮共享状态）：鼠标离开窗口即
  //      整窗透明 + 暂停，鼠标回来恢复显示（见 incognito.ts 的机制说明） ----
  const incognito = useIncognitoMode(videoRef);

  // ---- 沉浸流悬浮层显隐状态机（B站方案）抽在 use-player-overlay：
  //      3s 倒计时 / 暂停常显 / 控件悬停不收在这里统一裁决
  const { paused, setPaused, chromeShown, wakeChrome, hideChrome, setControlsHovered } =
    usePlayerOverlay({
      seriesPanelOpen,
      commentPanelOpen,
      danmakuPanelOpen,
      volumeOpen,
    });

  /** 简介展开态：切剧重挂载自然收回。 */
  const [introExpanded, setIntroExpanded] = useState(false);
  /**
   * 当前流是否已出画 / 正在卡顿。两条状态都带流指纹（streamKey）：
   * 信息流的 PlayerView 是复用的（不随切剧重建），不带指纹的话上一部剧
   * 留下的「已出画」会原样漏给下一部——切剧黑屏期间加载胶囊和封面占位
   * 全被跳过，正是「切剧=纯黑屏、一点反应都没有」的来源。
   * 读时校验指纹：换流即失效，不用在 effect 里追着清 state。
   */
  const [live, setLive] = useState<{ key: string; on: boolean }>({ key: '', on: false });
  const [stalled, setStalled] = useState<{ key: string; on: boolean }>({ key: '', on: false });
  const videoLive = live.key === streamKey && live.on;
  /** waiting 后时间轴恢复推进（timeupdate）或重新出画即视为不卡 */
  const videoStalled = stalled.key === streamKey && stalled.on;

  // ---- 滚轮切换（hgplayer 同款）：沉浸流=上一部/下一部剧，播放页=切集 ----
  // 选集浮层/下载面板打开时不抢滚动；冷却 400ms 防一次惯性滚动连跳。
  const wheelLock = useRef(0);
  const onStageWheel = useCallback(
    (e: React.WheelEvent) => {
      // 浮层（评论面板/选集/弹幕设置/倍速清晰度菜单）里的滚动是它自己在滚，
      // 不冒泡成「切集」。控制栏与面板用 data-wheel-block 标记；Radix 菜单
      // portal 到 body，真实 DOM 里不是舞台子孙，必须各自带标记才拦得住。
      if ((e.target as HTMLElement | null)?.closest?.('[data-wheel-block]')) return;
      // 滚轮也是「用户在场」：切剧/切集时唤醒悬浮层，让新一部的信息亮 3 秒
      wakeChrome();
      if (seriesPanelOpen || downloading) return;
      const now = Date.now();
      if (now - wheelLock.current < 400 || Math.abs(e.deltaY) < 15) return;
      wheelLock.current = now;
      const dir: 1 | -1 = e.deltaY > 0 ? 1 : -1;
      setSlideDir(dir);
      // 连播锁定时滚轮语义变为切集，不跟随宿主页换剧
      if (inBinge) stepEpisode(dir);
      else if (onWheelStep) onWheelStep(dir);
      else stepEpisode(dir);
    },
    [seriesPanelOpen, downloading, onWheelStep, stepEpisode, wakeChrome, inBinge],
  );

  // ---- 点击画面：信息流里=选中本剧（进入切集模式）；选中后/播放页=播放/暂停 ----
  // 第三方同款交互：未选中时单击视频=「选中这部剧」，此后滚轮/↑↓ 切集，
  // Esc 退出选中回到换剧；选中状态下的单击回归传统的播放/暂停。
  // 控件/面板/互动栏（data-wheel-block 标记区）里的点击是它们自己的事，
  // 不冒泡成选中/暂停。
  const onStageClick = useCallback(
    (e: React.MouseEvent) => {
      if ((e.target as HTMLElement | null)?.closest?.('[data-wheel-block]')) return;
      wakeChrome();
      if (onWheelStep && !inBinge) {
        setBinge(seriesId);
        return;
      }
      const video = videoRef.current;
      if (!video) return;
      if (video.paused) void video.play().catch(() => undefined);
      else video.pause();
    },
    [wakeChrome, onWheelStep, inBinge, setBinge, seriesId],
  );

  // 进度持久化（5s 节流落盘 + 云端上报节拍 + 卸载补写）抽在
  // use-playback-progress；lastKnownRef/srcKeyRef 两把 ref 是它和取流侧
  // （起播续点、startCompat 认流）共用的竞态防御，真值仍在这层持有。
  const { persist } = usePlaybackProgress({
    videoRef,
    seriesId,
    vidIndex,
    episodeKey,
    srcKeyRef,
    lastKnownRef,
    currentVid,
    currentSeries,
  });

  // ---- 小屏播放（对齐 hgplayer ng()/Op()）：同一窗口缩成 480×270 落 ----
  //      到屏幕右下角，侧栏隐藏、控件换紧凑条——video 元素原地不动，
  //      播放零中断（不暂停、不落库、不换页）

  const enterMini = useCallback(() => {
    // 大屏的浮层面板带不进 480×270 的小窗：进小屏前一并收掉
    setCommentPanelOpen(false);
    setDanmakuPanelOpen(false);
    setVolumeOpen(false);
    setSeriesPanelOpen(false);
    // 悬停态跟着旧控制栏一起卸载：组件卸载不触发 mouseleave，残留 true 会让
    // 小屏里鼠标已经离开画面、控制条却常显不收
    setControlsHovered(false);
    setMiniScreen(true);
    void appApi.enterMiniScreen().catch((e: Error) => toast.error(e.message));
    // 信息流上下文（首页沉浸流内嵌本组件）：小窗里只装播放器——先强落
    // 一次进度再跳纯播放页，/player 挂载后凭 resumeAt 精准接上
    if (onWheelStep) {
      const video = videoRef.current;
      if (video) persist(video.currentTime, true);
      void navigate({ to: '/player' });
    }
  }, [
    setMiniScreen,
    setCommentPanelOpen,
    setDanmakuPanelOpen,
    setVolumeOpen,
    setSeriesPanelOpen,
    setControlsHovered,
    onWheelStep,
    navigate,
    persist,
    videoRef,
  ]);

  /** 退出小屏：恢复窗口几何，留在播放页继续看。 */
  const exitMini = useCallback(() => {
    setMiniScreen(false);
    setControlsHovered(false); // 同 enterMini：悬停态别跨大小屏残留
    void appApi.exitMiniScreen().catch(() => undefined);
  }, [setMiniScreen, setControlsHovered]);

  /** 结束播放：退出小屏并回首页（小屏里唯一的「关掉」出口）。 */
  const stopMini = useCallback(() => {
    const video = videoRef.current;
    if (video && !video.paused) {
      video.pause(); // onPause 里会强制落一次进度
    }
    setMiniScreen(false);
    setControlsHovered(false); // 同 enterMini：悬停态别跨大小屏残留
    void appApi.exitMiniScreen().catch(() => undefined);
    void navigate({ to: '/' });
  }, [navigate, setMiniScreen, setControlsHovered, videoRef]);

  // 置顶（hgplayer De.pinned）：窗口级状态，大小屏共用同一个开关——
  // 大屏顶栏与小屏紧凑条两个入口，切换的是同一个 set_always_on_top
  const pinned = useUiStore((s) => s.pinned);
  const setPinned = useUiStore((s) => s.setPinned);
  const togglePinned = useCallback(() => {
    const next = !pinned;
    void appApi
      .setAlwaysOnTop(next)
      .then(() => setPinned(next))
      .catch(() => undefined);
  }, [pinned, setPinned]);

  // 键盘快捷键：空格 / ←→ / ↑↓。依赖显式列出，避免每次渲染重绑。
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const video = videoRef.current;
      if (!video) return;
      // 输入框里打字时不劫持按键：表单控件之外还要算上富文本输入
      // （弹幕/评论输入是 contentEditable 的 div，←→/空格/↑↓ 是移动
      // 光标和输入的一部分，不是快进快退/播放暂停/切集）
      const el = e.target as HTMLElement | null;
      if (el && (['INPUT', 'TEXTAREA', 'SELECT'].includes(el.tagName) || el.isContentEditable)) {
        return;
      }

      switch (e.key) {
        case 'Escape':
          // 面板（评论/选集/弹幕设置）开着时它们的 Esc 只管关面板；
          // 都没开而处于选中态时，Esc = 退出选中（滚轮/↑↓ 回到换剧）
          if (
            inBinge &&
            !seriesPanelOpen &&
            !commentPanelOpen &&
            !danmakuPanelOpen &&
            !volumeOpen
          ) {
            setBinge(null);
          }
          break;
        case ' ':
          e.preventDefault();
          if (video.paused) void video.play();
          else video.pause();
          break;
        case 'ArrowLeft':
          e.preventDefault();
          wakeChrome();
          video.currentTime = Math.max(0, video.currentTime - 5);
          break;
        case 'ArrowRight':
          e.preventDefault();
          wakeChrome();
          video.currentTime = Math.min(video.duration, video.currentTime + 5);
          break;
        case 'ArrowUp':
        case 'ArrowDown': {
          e.preventDefault();
          wakeChrome();
          // ↑↓ 的语义与滚轮同源：沉浸流（未选定剧）= 切上一部/下一部剧，
          // 从详情/历史等**选定**剧进来 = 切上一集/下一集
          const dir: 1 | -1 = e.key === 'ArrowDown' ? 1 : -1;
          setSlideDir(dir);
          if (inBinge) stepEpisode(dir);
          else if (onWheelStep) onWheelStep(dir);
          else stepEpisode(dir);
          break;
        }
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [
    stepEpisode,
    wakeChrome,
    onWheelStep,
    inBinge,
    setBinge,
    seriesPanelOpen,
    commentPanelOpen,
    danmakuPanelOpen,
    volumeOpen,
  ]);

  const handleEnded = () => {
    // 这个函数写在 `!seriesId || !vidIndex` 的提前返回之前，
    // TS 还没把 store 里的 null 收窄掉，所以这里得自己挡一道。
    // 元素本身确实存在（ended 只能由它自己触发），但不写检查就只能写 `!`。
    const video = videoRef.current;
    if (!seriesId || !vidIndex || !video) return;
    persist(video.currentTime, true);
    // 看完自动删：先清掉刚看完这集的本地文件，再决定连播下一集。
    // 没下载过的集本来就没有文件，后端返回 false，不提示。
    if (autoDelete) {
      deleteEpisode.mutate(
        { seriesId, vidIndex },
        {
          onSuccess: (deleted) => {
            if (deleted) toast.success(tf('player.autoDeleteDone', { index: vidIndex }));
          },
        },
      );
    }
    // 下载面板开着就连播：切集会让 PlayerView 带着 key 整体重挂载，
    // 面板连同勾选一起消失，用户刚选完的集就没了。
    // 末集播完不是「没有下一集」，是剧终：走剧终接力（下一季 → 推荐）
    if (autoNext && !downloading) {
      const total = currentSeries?.episodes.length ?? 0;
      if (total > 0 && vidIndex >= total) advanceAfterSeriesEnd();
      else stepEpisode(1);
    }
  };

  const handleVideoError = () => {
    if (error) return;
    // 带上浏览器给的 MediaError 编号：解码错(3)和网络错(2)的修法完全不同
    const me = videoRef.current?.error;
    const detail = me ? `（MediaError ${me.code}${me.message ? `: ${me.message}` : ''}）` : '';
    // 在线渐进流断供可自愈（重新 prepare 换新流接着播，位置不丢）：
    // 每集只自动兜一次，兜底转码进行中不抢跑；本地文件/转码产物坏了
    // 重试也是白搭，直接报错。在线的失败把 <video> 撤下来，换成封面 +
    // 明确错误 + 重试按钮，不再是一根小小的报错条挂在冻结画面上。
    const online = playSrc?.includes('hongguo-stream') ?? false;
    if (online && !compatSrc && !compat && retryTick < 1) {
      retryOnline();
      return;
    }
    setError(`${online ? t('player.onlineInterrupted') : t('error.media')}${detail}`);
    if (online) setSrc(null);
  };

  if (!seriesId || !vidIndex) {
    // store 目标恢复（重启读档/切源首批）前的瞬态：中性黑场 + 加载圈。
    // 曾经这里是「还没有选择剧集」的正式空态文案，刷新/切 tab 时它
    // 一闪而过，读起来像出错——瞬态就该长得像瞬态。
    return (
      <div className="grid h-full place-items-center bg-black">
        <Loader2 className="size-6 animate-spin text-white/40" aria-label={t('common.loading')} />
      </div>
    );
  }

  // 封面占位：盖在 <video> 之上（z-[5] 压过视频、让位弹幕 z-10），首帧出画
  // （playing）后交叉淡出。取流/解码的间隙里用户看到的是这部剧的封面而不是
  // 黑屏——「滚动切剧卡顿」的观感大头在这。取流中（!playSrc）同样垫着。
  const coverBackdrop = coverUrl ? (
    <CoverBackdrop key={coverUrl} src={coverUrl} hidden={videoLive && !switching} />
  ) : null;

  return (
    // 全模式一个视觉：视频区全出血贴边（无 padding/圆角/留白）。
    // 从历史/收藏等页进入与首页沉浸流是**同一个播放器**，选集走控制栏的
    // 「选集」弹层（immersive），不再维护第二套侧栏布局。
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex min-w-0 flex-1 flex-col">
        <div
          ref={stageRef}
          onWheel={onStageWheel}
          onClick={onStageClick}
          // 「移入显示」显式接 mouseenter：从画面外重新进入时浏览器可能
          // 没派发 mousemove（如跨窗口边界缓入），靠它兜底亮出控制栏
          onMouseEnter={wakeChrome}
          onMouseMove={wakeChrome}
          // 点击也算在场：点控制栏按钮后鼠标未必再动，不续命的话
          // 按钮一点、倒计时一到期控件就消失，观感像被抢走
          onPointerDown={wakeChrome}
          // 「移出隐藏」立即收起，不等倒计时
          onMouseLeave={hideChrome}
          className={cn(
            'relative min-h-0 flex-1 overflow-hidden bg-black',
            // 悬浮层收起后光标跟着藏（B站同款）：控制栏不挡内容，光标也不许挡。
            // 动一下鼠标 mousemove 先唤醒悬浮层，光标随之回来
            !chromeShown && 'cursor-none',
          )}
        >
          {/* 小屏的拖拽条：顶栏在小屏不渲染（第三方小屏是纯播放器），
              窗口拖动职责移到这条 24px 顶带。stopPropagation：拖拽残留
              的 click 不能触发「点画面暂停」。 */}
          {miniScreen && (
            <div
              data-tauri-drag-region
              onClick={(e) => e.stopPropagation()}
              className="absolute inset-x-0 top-0 z-30 h-6"
            />
          )}
          {/* 抖音式切换过渡：key 绑「实际供数的流」（取流中旧流继续播，
              动画精确落在新内容出画的那一帧），内容整体按方向滑入
              （下一个从下、上一个从上）+淡入——配合封面占位读作「翻页」，
              而不是硬切。方向在事件里先写进 state 再换目标：切换动作到
              流地址交换之间必然隔至少一次渲染，这次渲染读到的符号就是
              本次切换的方向。 */}
          <div
            key={playSrc ?? episodeKey}
            className={cn(
              'animate-in fade-in absolute inset-0 duration-300 ease-out',
              slideDir === -1 ? 'slide-in-from-top-10' : 'slide-in-from-bottom-10',
            )}
          >
            {playSrc ? (
              <>
                {/* 自绘控件，不要原生 controls：它既不跟主题，也放不下下载/清晰度这类业务动作。
                  key 绑 src：切清晰度时后端给出的流地址变了（地址里带档位），
                  换元素才能保证 <video> 真的重新加载——只改 src 属性在
                  WebView2 上不一定会触发重载，表现就是「点了 720p 画面不变」。 */}
                <video
                  /* key 绑播放地址：切清晰度、以及兜底转成 H.264 之后地址都变了，
                   换元素才能保证 <video> 真的重新加载——只改 src 属性在
                   WebView2 上不一定会触发重载，表现就是「点了 720p 画面不变」。 */
                  key={playSrc}
                  ref={videoRef}
                  src={playSrc}
                  /* 绝对定位铺满舞台：WKWebView 在流式布局里会把 100% 高度的
                   <video> 按固有纵横比撑大（1100 宽 → 1956 高），舞台裁切后
                   表现为「画面放大、控件全被顶出窗口」（mac 长期已知 bug）。
                   绝对定位把盒子钉死在舞台内，object-contain 只管留黑边。 */
                  className="absolute inset-0 size-full object-contain"
                  autoPlay
                  onLoadedMetadata={handleLoadedMetadata}
                  onPlay={() => {
                    // 恢复播放（含起播）也算「用户在场」：重新亮 3 秒再淡出，
                    // 不然暂停期间常显的界面会在恢复的一瞬全部消失
                    setPaused(false);
                    wakeChrome();
                  }}
                  onPlaying={() => {
                    // 真正出画/恢复出画：封面占位退场、卡顿态收掉
                    setLive({ key: streamKey, on: true });
                    setStalled({ key: streamKey, on: false });
                  }}
                  onPause={(e) => {
                    setPaused(true);
                    persist(e.currentTarget.currentTime, true);
                  }}
                  onLoadedData={() => setLive({ key: streamKey, on: true })}
                  // waiting：缓冲/seek 供不上数据，画面停住转黑——必须给出「在动」的信号，
                  // 否则网络一抖就是一帧黑屏挂在那里，观感等于卡死
                  onWaiting={() => setStalled({ key: streamKey, on: true })}
                  onCanPlay={() => setStalled({ key: streamKey, on: false })}
                  onTimeUpdate={(e) => {
                    // playing 事件在个别 WebView 起播路径上不触发：封面退场
                    // 不能只靠它一路信号，时间轴真的走起来了也算出画
                    if (e.currentTarget.currentTime > 0.1) setLive({ key: streamKey, on: true });
                    // 时间轴在推进本身就是「没卡住」的证据，waiting 的卡顿态在这里收掉
                    setStalled({ key: streamKey, on: false });
                    persist(e.currentTarget.currentTime);
                  }}
                  onEnded={handleEnded}
                  onRateChange={handleRateChange}
                  onVolumeChange={handleVolumeChange}
                  onError={handleVideoError}
                />
                {coverBackdrop}
                <DanmakuLayer
                  videoRef={videoRef}
                  items={danmakuQuery.data ?? []}
                  enabled={danmakuOn}
                  display={danmakuDisplay}
                />
                {/* 沉浸流信息叠加（hgplayer 同款）：@剧名/集数/简介压在画面左下。
                  控制栏弹出时整体抬到控制栏上沿之上（bottom-28），隐藏时落回
                  bottom-14——两者transition 联动，不再互相遮挡。
                  小屏模式不用这坨：紧凑控件条自带收敛的剧名行。 */}
                <div
                  className={cn(
                    'absolute left-3 z-10 max-w-[62%] transition-all duration-300',
                    chromeShown && !miniScreen
                      ? 'bottom-28 opacity-100'
                      : 'pointer-events-none bottom-14 opacity-0',
                  )}
                >
                  {/* 热度行（hgplayer 1.1.6 同款：剧名上方） */}
                  {overlayMeta?.heatText && (
                    <p className="text-xs font-medium text-amber-300/90 drop-shadow-md">
                      {overlayMeta.heatText}
                    </p>
                  )}
                  {/* 剧名 → 详情页。第三方同款交互：点标题离开播放器看档案/选集。
                    stopPropagation：点标题不能同时触发「点画面暂停」。 */}
                  <div className="flex items-center gap-1.5">
                    {overlayMeta?.badge && (
                      <span className="rounded-sm bg-red-500/90 px-1 py-px text-[10px] font-semibold text-white">
                        {overlayMeta.badge}
                      </span>
                    )}
                    {overlayMeta?.seasonTag && (
                      <span className="rounded-sm bg-white/20 px-1 py-px text-[10px] font-semibold text-white">
                        {overlayMeta.seasonTag}
                      </span>
                    )}
                    <button
                      type="button"
                      onClick={(e) => {
                        e.stopPropagation();
                        void navigate({ to: '/detail', search: { seriesId } });
                      }}
                      className="cursor-pointer text-sm font-semibold text-white drop-shadow-md hover:underline"
                      title={currentSeries?.title}
                    >
                      @{currentSeries?.title ?? ''}
                    </button>
                  </div>
                  <p className="mt-0.5 text-xs text-white/85 drop-shadow-md">
                    {tf('player.epShort', { index: vidIndex })}
                    {currentSeries && currentSeries.episodeCount > 0 && (
                      <span className="text-white/70">
                        {' · '}
                        {tf('player.totalEpisodes', { count: currentSeries.episodeCount })}
                      </span>
                    )}
                  </p>
                  {meta?.intro && (
                    // 简介块只占舞台约三分之一（hgplayer 同款量级，大屏实测
                    // ~400px）：之前跟着容器吃到 62%，两行密文糊满左下角。
                    // 容器吃掉点击（防触发舞台暂停/继续），展开交互只在按钮上
                    <div
                      className="mt-1 flex max-w-[36%] items-end gap-2"
                      onClick={(e) => e.stopPropagation()}
                    >
                      <p
                        className={cn(
                          'text-xs leading-relaxed text-white/70 drop-shadow-md',
                          !introExpanded && 'line-clamp-2',
                        )}
                      >
                        {meta.intro}
                      </p>
                      {meta.intro.length > 40 && (
                        <button
                          type="button"
                          onClick={() => setIntroExpanded((v) => !v)}
                          className="shrink-0 cursor-pointer pb-0.5 text-xs text-white/60 drop-shadow-md hover:text-white"
                        >
                          {introExpanded ? t('player.introCollapse') : t('player.introExpand')}
                        </button>
                      )}
                    </div>
                  )}
                </div>
                {/* 快捷键提示只在暂停时露一面向中部提示——常驻顶栏会把分类 tab
                  挡死（顶部让位给 tab），平时不打扰。小屏（480 宽）装不下。 */}
                {paused && !error && !miniScreen && (
                  <div className="pointer-events-none absolute inset-x-0 top-[38%] z-10 flex justify-center">
                    <span className="rounded-full bg-black/55 px-4 py-1.5 text-xs text-white/75 backdrop-blur-sm">
                      {t('player.keyboardHint')}
                    </span>
                  </div>
                )}
                {/* 沉浸流分类 tab 栏已上移 AppShell 顶栏（portal 插槽，
                  画面顶部不再有悬浮 tab 层） */}
                {/* 沉浸流评论区：右侧滑出（💬 触发）；进小屏时已收起 */}
                {commentPanelOpen && !miniScreen && (
                  <CommentPanel
                    seriesId={seriesId}
                    vid={currentVid ? `${currentVid}:${seriesId}` : ''}
                    onClose={() => setCommentPanelOpen(false)}
                  />
                )}
                {/* 互动栏（点赞/评论/收藏/预约/分享，抖音系右缘形态）：跟随悬浮层淡出；
                  评论区面板打开时让位隐藏（面板就盖在右缘，Esc 或 💬 再开）。
                  vid 是「vid:seriesId」组合形态（与弹幕缓存 key 同构），组件内部自行拆用。 */}
                <InteractionRail
                  seriesId={seriesId}
                  vid={currentVid ? `${currentVid}:${seriesId}` : ''}
                  visible={chromeShown && !commentPanelOpen && !miniScreen}
                  title={currentSeries?.title}
                  // 公开计数来自剧集档案（detail 接口逐集下发），匿名可见
                  commentCount={currentEpisode?.commentCount}
                  diggCount={currentEpisode?.diggCount}
                  followCount={currentSeries?.followedCnt}
                />
                {/* 控制栏大小屏两套形态，共用同一个 <video>（元素在上面，
                  不随这里的切换卸载）——切大小屏播放零中断 */}
                {miniScreen ? (
                  <MiniScreenControls
                    videoRef={videoRef}
                    title={currentSeries?.title}
                    intro={meta?.intro}
                    vidIndex={vidIndex}
                    total={currentSeries?.episodes.length ?? 0}
                    hasNext={
                      currentSeries?.episodes.some((e) => e.vidIndex === (vidIndex ?? 0) + 1) ??
                      false
                    }
                    onStepEpisode={(d) => stepEpisode(d)}
                    onExpand={exitMini}
                    onClose={stopMini}
                    incognitoOn={incognito.on}
                    onToggleIncognito={incognito.toggle}
                    pinned={pinned}
                    onTogglePinned={togglePinned}
                    visible={chromeShown}
                    // B站同款：悬在小屏控制条上不许收（静止 3 秒收起对控件操作是抢走）
                    onControlsEnter={() => {
                      setControlsHovered(true);
                      wakeChrome();
                    }}
                    onControlsLeave={() => setControlsHovered(false)}
                  />
                ) : (
                  <PlayerControls
                    videoRef={videoRef}
                    stageRef={stageRef}
                    onControlsEnter={() => {
                      setControlsHovered(true);
                      wakeChrome();
                    }}
                    onControlsLeave={() => setControlsHovered(false)}
                    seriesId={seriesId}
                    episodes={currentSeries?.episodes ?? []}
                    currentIndex={vidIndex}
                    downloading={downloading}
                    onDownloadingChange={setDownloading}
                    onStepEpisode={stepEpisode}
                    onOpenMini={enterMini}
                    incognito={incognito.on}
                    onToggleIncognito={incognito.toggle}
                    definition={activeDefinition}
                    definitions={definitions}
                    onDefinitionChange={setDefinition}
                    src={playSrc}
                    danmakuOn={danmakuOn}
                    onToggleDanmaku={toggleDanmaku}
                    danmakuDisplay={danmakuDisplay}
                    onDanmakuDisplayChange={updateDanmakuDisplay}
                    immersive
                    visible={chromeShown}
                    // 信息流里主动点了某一级 = 要追这部：进入选中剧连播（/player，
                    // 滚轮/↑↓ 自动变为切集），而不是留在推荐流继续换剧
                    pickerHint={onWheelStep ? t('player.pickerBingeHint') : undefined}
                    onPickEpisode={(idx) => {
                      setTarget(seriesId, idx);
                      // 信息流里主动选了某集 = 要追这部：原地锁定本剧连播
                      // （滚轮/↑↓ 切集 + 左上角亮出连播指示），画面无缝续播
                      if (onWheelStep) setBinge(seriesId);
                    }}
                  />
                )}

                {/* 兜底转码浮层。转一集要几十秒，没有它用户只能盯着黑屏，
                  不知道是卡住了还是在慢慢转。z-30：压过封面占位。 */}
                {compat && (
                  <div className="absolute inset-0 z-30 grid place-items-center bg-black/85 p-6 text-center text-sm text-neutral-200">
                    <div className="flex w-full max-w-sm flex-col items-center gap-3">
                      <p>
                        {compat.phase === 'downloading'
                          ? t('player.compatFetching')
                          : tf('player.compatTranscoding', { percent: Math.floor(compat.percent) })}
                      </p>
                      <Progress
                        value={compat.phase === 'downloading' ? 0 : compat.percent}
                        className="h-1.5"
                      />
                      <span className="text-xs text-neutral-400">{t('player.compatHint')}</span>
                    </div>
                  </div>
                )}

                {/* 加载反馈 = 顶部 2px 细进度条（hg-loadbar，样式见 index.css）：
                  切换在途/首帧未出/缓冲中任何一种未就绪都亮。中央的
                  「正在缓冲 X%」胶囊按用户要求移除——它压在画面正中
                  挡内容；细条贴边滑过，反馈有了、打扰没了。 */}
                {(switching || !videoLive || videoStalled) && !compat && (
                  <div className="pointer-events-none absolute inset-x-0 top-0 z-40 h-0.5">
                    <div className="hg-loadbar-track">
                      <div className="bg-primary hg-loadbar" />
                    </div>
                  </div>
                )}
              </>
            ) : (
              <>
                {coverBackdrop}
                <div className="absolute inset-0 z-10 grid place-items-center p-6">
                  {/* 缓冲时给的是「在动到哪了」，不是一个没头没尾的转圈；
                    文案收进胶囊压在封面上。自动重试耗尽的错误态再给一个
                    手动重试入口——用户不该只能眼看黑屏干着急。 */}
                  <div className="flex flex-col items-center gap-3">
                    <span className="flex items-center gap-2 rounded-full bg-black/60 px-4 py-1.5 text-xs text-white/85 backdrop-blur-sm">
                      {!error && <Loader2 className="size-3.5 animate-spin" aria-hidden />}
                      {error ??
                        (buffering && buffering.phase !== 'ready'
                          ? tf('player.buffering', {
                              percent: buffering.total > 0 ? Math.floor(buffering.percent) : 0,
                              size:
                                buffering.total > 0
                                  ? `${formatBytes(buffering.received)} / ${formatBytes(buffering.total)}`
                                  : formatBytes(buffering.received),
                            })
                          : t('common.loading'))}
                    </span>
                    {error && (
                      <Button
                        size="sm"
                        variant="outline"
                        data-wheel-block
                        onClick={(e) => {
                          e.stopPropagation();
                          retryOnline();
                        }}
                      >
                        {t('player.retry')}
                      </Button>
                    )}
                  </div>
                </div>
              </>
            )}
          </div>
        </div>

        {/* 页面级杂物的浮层化：错误条压在画面**右上角**——
            左上贴着顶栏应用名会被读成「挡标题」，左下是剧名信息层，
            右上只在评论区面板打开时让位。跟随悬浮层淡出。
            连播/看完自动删不再在此放开关，统一去设置页改。 */}
        <div
          data-wheel-block
          className={cn(
            'absolute top-3 right-3 z-20 flex items-center gap-2',
            'transition-opacity duration-300',
            chromeShown ? 'opacity-100' : 'pointer-events-none opacity-0',
          )}
        >
          {error && (
            <span className="rounded-full border border-red-500/30 bg-red-950/90 px-3 py-1 text-xs text-red-200">
              {error}
            </span>
          )}
        </div>
      </div>
    </div>
  );
}

/**
 * 封面占位图（裂图自愈）。
 *
 * 源图可能是 HEIC（部分 WebView 渲染不了），onError 后整层退场，不留一个
 * 破图标压在画面上。`hidden` 是「视频已出画」：淡出而非卸载，切下一部剧时
 * key 随 src 变化重挂载，broken/透明度状态自然归零。
 */
function CoverBackdrop({ src, hidden }: { src: string; hidden: boolean }) {
  const [broken, setBroken] = useState(false);
  if (broken) return null;
  return (
    <img
      src={src}
      alt=""
      aria-hidden
      onError={() => setBroken(true)}
      className={cn(
        'pointer-events-none absolute inset-0 z-[5] size-full object-cover object-top brightness-[0.55]',
        'transition-opacity duration-500',
        hidden ? 'opacity-0' : 'opacity-100',
      )}
    />
  );
}
