import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react';
import { useNavigate } from '@tanstack/react-router';
import { ChevronRight, Loader2 } from 'lucide-react';
import { toast } from 'sonner';
import { DanmakuLayer } from './danmaku-layer';
import { PlayerControls } from './controls/player-controls';
import { MiniScreenControls } from './mini-screen-controls';
import { InteractionRail } from './interaction-rail';
import { CommentPanel } from './comment-panel';
import { ChromeSurface } from './chrome-surface';
import { CompatOverlay } from './compat-overlay';
import { CoverBackdrop } from './cover-backdrop';
import { ErrorBar } from './error-bar';
import { FrameHold } from './frame-hold';
import { HevcInstallCard } from './hevc-install-card';
import { ImmersiveInfoOverlay } from './immersive-info-overlay';
import { KeyboardHint } from './keyboard-hint';
import { Loadbar } from './loadbar';
import { LoadingPanel } from './loading-panel';
import { MiniDragBar } from './mini-drag-bar';
import {
  useDanmaku,
  usePlaybackCapability,
  useSeriesDetailMeta,
  useSeriesEpisodes,
  useSettings,
  useStorageActions,
} from '@/service/queries';
import { usePlayerStore } from '@/stores/player';
import { useUiStore } from '@/stores/ui';
import { STREAM_SCHEME } from '@/utils/scheme';
import { t, tf } from '@/locales';
import { cn } from '@/lib/utils';
import { useIncognitoMode } from './incognito';
import { useMinimizeAutoPause } from '../hooks/use-minimize-pause';
import { classifyMediaError } from '../hooks/media-error';
import type { MediaErrorKind } from '../hooks/media-error';
import { emptyFailureCounts, recoveryAction } from '../hooks/recovery-policy';
import type { FailureCounts } from '../hooks/recovery-policy';
import { hevcSession } from '../hooks/hevc-session';
import { usePlaybackSource } from '../hooks/use-playback-source';
import { useTranscodeFallback } from '../hooks/use-transcode-fallback';
import { useFrameHold } from '../hooks/use-frame-hold';
import { usePlaybackProgress } from '../hooks/use-playback-progress';
import { useDanmakuSettings } from '../hooks/use-danmaku-settings';
import { useBingeRelay } from '../hooks/use-binge-relay';
import { usePlayerOverlay } from '../hooks/use-player-overlay';
import { usePlayerInteractions } from '../hooks/use-player-interactions';
import { useMiniWindow } from '../hooks/use-mini-window';
import { usePortraitWindow } from '../hooks/use-portrait-window';
import { isAutoAdvanceStart, isPickerClickCooldown, markAutoAdvance } from '../playback-signals';
import { usePinWindow } from '@/hooks/use-pin-window';

/** 自动连播静默硬上限：主闸门是「首播之前不唤醒」，这只兜起播失败的死局。 */
const QUIET_WINDOW_MS = 8_000;

