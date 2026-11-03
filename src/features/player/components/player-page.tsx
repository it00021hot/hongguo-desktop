import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { useNavigate, useRouter } from '@tanstack/react-router';
import { ChevronRight, Loader2 } from 'lucide-react';
import { toast } from 'sonner';
import { DanmakuLayer } from './danmaku-layer';
import { EnhancementLayer } from './enhancement-layer';
import { PlayerControls } from './controls/player-controls';
import { MiniScreenControls } from './mini-screen-controls';
import { PhonePlayerChrome } from './phone-player-chrome';
import { PhoneDanmakuSend } from './phone-danmaku-send';
import { PhoneSeriesSheet } from './phone-series-sheet';
import { DownloadSheet } from './download-sheet';
import { InteractionRail } from './interaction-rail';
import { CommentPanel } from './comment-panel';
import { ChromeSurface } from './chrome-surface';
import { CompatOverlay } from './compat-overlay';
import { CoverBackdrop } from './cover-backdrop';
import { ErrorBar } from './error-bar';
import { FrameHold } from './frame-hold';
import { HevcInstallCard } from './hevc-install-card';
import { ImmersiveInfoOverlay, PlayerChromePills } from './immersive-info-overlay';
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
import { useSwitchVeil } from '../hooks/use-switch-veil';
import { usePlaybackProgress } from '../hooks/use-playback-progress';
import { useDanmakuSettings } from '../hooks/use-danmaku-settings';
import { useEnhancement } from '../hooks/use-enhancement';
import { useBingeRelay } from '../hooks/use-binge-relay';
import { usePlayerOverlay } from '../hooks/use-player-overlay';
import { usePlayerInteractions } from '../hooks/use-player-interactions';
import { useMiniWindow } from '../hooks/use-mini-window';
import { usePortraitWindow } from '../hooks/use-portrait-window';
import { isAutoAdvanceStart, isPickerClickCooldown, markAutoAdvance } from '../playback-signals';
import { usePinWindow } from '@/hooks/use-pin-window';
import { useNarrowScreen } from '@/hooks/use-match-media';

/** 自动连播静默硬上限：主闸门是「首播之前不唤醒」，这只兜起播失败的死局。 */
const QUIET_WINDOW_MS = 8_000;