export function PlayerView({
  onWheelStep,
  coverUrl,
  overlayMeta,
  onFirstFrame,
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
  /**
   * 首帧真正出画的回调：沉浸流宿主用它给「预取下一部」发令——在播的
   * 一路站稳了，带宽才让给下一部的铺垫。onPlaying 在个别 WebView 起播
   * 路径上不触发，timeupdate 的出画兜底同样会调用；重复调用无副作用。
   */
  onFirstFrame?: () => void;
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
  /**
   * 自动连播静默起步：handleEnded 切集时打了点，这次挂载若是它带来的，
   * 悬浮层以「无人操作」起步——不亮控制栏、不亮光标，起播的 play 事件
   * 也不唤醒（用户感知就是上一集安安静静接到下一集）。useState 初始化器
   * 只在挂载时求值一次，之后的重渲染与本次无关。
   */
  const [quietStart] = useState(isAutoAdvanceStart());
  /**
   * 静默期开关（喂给 usePlayerOverlay 的 silenced，chromeShown 一票否决）：
   * 从自动连播打点起，到新集首播（onPlay）或用户真实输入（点击/滚轮/按键）
   * 为止。期间一切噪声都点不亮任何悬浮层。挂载即静默走同一份 state。
   */
  const [quietArmed, setQuietArmed] = useState(quietStart);
  /**
   * 静默窗截止时刻（硬上限兜底）。光标停在画面上时，切集会把舞台里的
   * DOM 换掉两回（换加载占位、换新流的 <video>）——每回 Chromium 都会
   * 向新元素**补发 mouseenter/mousemove**（悬停重算，指针其实没动）。
   * 新流什么时候出画取决于取流快慢，所以主闸门是「首播之前一律静默」
   * （见 wakeOnMove/playedOnceRef），这个定时窗只兜自动起播失败的情形：
   * 超时后移动照常唤醒，用户不会被困在没有控制栏的黑屏里。
   * 布防放 useLayoutEffect：它在提交内同步跑完，先于浏览器派发幻影事件
   * 的那个任务（Date.now() 不许进渲染期，purity 规则也这么要求）。
   */
  const quietUntilRef = useRef(0);
  useLayoutEffect(() => {
    if (quietStart) quietUntilRef.current = Date.now() + QUIET_WINDOW_MS;
  }, [quietStart]);
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
  /**
   * 本集是否已经真正播起来过。两个用途：
   * - 首播前的 pause 是新元素装载噪声，不算暂停（见 onPause）；
   * - 首播之前移动类事件不唤醒悬浮层（幻影 mousemove，见 wakeOnMove）。
   * 归零点在 handleEnded（同步、事件处理器内）：信息流里 PlayerView 跨集
   * 复用，若靠 effect 归零，被动 effect 跑完前幻影事件会读到上一集的 true。
   */
  const playedOnceRef = useRef(false);

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
  // 无 HEVC 扩展机器的安装引导数据源（false 才弹卡，探测未落定不渲染）
  const { data: hevcPlayable } = usePlaybackCapability();
  /**
   * 解码失败待裁决：带 key（集标识）读时过滤而非换集时清空——同步
   * setState 会触发级联渲染（react-hooks/set-state-in-effect 会拦）。
   */
  const [decodeBlocked, setDecodeBlocked] = useState<{ key: string } | null>(null);
  const { danmakuOn, danmakuDisplay, updateDanmakuDisplay, toggleDanmaku } = useDanmakuSettings();
  // 弹幕拉取失败不能静默：画面照常播，但用户该知道弹幕为什么没了
  useEffect(() => {
    if (danmakuQuery.isError) {
      toast.error(tf('player.danmakuFailed', { reason: String(danmakuQuery.error) }));
    }
  }, [danmakuQuery.isError, danmakuQuery.error]);

  // 转码兜底与最终供流地址裁决抽在 use-transcode-fallback：
  // videoWidth==0 探测 + H.264 转码轮询 + 重试防缓存钉。
  // 裁决入口给 hook 一个恒定代理：真正实现依赖 hook 回传的 startCompat
  // （要问「装扩展还是转兼容」），定义在 hook 之后经 ref 同步。
  const onDecodeFailureRef = useRef<() => void>(() => {});
  const onDecodeFailure = useCallback(() => onDecodeFailureRef.current(), []);
  const { compatSrc, compat, playSrc, streamKey, startCompat } = useTranscodeFallback({
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
    onDecodeFailure,
  });
  /** 切换在途：目标已是新的一集，元素里还是上一条流（play 请求未返回） */
  const switching = srcKey !== episodeKey;

  // 解码失败的裁决：无 HEVC 机器先出主卡问「装扩展还是转兼容」（选过
  // 转兼容的会话不再重复问，径直兜底——连播不被打断）；其余情况直接转。
  // 装完扩展后能力缓存已刷新为 true，此后解码失败自动走兜底（罕见路径）。
  const decodeBlocking = decodeBlocked?.key === episodeKey;
  const requestDecodeFallback = useCallback(() => {
    if (hevcPlayable === false && !hevcSession.compatAuthorized()) {
      setDecodeBlocked({ key: episodeKey });
      return;
    }
    startCompat();
  }, [hevcPlayable, startCompat, episodeKey]);
  // 每次渲染同步最新裁决到代理（effect 时机足够：轮询首发至少 1.2s 后）
  useEffect(() => {
    onDecodeFailureRef.current = requestDecodeFallback;
  }, [requestDecodeFallback]);

  // 失败账本：本集各类错误已发生几次，recovery-policy 据此裁决「首次重试/
  // 复发升级」。ref 而非 state——只在错误事件回调里读写，不驱动渲染；
  // 带集 key 读时校验，换集自然归零。
  const failuresRef = useRef<{ key: string; counts: FailureCounts } | null>(null);
  const failuresOf = () =>
    failuresRef.current?.key === episodeKey ? failuresRef.current.counts : emptyFailureCounts();
  const recordFailure = (kind: MediaErrorKind) => {
    const counts = failuresOf();
    failuresRef.current = { key: episodeKey, counts: { ...counts, [kind]: counts[kind] + 1 } };
  };

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

  // ---- 隐身模式（与控制栏的 Eye 按钮共享状态）：鼠标离开窗口即
  //      整窗透明 + 暂停，鼠标回来恢复显示（见 incognito.ts 的机制说明） ----
  const incognito = useIncognitoMode(videoRef);

  // ---- 最小化自动暂停（设置可关）：窗口最小化即暂停，恢复继续；
  //      与隐身模式的欠账模型各自独立 ----
  useMinimizeAutoPause({
    videoRef,
    enabled: settings?.pauseOnMinimize ?? true,
  });

  // ---- 沉浸流悬浮层显隐状态机（B站方案）抽在 use-player-overlay：
  //      3s 倒计时 / 暂停常显 / 控件悬停不收在这里统一裁决；
  //      静默期（quietArmed）下 chromeShown 被 silenced 一票否决
  const { paused, setPaused, chromeShown, wakeChrome, hideChrome, setControlsHovered } =
    usePlayerOverlay({
      seriesPanelOpen,
      commentPanelOpen,
      danmakuPanelOpen,
      volumeOpen,
      startVisible: !quietStart,
      silenced: quietArmed,
    });

  // 静默期的两类唤醒闸门：silenced 在源头兜底（见上），这里再拦一层让
  // wakeChrome 连状态都不被噪声污染（否则静默一解除，残留的 chromeVisible
  // 会立刻把悬浮层顶出来）；点击/滚轮/按键是铁证，解除静默并正常唤醒
  const wakeOnMove = useCallback(() => {
    if (playedOnceRef.current || Date.now() >= quietUntilRef.current) wakeChrome();
  }, [wakeChrome]);
  const wakeOnInput = useCallback(() => {
    setQuietArmed(false);
    wakeChrome();
  }, [wakeChrome]);

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

  // 同剧切集的末帧定格：取流窗口旧画面继续播、流交换后定格帧顶住，直到
  // 新集首帧出画——封面只留给切剧接管与初次起播。显隐真值表与抓帧时机
  // 在 use-frame-hold（同文件名的 test 是观感回归锁）。
  const {
    frame: heldFrame,
    holdVisible,
    coverHidden: holdCoverHidden,
    capture: captureFrame,
  } = useFrameHold({
    videoRef,
    seriesId,
    episodeKey,
    videoLive,
    switching,
    streamKey,
    playSrc,
    error,
    compatActive: compat != null,
  });

  // ---- 舞台交互三套裁决（滚轮/点击/键盘）抽在 use-player-interactions：
  //      沉浸流=换剧、选定剧/连播=切集的语义与 400ms 滚轮冷却都在那边
  const { onStageWheel, onStageClick } = usePlayerInteractions({
    videoRef,
    setSlideDir,
    seriesPanelOpen,
    downloading,
    inBinge,
    stepEpisode,
    onWheelStep,
    wakeChrome: wakeOnInput,
    setBinge,
    commentPanelOpen,
    danmakuPanelOpen,
    volumeOpen,
  });

  /**
   * 选集浮层刚关闭的一瞬，跟手/连击的点击会落到舞台上——浮层已卸载，
   * data-wheel-block 拦不住，就成了无意识的播放/暂停切换（选集选着选着
   * 视频停了，多半是它）。短窗内的舞台点击一律忽略。
   */
  const onStageClickGuarded = useCallback(
    (e: React.MouseEvent) => {
      if (isPickerClickCooldown()) return;
      onStageClick(e);
    },
    [onStageClick],
  );

  // 进度持久化（5s 节流落盘 + 观看历史上报心跳 + 卸载补写）抽在
  // use-playback-progress；lastKnownRef/srcKeyRef 两把竞态防御 ref 的
  // 真值在取流侧持有，这里经参数转入（起播续点、startCompat 认流共用）。
  // report 走播放位置心跳（节拍独立于落盘节流），persist 只管落盘与收尾上报。
  const { persist, report } = usePlaybackProgress({
    videoRef,
    seriesId,
    vidIndex,
    episodeKey,
    srcKeyRef,
    lastKnownRef,
    currentVid,
    currentSeries,
  });

  // 小屏进出（enterMini/exitMini）抽在 use-mini-window；
  // 置顶开关抽在 src/hooks/use-pin-window 与 AppShell 顶栏共用——
  // 大屏顶栏与小屏紧凑条两个入口，切换的是同一个 set_always_on_top。
  const { miniScreen, enterMini, exitMini } = useMiniWindow({
    setControlsHovered,
    setCommentPanelOpen,
    setDanmakuPanelOpen,
    setVolumeOpen,
    setSeriesPanelOpen,
  });
  // 竖屏进出/重适配/铺满档全在本 hook（store 只是镜像）：控制形态的三分
  // 支（小窗/竖屏/全量）在这里裁决，PlayerControls 不再自持竖屏态
  const { portrait, portraitFull, togglePortrait, togglePortraitFull } = usePortraitWindow({
    videoRef,
  });
  // 无边框播放（hgplayer 播放页 bare 布局同款）：chrome 全藏不进 OS 全屏，
  // 开关在控制栏；这里只消费状态（拖拽条显隐）
  const frameless = useUiStore((s) => s.playerFrameless);
  const { pinned, togglePinned } = usePinWindow();

  const handleEnded = () => {
    // 这个函数写在 `!seriesId || !vidIndex` 的提前返回之前，
    // TS 还没把 store 里的 null 收窄掉，所以这里得自己挡一道。
    // 元素本身确实存在（ended 只能由它自己触发），但不写检查就只能写 `!`。
    const video = videoRef.current;
    if (!seriesId || !vidIndex || !video) return;
    // 评论面板开着 = 用户在读评论/等 AI 回复：本集播完**循环重播本集**，
    // 不自动连播切集、不落「看完」进度（官方同款；关面板恢复连播）
    if (commentPanelOpen) {
      video.currentTime = 0;
      void video.play();
      return;
    }
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
      // 自动连播打点：新视图静默起步（不亮操作栏/光标），见 playback-signals。
      // 信息流里 PlayerView 是复用的（不重挂载），静默窗要在同一实例里续上，
      // 盖住流交换时补发的幻影 mousemove——两种上下文都得有这扇窗。
      // playedOnce 同步归零也在这一拍：被动 effect 归零跑得比幻影事件慢，
      // 会把上一集的「已播过」漏给新一集的静默闸门。
      markAutoAdvance();
      playedOnceRef.current = false;
      quietUntilRef.current = Date.now() + QUIET_WINDOW_MS;
      setQuietArmed(true);
      const total = currentSeries?.episodes.length ?? 0;
      if (total > 0 && vidIndex >= total) advanceAfterSeriesEnd();
      else {
        // 自动连播完成本集 = 进入选中剧连播态（入口之二，另一个是
        // 「观看完整短剧」横条）：既然在本剧连着看，滚轮/↑↓ 就该切集
        setBinge(seriesId);
        stepEpisode(1);
      }
    }
  };

  const handleVideoError = () => {
    if (error) return;
    // 带上浏览器给的 MediaError 编号：解码错(3)和网络错(2)的修法完全不同
    const me = videoRef.current?.error;
    const detail = me ? `（MediaError ${me.code}${me.message ? `: ${me.message}` : ''}）` : '';
    // 在线渐进流断供可自愈（重新 prepare 换新流接着播，位置不丢）：每类
    // 错误各有一次重试预算，用完才升级——decode 升转码兜底、其余落错误页
    // （裁决矩阵见 recovery-policy）。本地文件/转码产物坏了重试也是白搭，
    // 直接报错。在线的失败把 <video> 撤下来，换成封面 + 明确错误 + 重试
    // 按钮，不再是一根小小的报错条挂在冻结画面上。
    const online = playSrc?.includes(STREAM_SCHEME) ?? false;
    if (online && !compatSrc && !compat) {
      const kind = classifyMediaError(me?.code);
      const action = recoveryAction(kind, failuresOf());
      recordFailure(kind);
      if (action === 'retry') {
        retryOnline();
        return;
      }
      if (action === 'fallback') {
        requestDecodeFallback();
        return;
      }
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

  // 封面占位：盖在 <video> 之上（z-[5] 压过视频、让位弹幕 z-10）。只属于
  // 切剧接管与画面真空（初次起播/出错）：同剧切集的间隙由定格帧接棒，
  // 显隐真值在 use-frame-hold 的 overlayVisibility——切集不再每次闪封面。
  const coverBackdrop = coverUrl ? (
    <CoverBackdrop key={coverUrl} src={coverUrl} hidden={holdCoverHidden} />
  ) : null;

  return (
    // 全模式一个视觉：视频区全出血贴边（无 padding/圆角/留白）。
    // 从历史/收藏等页进入与首页沉浸流是**同一个播放器**，选集走控制栏的
    // 「选集」弹层（immersive），不再维护第二套侧栏布局。
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex min-w-0 flex-1 flex-col">
        <div
          onWheel={onStageWheel}
          onClick={onStageClickGuarded}
          // 「移入显示」显式接 mouseenter：从画面外重新进入时浏览器可能
          // 没派发 mousemove（如跨窗口边界缓入），靠它兜底亮出控制栏
          onMouseEnter={wakeOnMove}
          onMouseMove={wakeOnMove}
          // 点击也算在场：点控制栏按钮后鼠标未必再动，不续命的话
          // 按钮一点、倒计时一到期控件就消失，观感像被抢走
          onPointerDown={wakeOnInput}
          // 「移出隐藏」立即收起，不等倒计时
          onMouseLeave={hideChrome}
          className={cn(
            'relative min-h-0 flex-1 overflow-hidden bg-black',
            // 悬浮层收起后光标跟着藏（B站同款）：控制栏不挡内容，光标也不许挡。
            // 动一下鼠标 mousemove 先唤醒悬浮层，光标随之回来
            !chromeShown && 'cursor-none',
          )}
        >
          {/* 小屏/无边框都没有顶栏：拖拽条顶上（无边框模式下它是唯一的
              移窗抓手；小屏沿用原语义） */}
          {(miniScreen || frameless) && <MiniDragBar />}
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
                {/* 自绘控件，不要原生 controls：它既不跟主题，也放不下下载/清晰度这类业务动作。 */}
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
                  className={cn(
                    'absolute inset-0 size-full',
                    // 小屏 object-cover：迷你窗是固定 16:9，竖版/异比例源用
                    // contain 会留大片黑边（用户实测截图）——小窗里裁切比
                    // 黑边可取；大屏保留 contain（换剧的过场不裁内容）
                    miniScreen ? 'object-cover' : 'object-contain',
                  )}
                  autoPlay
                  onLoadedMetadata={handleLoadedMetadata}
                  onPlay={() => {
                    // 这里不再唤醒悬浮层：play 事件分不清「用户按的播放」还是
                    // 「切集后的自动起播」，而后者亮操作栏正是「自动下一集抢占
                    // 鼠标」的来源。用户路径各有天然唤醒（点击=舞台 pointerdown、
                    // 空格=快捷键里显式唤醒），这里只管同步播放态。
                    // 首播同时解除静默：新集画面已经在走了
                    playedOnceRef.current = true;
                    setQuietArmed(false);
                    setPaused(false);
                  }}
                  onPlaying={() => {
                    // 真正出画/恢复出画：封面占位退场、卡顿态收掉
                    setLive({ key: streamKey, on: true });
                    setStalled({ key: streamKey, on: false });
                    onFirstFrame?.();
                  }}
                  onPause={(e) => {
                    // 首播之前的 pause 是新元素的装载噪声（自动起播落定前
                    // 引擎会先发一记），它会把「暂停常显」点亮——控制栏和
                    // 快捷键提示闪一下，正是「自动切集亮控件」的另一半。
                    // 静默起步时 UI 本来就视同在播，噪声直接忽略；正常进入
                    // 时初值就是暂停态，忽略它也不改变任何显示。
                    if (!playedOnceRef.current) return;
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
                    if (e.currentTarget.currentTime > 0.1) {
                      setLive({ key: streamKey, on: true });
                      onFirstFrame?.();
                    }
                    // 时间轴在推进本身就是「没卡住」的证据，waiting 的卡顿态在这里收掉
                    setStalled({ key: streamKey, on: false });
                    persist(e.currentTarget.currentTime);
                    // 历史上报心跳：媒体时钟驱动（暂停即停），节拍见 history-cadence
                    report(e.currentTarget.currentTime);
                  }}
                  onEnded={handleEnded}
                  onRateChange={handleRateChange}
                  onVolumeChange={handleVolumeChange}
                  onError={handleVideoError}
                />
                {coverBackdrop}
                {/* 切集/首帧间隙的中央 loading：此刻必然还没有画面（封面占位
                  在下、弹幕在上），黑屏等待给个明确的「在加载」信号——自动
                  连播切集遇未缓存集时尤其需要，不然观感就是卡死黑屏。播放中
                  的缓冲不在此列（stalled 时有末帧可看，顶部细条已够），压
                  内容的中央胶囊也是用户明令移除过的，别借这条还魂。 */}
                {(switching || !videoLive) && !compat && !error && (
                  <div className="absolute inset-0 z-[6] grid place-items-center">
                    <Loader2
                      className="size-8 animate-spin text-white/60"
                      aria-label={t('common.loading')}
                    />
                  </div>
                )}
                <DanmakuLayer
                  videoRef={videoRef}
                  items={danmakuQuery.data ?? []}
                  enabled={danmakuOn}
                  display={danmakuDisplay}
                />
                <ImmersiveInfoOverlay
                  chromeShown={chromeShown}
                  miniScreen={miniScreen}
                  overlayMeta={overlayMeta}
                  title={currentSeries?.title}
                  episodeCount={currentSeries?.episodeCount}
                  seriesId={seriesId}
                  vidIndex={vidIndex}
                  intro={meta?.intro}
                  introExpanded={introExpanded}
                  setIntroExpanded={setIntroExpanded}
                  navigate={navigate}
                  inBinge={inBinge}
                  onExitBinge={() => setBinge(null)}
                />
                {/* 快捷键提示只在暂停时露一面向中部提示——常驻顶栏会把分类 tab
                  挡死（顶部让位给 tab），平时不打扰。小屏（480 宽）与竖屏窄条
                  都装不下，不渲染。 */}
                {paused && !error && !miniScreen && !portrait && <KeyboardHint />}
                {/* 沉浸流分类 tab 栏已上移 AppShell 顶栏（portal 插槽，
                  画面顶部不再有悬浮 tab 层） */}
                {/* 沉浸流评论区：右侧滑出（💬 触发）；进小屏时已收起 */}
                {commentPanelOpen && !miniScreen && (
                  <CommentPanel
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
                  // 追更钮显隐判定源（video_detail series_status，与详情页同源）
                  seriesStatus={meta?.seriesStatus}
                />
                {/* 沉浸流底部「观看完整短剧 · 全N集」横条：进入选中剧连播态
                  （滚轮/↑↓=切集）的**唯一手动入口**。已选中时不渲染——
                  横条的使命就是进入选中态，选中了就没它的事了。显隐跟随
                  播放控件（chromeShown），小屏不渲染（与互动栏同口径）。 */}
                {onWheelStep && currentSeries && !miniScreen && !inBinge && (
                  <ChromeSurface
                    shown={chromeShown}
                    data-wheel-block
                    className="absolute inset-x-3 bottom-14 z-10"
                  >
                    <button
                      type="button"
                      onClick={(e) => {
                        e.stopPropagation();
                        // 选中这部剧 = 连播锁定（切集）；同为「用户在场」信号，
                        // 唤醒悬浮层让左上角连播指示亮出来
                        wakeChrome();
                        setBinge(seriesId);
                      }}
                      className="flex w-full cursor-pointer items-center justify-center gap-2 rounded-lg bg-black/60 px-3 py-2.5 text-sm text-white backdrop-blur-sm transition-colors hover:bg-black/75"
                    >
                      <span className="truncate">
                        {t('player.watchFullSeries')}
                        {currentSeries.episodeCount > 0 && (
                          <span className="text-white/70">
                            {' '}
                            · {tf('player.totalEpisodes', { count: currentSeries.episodeCount })}
                          </span>
                        )}
                      </span>
                      <ChevronRight className="size-4 shrink-0 text-white/60" aria-hidden />
                    </button>
                  </ChromeSurface>
                )}
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
                ) : portrait ? (
                  // 竖屏窄条：控件对齐小窗紧凑条（portrait 形态）——全量控制栏
                  // 在竖条里必然溢出（选集浮层 460 宽更是放不下），hgplayer 竖屏
                  // 同样是紧凑一行。⤢ = 退出竖屏；✕ 的位置给铺满切换
                  <MiniScreenControls
                    variant="portrait"
                    videoRef={videoRef}
                    total={currentSeries?.episodes.length ?? 0}
                    hasNext={
                      currentSeries?.episodes.some((e) => e.vidIndex === (vidIndex ?? 0) + 1) ??
                      false
                    }
                    onStepEpisode={(d) => stepEpisode(d)}
                    onExpand={togglePortrait}
                    incognitoOn={incognito.on}
                    onToggleIncognito={incognito.toggle}
                    portraitFull={portraitFull}
                    onTogglePortraitFull={togglePortraitFull}
                    visible={chromeShown}
                    onControlsEnter={() => {
                      setControlsHovered(true);
                      wakeChrome();
                    }}
                    onControlsLeave={() => setControlsHovered(false)}
                  />
                ) : (
                  <PlayerControls
                    videoRef={videoRef}
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
                    onEnterPortrait={togglePortrait}
                    incognito={incognito.on}
                    onToggleIncognito={incognito.toggle}
                    definition={activeDefinition}
                    definitions={definitions}
                    onDefinitionChange={(d) => {
                      // 换档同样换流换元素：先定格当前帧，档位间隙不再闪封面
                      captureFrame();
                      setDefinition(d);
                    }}
                    src={playSrc}
                    danmakuOn={danmakuOn}
                    onToggleDanmaku={toggleDanmaku}
                    danmakuDisplay={danmakuDisplay}
                    onDanmakuDisplayChange={updateDanmakuDisplay}
                    immersive
                    visible={chromeShown}
                    // 选中剧的入口只有「观看完整短剧」横条与自动连播完成本集
                    // ——选集面板只负责跳到那一集，不锁连播
                    onPickEpisode={(idx) => {
                      setTarget(seriesId, idx);
                    }}
                  />
                )}

                {/* 兜底转码浮层。转一集要几十秒，没有它用户只能盯着黑屏，
                  不知道是卡住了还是在慢慢转。z-30：压过封面占位。 */}
                {compat && <CompatOverlay compat={compat} />}

                {/* 无 HEVC 扩展机器的安装引导：
                  - 底部悬浮（proactive）：降档播放时提示装扩展提画质；
                  - 居中主卡（blocking）：解码失败后必须二选一——装扩展
                    （10–30 秒，全剧原生秒开）或转兼容格式（本集分钟级）。
                  装好且画面没在播时借 retryOnline 换 URL 通道重试起播。 */}
                {hevcPlayable === false && !compat && !compatSrc && (
                  <HevcInstallCard
                    hevcPlayable={false}
                    variant={decodeBlocking ? 'blocking' : 'floating'}
                    onInstalled={() => {
                      if (!videoLive) retryOnline();
                    }}
                    onChooseCompat={() => {
                      setDecodeBlocked(null);
                      startCompat();
                    }}
                  />
                )}

                {/* 加载反馈 = 顶部 2px 细进度条（hg-loadbar，样式见 index.css）：
                  切换在途/首帧未出/缓冲中任何一种未就绪都亮。中央的
                  「正在缓冲 X%」胶囊按用户要求移除——它压在画面正中
                  挡内容；细条贴边滑过，反馈有了、打扰没了。 */}
                {(switching || !videoLive || videoStalled) && !compat && <Loadbar />}
              </>
            ) : (
              <>
                {coverBackdrop}
                <LoadingPanel error={error} buffering={buffering} retryOnline={retryOnline} />
              </>
            )}
          </div>
          {/* 末帧定格层：渲染在切换动画容器外（定格帧是静止参考帧，不跟
              新内容滑入），同剧切集的流交换间隙由它顶住直到新集首帧出画 */}
          {heldFrame && <FrameHold frame={heldFrame} hidden={!holdVisible} cover={miniScreen} />}
        </div>

        <ErrorBar chromeShown={chromeShown} error={error} />
      </div>
    </div>
  );
}