export function PlayerView({
  onWheelStep,
  coverUrl,
  overlayMeta,
  onFirstFrame,
}: {
  /**
   * 滚轮/↑↓ 的语义由宿主页给：首页（沉浸流）= 切上一部/下一部剧；不给则回落切上一集/下一集。
   */
  onWheelStep?: (dir: 1 | -1) => void;
  /**
   * 封面占位图：切剧/起播的取流间隙垫在画面上，首帧出画（playing）后交叉淡出——
   * 官方「滚动秒切」的观感一半来自这里，黑屏等待变成封面常驻。
   */
  coverUrl?: string;
  /** 信息流条目自带的展示标记（档案接口没有这几个字段，宿主页按当前 seriesId 从流条目挑出递进来）。 */
  overlayMeta?: { heatText?: string; seasonTag?: string; badge?: string };
  /**
   * 首帧出画回调：沉浸流宿主据此给「预取下一部」发令。onPlaying 在个别 WebView
   * 起播路径不触发，timeupdate 出画兜底同样会调用；重复调用无副作用。
   */
  onFirstFrame?: () => void;
}) {
  const navigate = useNavigate();
  // routeTree.gen 的 as any 污染 useRouter 返回值，这里只用 history.push
  const router = useRouter() as { history: { push: (href: string) => void } };
  const returnTo = usePlayerStore((s) => s.returnTo);
  const setReturnTo = usePlayerStore((s) => s.setReturnTo);
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
   * 自动连播静默起步：handleEnded 切集打了点，这次挂载若是它带来的，
   * 悬浮层以「无人操作」起步（不亮控制栏/光标）；useState 初始化器只在挂载时求值一次。
   */
  const [quietStart] = useState(isAutoAdvanceStart());
  /** 静默期开关（喂给 usePlayerOverlay 的 silenced，一票否决）：从自动连播打点到新集首播或用户真实输入为止。 */
  const [quietArmed, setQuietArmed] = useState(quietStart);
  /**
   * 静默窗硬上限兜底：切集会向新元素补发幻影 mouseenter/mousemove，主闸门是
   * 「首播之前一律静默」，这扇定时窗只兜自动起播失败的情形（超时后照常唤醒）。
   * 布防放 useLayoutEffect：先于浏览器派发幻影事件的任务。
   */
  const quietUntilRef = useRef(0);
  useLayoutEffect(() => {
    if (quietStart) quietUntilRef.current = Date.now() + QUIET_WINDOW_MS;
  }, [quietStart]);
  /**
   * 最近一次切换方向（1=下一个，-1=上一个）：流地址变化的那次渲染据此决定新内容
   * 从下边还是上边滑入。事件写 state 而不是 ref——切换动作到流交换之间必隔一次渲染，渲染期读 ref 会被 react-hooks 拦。
   */
  const [slideDir, setSlideDir] = useState<1 | -1>(1);
  const seriesId = usePlayerStore((s) => s.seriesId);
  const vidIndex = usePlayerStore((s) => s.vidIndex);
  const setTarget = usePlayerStore((s) => s.setTarget);
  const episodeKey = seriesId && vidIndex ? `${seriesId}:${vidIndex}` : '';
  /**
   * 本集是否已真正播起来过：首播前的 pause 是新元素装载噪声不算暂停（见 onPause）；
   * 首播前移动类事件不唤醒悬浮层（幻影 mousemove）。归零点在 handleEnded（同步、事件处理器内），
   * 信息流里 PlayerView 跨集复用，effect 归零跑得比幻影事件慢。
   */
  const playedOnceRef = useRef(false);

  const { data: settings } = useSettings();
  const { deleteEpisode } = useStorageActions();
  // 设置没加载完时按默认值走：连播默认开、看完自动删默认关（与后端 Settings::default 一致）
  const autoNext = settings?.autoNextEpisode ?? true;
  const autoDelete = settings?.autoDeleteAfterPlay ?? false;

  // 取流/起播/断供重试/取流进度订阅与媒体偏好贴回抽在 use-playback-source；srcKeyRef/lastKnownRef 真值在那边持有
  const {
    src,
    setSrc,
    error,
    setError,
    autoRetry,
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
  /** 窄屏 Sheet 开合提到本层，避免切集 key 重挂丢掉 */
  const [phoneMoreOpen, setPhoneMoreOpen] = useState(false);
  const [phoneSeriesOpen, setPhoneSeriesOpen] = useState(false);
  const [phoneEnhanceOpen, setPhoneEnhanceOpen] = useState(false);
  const [phoneDanmakuSendOpen, setPhoneDanmakuSendOpen] = useState(false);
  /** 片源竖屏态带集指纹：切集后未出 metadata 前不算竖屏 → narrow 走 contain，避免横屏被旧态裁切 */
  const [portraitByKey, setPortraitByKey] = useState<{ key: string; on: boolean }>({
    key: '',
    on: false,
  });
  const videoPortrait = portraitByKey.key === episodeKey && portraitByKey.on;

  // 选集与「下载到本地」都要完整分集表，从剧集档案直接取
  const { data: currentSeries } = useSeriesEpisodes(seriesId);
  // 简介只在沉浸流信息叠加里用（播放页右侧面板自己拉）
  const { data: meta } = useSeriesDetailMeta(seriesId ?? '');

  // 弹幕：vid 来自剧集档案，滚窗协议首窗起点 = 当前播放位置（metadata 就绪采样），
  // 后续窗由下面的推进 effect 在播放位置逼近已拉边界时预取（领先 10s）
  const currentVid = currentSeries?.episodes.find((e) => e.vidIndex === vidIndex)?.vid ?? '';
  // 当前集的公开计数（detail 接口下发；右栏 ♥/💬 数字）
  const currentEpisode = currentSeries?.episodes.find((e) => e.vidIndex === vidIndex);
  // 首窗起点：metadata 就绪前 null；换集即重置（渲染期重置是 React 官方模式，不落地旧状态）
  const [danmakuStartVid, setDanmakuStartVid] = useState('');
  const [danmakuStartSecs, setDanmakuStartSecs] = useState<number | null>(null);
  if (danmakuStartVid !== currentVid) {
    setDanmakuStartVid(currentVid);
    setDanmakuStartSecs(null);
  }
  const handleLoadedMetadataWithDanmaku = useCallback(() => {
    handleLoadedMetadata();
    const video = videoRef.current;
    if (video && video.videoWidth > 0 && video.videoHeight > 0 && episodeKey) {
      setPortraitByKey({ key: episodeKey, on: video.videoHeight >= video.videoWidth });
    }
    setDanmakuStartSecs(Math.floor(video?.currentTime ?? 0));
  }, [handleLoadedMetadata, videoRef, episodeKey]);
  const danmakuQuery = useDanmaku(
    currentVid ? `${currentVid}:${seriesId}` : '',
    currentEpisode?.duration ?? 0,
    danmakuStartSecs,
  );
  const danmakuItems = useMemo(
    () => danmakuQuery.data?.pages.flatMap((p) => p.items) ?? [],
    [danmakuQuery.data],
  );
  // 滚窗推进：播放位置 + 领先量触到已拉 frontier 的下一窗起点就续拉。
  // 依赖只收原始值——query 对象每次渲染都是新身份，进依赖会让 interval 永远活不到触发
  const danmakuNextStartMs = danmakuQuery.data?.pages.at(-1)?.nextStartMs ?? 0;
  const danmakuHasNext = danmakuQuery.hasNextPage;
  const danmakuFetchingNext = danmakuQuery.isFetchingNextPage;
  const danmakuQueryRef = useRef(danmakuQuery);
  useEffect(() => {
    danmakuQueryRef.current = danmakuQuery;
  });
  useEffect(() => {
    if (!danmakuHasNext || danmakuFetchingNext || !danmakuNextStartMs) return;
    const timer = setInterval(() => {
      const t = videoRef.current?.currentTime;
      if (t != null && t * 1000 + 10_000 >= danmakuNextStartMs) {
        void danmakuQueryRef.current.fetchNextPage();
      }
    }, 1000);
    return () => clearInterval(timer);
  }, [danmakuHasNext, danmakuFetchingNext, danmakuNextStartMs, videoRef]);
  // 无 HEVC 扩展机器的安装引导数据源（false 才弹卡）
  const { data: hevcPlayable } = usePlaybackCapability();
  /** 解码失败待裁决：带 key 读时过滤而非换集清空（同步 setState 会触发级联渲染）。 */
  const [decodeBlocked, setDecodeBlocked] = useState<{ key: string } | null>(null);
  const { danmakuOn, danmakuDisplay, updateDanmakuDisplay, toggleDanmaku } = useDanmakuSettings();
  const {
    settings: enhanceSettings,
    update: updateEnhancement,
    status: enhanceStatus,
    setStatus: setEnhanceStatus,
  } = useEnhancement();
  // 弹幕拉取失败不能静默：画面照常播，但用户该知道弹幕为什么没了
  useEffect(() => {
    if (danmakuQuery.isError) {
      toast.error(tf('player.danmakuFailed', { reason: String(danmakuQuery.error) }));
    }
  }, [danmakuQuery.isError, danmakuQuery.error]);

  // 转码兜底与最终供流地址裁决抽在 use-transcode-fallback；裁决入口给 hook 一个恒定代理，
  // 真正实现依赖 hook 回传的 startCompat，定义在 hook 之后经 ref 同步
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

  // 解码失败的裁决：无 HEVC 机器先出主卡问「装扩展还是转兼容」（会话已授权转兼容的径直兜底）；其余直接转
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

  // 失败账本：本集各类错误已发生几次（recovery-action 升级用）。ref 不驱动渲染，带集 key 读时校验换集自然归零
  const failuresRef = useRef<{ key: string; counts: FailureCounts } | null>(null);
  const failuresOf = () =>
    failuresRef.current?.key === episodeKey ? failuresRef.current.counts : emptyFailureCounts();
  const recordFailure = (kind: MediaErrorKind) => {
    const counts = failuresOf();
    failuresRef.current = { key: episodeKey, counts: { ...counts, [kind]: counts[kind] + 1 } };
  };

  // 剧终三级接力与连播锁定抽在 use-binge-relay；slideDir 是过渡动画的方向信号
  const { inBinge, setBinge, advanceAfterSeriesEnd, stepEpisode } = useBingeRelay({
    seriesId,
    vidIndex,
    setTarget,
    setSlideDir,
    downloading,
    onWheelStep,
    currentSeries,
  });

  // 隐身模式与控制栏共享状态
  const incognito = useIncognitoMode(videoRef);

  // 最小化自动暂停（设置可关），与隐身模式的欠账模型各自独立
  useMinimizeAutoPause({
    videoRef,
    enabled: settings?.pauseOnMinimize ?? true,
  });

  // 沉浸流悬浮层显隐状态机抽在 use-player-overlay（静默期 silenced 一票否决）
  const { paused, setPaused, chromeShown, wakeChrome, hideChrome, setControlsHovered } =
    usePlayerOverlay({
      seriesPanelOpen,
      commentPanelOpen,
      danmakuPanelOpen,
      volumeOpen,
      phoneMoreOpen,
      phoneSeriesOpen,
      phoneEnhanceOpen,
      phoneDanmakuSendOpen,
      startVisible: !quietStart,
      silenced: quietArmed,
    });

  // 静默期的两类唤醒闸门：silenced 在源头兜底，这里再拦一层让 wakeChrome 连状态都不被噪声污染；
  // 点击/滚轮/按键是铁证，解除静默并正常唤醒
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
   * 当前流是否已出画 / 正在卡顿，都带流指纹（streamKey）：信息流的 PlayerView
   * 复用不随切剧重建，不带指纹上一部剧的状态会漏给下一部；读时校验换流即失效。
   */
  const [live, setLive] = useState<{ key: string; on: boolean }>({ key: '', on: false });
  const [stalled, setStalled] = useState<{ key: string; on: boolean }>({ key: '', on: false });
  const videoLive = live.key === streamKey && live.on;
  /** waiting 后时间轴恢复推进（timeupdate）或重新出画即视为不卡 */
  const videoStalled = stalled.key === streamKey && stalled.on;

  // 同剧切集黑幕暗转：置位/收口在 use-switch-veil，显隐真值表在 use-frame-hold
  const { veiled } = useSwitchVeil({
    seriesId,
    episodeKey,
    videoLive,
    error,
    compatActive: compat != null,
  });
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
    veiled,
  });

  // 舞台交互四套裁决抽在 use-player-interactions
  const { onStageWheel, onStageTouchStart, onStageTouchEnd, onStageClick } = usePlayerInteractions({
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

  /** 选集浮层刚关闭的一瞬，跟手点击会落到舞台上成为无意识播放/暂停切换，短窗内忽略。 */
  const onStageClickGuarded = useCallback(
    (e: React.MouseEvent) => {
      if (isPickerClickCooldown()) return;
      onStageClick(e);
    },
    [onStageClick],
  );

  // 进度持久化抽在 use-playback-progress：report 走播放位置心跳（节拍独立于落盘节流），persist 只管落盘与收尾上报
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

  // 小屏进出抽在 use-mini-window；置顶开关抽在 src/hooks/use-pin-window 与 AppShell 顶栏共用
  const { miniScreen, enterMini, exitMini } = useMiniWindow({
    setControlsHovered,
    setCommentPanelOpen,
    setDanmakuPanelOpen,
    setVolumeOpen,
    setSeriesPanelOpen,
  });
  // 竖屏进出/重适配/铺满档全在本 hook，PlayerControls 不再自持竖屏态
  const { portrait, portraitFull, togglePortrait, togglePortraitFull } = usePortraitWindow({
    videoRef,
  });
  // 无边框播放：这里只消费状态（拖拽条显隐），开关在控制栏
  const frameless = useUiStore((s) => s.playerFrameless);
  const { pinned, togglePinned } = usePinWindow();
  // 窄于 md：手机端 / 缩到手机宽的桌面窗——全量控制栏必然溢出，画面用 cover 填满
  const narrow = useNarrowScreen();
  // 小窗一律 cover；窄屏仅竖屏片源 cover，横屏 contain（完整画面 +「全屏观看」沉浸放大舞台）
  const coverFill = miniScreen || (narrow && videoPortrait);

  // 返回来源页（usePlaySeries 进场时记的路由），消费即清
  const handleReturnToSource = useCallback(() => {
    const target = usePlayerStore.getState().returnTo;
    setReturnTo(null);
    if (target) router.history.push(target);
  }, [router, setReturnTo]);

  const handleEnded = () => {
    // 该函数在 null 收窄之前定义，seriesId/vidIndex 需要自行判空
    const video = videoRef.current;
    if (!seriesId || !vidIndex || !video) return;
    // 评论面板开着 = 用户在读评论：循环重播本集，不自动连播、不落「看完」进度（官方同款）
    if (commentPanelOpen) {
      video.currentTime = 0;
      void video.play();
      return;
    }
    persist(video.currentTime, true);
    // 看完自动删：先清掉刚看完这集的本地文件再连播下一集；没下载过的集后端返回 false 不提示
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
    // 下载面板开着就连播：切集会让 PlayerView 整体重挂载，面板连同勾选一起消失。
    // 末集播完不是「没有下一集」，是剧终：走剧终接力
    if (autoNext && !downloading) {
      // 自动连播打点 + 静默窗续上（PlayerView 复用不重挂载）+ playedOnce 同步归零
      // （被动 effect 归零跑得比幻影事件慢，会把上一集的「已播过」漏给新一集）
      markAutoAdvance();
      playedOnceRef.current = false;
      quietUntilRef.current = Date.now() + QUIET_WINDOW_MS;
      setQuietArmed(true);
      const total = currentSeries?.episodes.length ?? 0;
      if (total > 0 && vidIndex >= total) advanceAfterSeriesEnd();
      else {
        // 自动连播完成本集 = 进入选中剧连播态，滚轮/↑↓ 就该切集
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
    // 在线渐进流断供可自愈：每类错误各有一次重试预算（recovery-policy），用完才升级——
    // decode 升转码兜底、其余落错误页；本地文件/转码产物坏了直接报错
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
    // store 目标恢复前的瞬态：中性黑场 + 加载圈（瞬态就该长得像瞬态）
    return (
      <div className="grid h-full place-items-center bg-black">
        <Loader2 className="size-6 animate-spin text-white/40" aria-label={t('common.loading')} />
      </div>
    );
  }

  // 封面占位：只属于切剧接管与画面真空（显隐真值在 use-frame-hold 的 overlayVisibility），切集不再每次闪封面
  const coverBackdrop = coverUrl ? (
    <CoverBackdrop key={coverUrl} src={coverUrl} hidden={holdCoverHidden} />
  ) : null;

  return (
    // 全模式一个视觉：视频区全出血贴边；选集走控制栏的「选集」弹层，不维护第二套侧栏布局
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex min-w-0 flex-1 flex-col">
        <div
          onWheel={onStageWheel}
          onTouchStart={onStageTouchStart}
          onTouchEnd={onStageTouchEnd}
          onClick={onStageClickGuarded}
          // 「移入显示」显式接 mouseenter：跨窗口边界缓入时浏览器可能不派发 mousemove
          onMouseEnter={wakeOnMove}
          onMouseMove={wakeOnMove}
          // 点击也算在场：点控制栏按钮后鼠标未必再动
          onPointerDown={wakeOnInput}
          // 「移出隐藏」立即收起，不等倒计时
          onMouseLeave={hideChrome}
          className={cn(
            'relative min-h-0 flex-1 overflow-hidden bg-black',
            // 悬浮层收起后光标跟着藏；动一下鼠标先唤醒悬浮层，光标随之回来
            !chromeShown && 'cursor-none',
          )}
        >
          {/* 小屏/无边框都没有顶栏：拖拽条顶上（无边框模式下它是唯一的移窗抓手） */}
          {(miniScreen || frameless) && <MiniDragBar />}
          {/* 抖音式切换过渡：key 绑「实际供数的流」（取流中旧流继续播，动画精确落在
              新内容出画那一帧），内容按方向滑入 + 淡入。方向在事件里先写进 state
              再换目标，切换动作到流地址交换之间必隔一次渲染。 */}
          <div
            key={playSrc ?? episodeKey}
            className={cn(
              'animate-in fade-in absolute inset-0 duration-300 ease-out',
              slideDir === -1 ? 'slide-in-from-top-10' : 'slide-in-from-bottom-10',
            )}
          >
            {playSrc ? (
              <>
                {/* 自绘控件，不要原生 controls：它既不跟主题，也放不下业务动作。 */}
                <video
                  /* key 绑播放地址：换元素才能保证 <video> 真的重新加载——只改 src
                     属性在 WebView2 上不一定触发重载（点了 720p 画面不变）。 */
                  key={playSrc}
                  ref={videoRef}
                  src={playSrc}
                  /* crossOrigin=anonymous：画质增强的 WebGPU 帧上传要求画面 origin-clean。
                     流经自定义协议（对页面是跨源），协议响应带 ACAO 头时 crossorigin
                     请求通过 CORS 检查、保持不 taint——两处缺一不可。 */
                  crossOrigin="anonymous"
                  /* 绝对定位铺满舞台：WKWebView 在流式布局里会把 100% 高度的 <video>
                     按固有纵横比撑大（舞台裁切后「画面放大、控件全被顶出窗口」）。
                     绝对定位把盒子钉死在舞台内，object-contain 只管留黑边。 */
                  className={cn(
                    'absolute inset-0 size-full',
                    // 小屏 / 手机窄舞台 object-cover：窗形与片源常不一致，
                    // contain 会留大片黑边，裁切比黑边可取
                    coverFill ? 'object-cover' : 'object-contain',
                  )}
                  autoPlay
                  onLoadedMetadata={handleLoadedMetadataWithDanmaku}
                  onPlay={() => {
                    // 不在 play 事件唤醒悬浮层：它分不清用户动作与切集自动起播，
                    // 后者亮操作栏正是「自动下一集抢占鼠标」的来源。只同步播放态
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
                    // 首播前的 pause 是新元素装载噪声（会把「暂停常显」点亮，
                    // 正是「自动切集亮控件」的另一半），静默起步 UI 本就视同在播，忽略
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
                    // playing 事件在个别 WebView 起播路径不触发：时间轴走起来也算出画
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
                {/* 画质增强层：紧跟 <video>；available 绑 videoLive——取流间隙自动空转、出画自动恢复 */}
                <EnhancementLayer
                  videoRef={videoRef}
                  src={playSrc}
                  available={videoLive}
                  cover={coverFill}
                  settings={enhanceSettings}
                  onStatus={setEnhanceStatus}
                />
                {coverBackdrop}
                {/* 切集/首帧间隙的中央 loading：此刻必然还没有画面（播放中的缓冲有末帧可看，
                  不在此列），黑屏等待给个明确的「在加载」信号 */}
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
                  items={danmakuItems}
                  enabled={danmakuOn}
                  display={danmakuDisplay}
                />
                <ImmersiveInfoOverlay
                  chromeShown={chromeShown && !phoneDanmakuSendOpen}
                  miniScreen={miniScreen}
                  narrow={narrow && !portrait}
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
                  returnTo={returnTo}
                  onReturnToSource={handleReturnToSource}
                  onOpenDanmakuSend={
                    narrow && !portrait
                      ? () => {
                          setPhoneDanmakuSendOpen(true);
                          wakeChrome();
                        }
                      : undefined
                  }
                />
                {/* 快捷键提示只在暂停时露出；常驻顶栏会把分类 tab 挡死。小屏/竖屏/窄屏装不下 */}
                {paused && !error && !miniScreen && !portrait && !narrow && <KeyboardHint />}
                {/* 沉浸流分类 tab 栏已上移 AppShell 顶栏（portal 插槽） */}
                {/* 沉浸流评论区：右侧滑出（💬 触发）；进小屏时已收起 */}
                {commentPanelOpen && !miniScreen && (
                  <CommentPanel
                    vid={currentVid ? `${currentVid}:${seriesId}` : ''}
                    onClose={() => setCommentPanelOpen(false)}
                  />
                )}
                {/* 互动栏（点赞/评论/收藏/预约/分享）：跟随悬浮层淡出，评论区面板打开时让位隐藏。
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
                {/* 沉浸流底部「观看完整短剧 · 全N集」横条：进入选中剧连播态的唯一手动入口，
                  已选中时不渲染。显隐跟随播放控件，小屏不渲染。 */}
                {onWheelStep && currentSeries && !miniScreen && !inBinge && (
                  <ChromeSurface
                    shown={chromeShown}
                    data-wheel-block
                    className={cn(
                      'absolute inset-x-3 z-10',
                      narrow && !portrait ? 'bottom-10' : 'bottom-14',
                    )}
                  >
                    <button
                      type="button"
                      onClick={(e) => {
                        e.stopPropagation();
                        // 选中 = 连播锁定；同为「用户在场」信号，唤醒悬浮层
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
                {/* 控制栏：小屏 / 竖屏 / 手机窄舞台用紧凑条，宽屏才上全量栏；
                    共用同一个 <video>，切换形态播放零中断 */}
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
                  // 竖屏窄条：控件对齐小窗紧凑条；⤢ = 退出竖屏
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
                ) : narrow ? (
                  // 窄屏官方同款：右上倍速 + 底三键/细进度；Sheet 提到本层防切集重挂
                  <>
                    <PhonePlayerChrome
                      videoRef={videoRef}
                      visible={chromeShown && !phoneDanmakuSendOpen}
                      moreOpen={phoneMoreOpen}
                      onMoreOpenChange={setPhoneMoreOpen}
                      enhanceOpen={phoneEnhanceOpen}
                      onEnhanceOpenChange={setPhoneEnhanceOpen}
                      onOpenSeries={() => {
                        setPhoneSeriesOpen(true);
                        if (!inBinge) setBinge(seriesId);
                      }}
                      episodeCount={currentSeries?.episodeCount ?? 0}
                      definition={activeDefinition}
                      definitions={definitions}
                      onDefinitionChange={(d) => {
                        captureFrame();
                        setDefinition(d);
                      }}
                      danmakuOn={danmakuOn}
                      onToggleDanmaku={toggleDanmaku}
                      danmakuDisplay={danmakuDisplay}
                      onDanmakuDisplayChange={updateDanmakuDisplay}
                      enhanceSettings={enhanceSettings}
                      onEnhanceChange={updateEnhancement}
                      enhanceStatus={enhanceStatus}
                      enhanceAvailable={videoLive}
                      onDownloadingChange={setDownloading}
                      onControlsEnter={() => {
                        setControlsHovered(true);
                        wakeChrome();
                      }}
                      onControlsLeave={() => setControlsHovered(false)}
                    />
                    <PhoneDanmakuSend
                      open={phoneDanmakuSendOpen}
                      onOpenChange={setPhoneDanmakuSendOpen}
                      videoRef={videoRef}
                      vid={currentVid ? `${currentVid}:${seriesId}` : ''}
                    />
                    <PhoneSeriesSheet
                      open={phoneSeriesOpen}
                      onOpenChange={setPhoneSeriesOpen}
                      seriesId={seriesId}
                      title={currentSeries?.title}
                      cover={currentSeries?.cover}
                      intro={meta?.intro}
                      episodeCount={currentSeries?.episodeCount ?? 0}
                      currentIndex={vidIndex}
                      onPickEpisode={(idx) => {
                        setTarget(seriesId, idx);
                      }}
                    />
                    <DownloadSheet
                      seriesId={seriesId}
                      episodes={currentSeries?.episodes ?? []}
                      defaultSelected={vidIndex ? [vidIndex] : []}
                      open={downloading}
                      onOpenChange={setDownloading}
                    />
                  </>
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
                    enhanceSettings={enhanceSettings}
                    onEnhanceChange={updateEnhancement}
                    enhanceStatus={enhanceStatus}
                    enhanceAvailable={videoLive}
                    immersive
                    visible={chromeShown}
                    // 选集面板只负责跳到那一集，不锁连播
                    onPickEpisode={(idx) => {
                      setTarget(seriesId, idx);
                    }}
                  />
                )}

                {/* 兜底转码浮层：转一集几十秒，没有它用户只能盯着黑屏 */}
                {compat && <CompatOverlay compat={compat} />}

                {/* HEVC 安装引导：底部悬浮（proactive，降档播放时提示）与居中主卡
                  （blocking，解码失败必须二选一）两种形态；
                  装好且画面没在播时借 retryOnline 重试起播。 */}
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

                {/* 加载反馈 = 顶部 2px 细进度条：切换在途/首帧未出/缓冲中任何一种未就绪都亮 */}
                {(switching || !videoLive || videoStalled) && !compat && <Loadbar />}
              </>
            ) : (
              <>
                {coverBackdrop}
                {/* 错误/加载态的导航出口：取流失败时信息叠加层与 Esc 都不在，pill 常显 */}
                <PlayerChromePills
                  chromeShown={chromeShown}
                  miniScreen={miniScreen}
                  always
                  vidIndex={vidIndex}
                  returnTo={returnTo}
                  onReturnToSource={handleReturnToSource}
                />
                <LoadingPanel
                  error={error}
                  autoRetry={autoRetry}
                  buffering={buffering}
                  retryOnline={retryOnline}
                />
              </>
            )}
          </div>
          {/* 末帧定格层：渲染在切换动画容器外（静止参考帧不跟新内容滑入）；
              同剧切集的间隙归黑幕，定格帧不参与 */}
          {heldFrame && <FrameHold frame={heldFrame} hidden={!holdVisible} cover={coverFill} />}
          {/* 同剧切集的黑幕：切集提交拍淡入盖住静止期，新集出画后淡出（z-25 让位控制栏与面板） */}
          <div
            aria-hidden
            className={cn(
              'pointer-events-none absolute inset-0 z-[25] bg-black transition-opacity duration-200 ease-out',
              veiled ? 'opacity-100' : 'opacity-0',
            )}
          />
        </div>

        <ErrorBar chromeShown={chromeShown} error={error} />
      </div>
    </div>
  );
}
