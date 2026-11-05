import { useCallback, useEffect, useState, useRef } from 'react';
import { useNavigate } from '@tanstack/react-router';
import { Loader2 } from 'lucide-react';
import { toast } from 'sonner';
import { DanmakuLayer } from './danmaku-layer';
import { EnhancementLayer } from './enhancement-layer';
import { setAndroidLandscape } from '@/utils/android-shell';
import { isAndroid } from '@/utils/platform';
import { applyWindowFullscreen } from '@/hooks/use-window-fullscreen';
import { InteractionRail } from './interaction-rail';
import { CommentPanel } from './comment-panel';
import { CoverBackdrop } from './cover-backdrop';
import { ErrorBar } from './error-bar';
import { FrameHold } from './frame-hold';
import { ImmersiveInfoOverlay } from './immersive-info-overlay';
import { KeyboardHint } from './keyboard-hint';
import { Loadbar } from './loadbar';
import { MiniDragBar } from './mini-drag-bar';
import { BingeEntryBar } from './binge-entry-bar';
import { PlayerStageChrome } from './player-stage-chrome';
import { IdleRecoveryPanel, PlayingRecoveryOverlays } from './player-recovery-ui';
import {
  usePlaybackCapability,
  useSeriesDetailMeta,
  useSeriesEpisodes,
  useSettings,
  useStorageActions,
} from '@/service/queries';
import { usePlayerStore } from '@/stores/player';
import { useUiStore } from '@/stores/ui';
import { t, tf } from '@/locales';
import { cn } from '@/lib/utils';
import { useIncognitoMode } from './incognito';
import { useMinimizeAutoPause } from '../hooks/use-minimize-pause';
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
import { useQuietStart } from '../hooks/use-quiet-start';
import { useDanmakuWindow } from '../hooks/use-danmaku-window';
import { useDecodeFailureProxy, useMediaRecovery } from '../hooks/use-media-recovery';
import { useVideoLive } from '../hooks/use-video-live';
import { isPickerClickCooldown, markAutoAdvance } from '../playback-signals';
import { usePinWindow } from '@/hooks/use-pin-window';
import { useNarrowScreen } from '@/hooks/use-match-media';

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
  const seriesPanelOpen = usePlayerStore((s) => s.seriesPanelOpen);
  const commentPanelOpen = usePlayerStore((s) => s.commentPanelOpen);
  const danmakuPanelOpen = usePlayerStore((s) => s.danmakuPanelOpen);
  const volumeOpen = usePlayerStore((s) => s.volumeOpen);
  const setCommentPanelOpen = usePlayerStore((s) => s.setCommentPanelOpen);
  const setDanmakuPanelOpen = usePlayerStore((s) => s.setDanmakuPanelOpen);
  const setVolumeOpen = usePlayerStore((s) => s.setVolumeOpen);
  const setSeriesPanelOpen = usePlayerStore((s) => s.setSeriesPanelOpen);
  const videoRef = useRef<HTMLVideoElement>(null);

  const {
    quietStart,
    quietArmed,
    playedOnceRef,
    armQuiet,
    markPlayed,
    wakeOnMove: quietWakeOnMove,
    wakeOnInput: quietWakeOnInput,
  } = useQuietStart();

  /**
   * 最近一次切换方向（1=下一个，-1=上一个）：流地址变化的那次渲染据此决定新内容
   * 从下边还是上边滑入。事件写 state 而不是 ref——切换动作到流交换之间必隔一次渲染，渲染期读 ref 会被 react-hooks 拦。
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

  // 取流/起播/断供重试/取流进度订阅与媒体偏好贴回抽在 use-playback-source
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

  // 选集与「下载到本地」都要完整分集表，从剧集档案直接取
  const { data: currentSeries } = useSeriesEpisodes(seriesId);
  // 简介只在沉浸流信息叠加里用（播放页右侧面板自己拉）
  const { data: meta } = useSeriesDetailMeta(seriesId ?? '');

  const currentVid = currentSeries?.episodes.find((e) => e.vidIndex === vidIndex)?.vid ?? '';
  const currentEpisode = currentSeries?.episodes.find((e) => e.vidIndex === vidIndex);
  const currentVidKey = currentVid ? `${currentVid}:${seriesId}` : '';

  const { danmakuItems, handleLoadedMetadataWithDanmaku, videoPortrait } = useDanmakuWindow({
    videoRef,
    currentVid,
    seriesId,
    duration: currentEpisode?.duration ?? 0,
    onLoadedMetadata: handleLoadedMetadata,
    episodeKey,
  });

  const { data: hevcPlayable } = usePlaybackCapability();
  const { danmakuOn, danmakuDisplay, updateDanmakuDisplay, toggleDanmaku } = useDanmakuSettings();
  const {
    settings: enhanceSettings,
    update: updateEnhancement,
    status: enhanceStatus,
    setStatus: setEnhanceStatus,
  } = useEnhancement();

  // 转码兜底：onDecodeFailure 代理须先于 hook 创建
  const { onDecodeFailure, onDecodeFailureRef } = useDecodeFailureProxy();
  const { compatSrc, compat, playSrc, streamKey, startCompat, forceCompat } = useTranscodeFallback({
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
    hevcPlayable,
    android: isAndroid(),
    onDecodeFailure,
  });
  /** 切换在途：目标已是新的一集，元素里还是上一条流（play 请求未返回） */
  const switching = srcKey !== episodeKey;

  const { decodeBlocking, handleVideoError, clearDecodeBlocked } = useMediaRecovery({
    videoRef,
    episodeKey,
    hevcPlayable,
    startCompat,
    forceCompat,
    playSrc,
    compatSrc,
    compat,
    error,
    setError,
    setSrc,
    retryOnline,
    onDecodeFailureRef,
  });

  // 剧终三级接力与连播锁定抽在 use-binge-relay
  const { inBinge, setBinge, advanceAfterSeriesEnd, stepEpisode } = useBingeRelay({
    seriesId,
    vidIndex,
    setTarget,
    setSlideDir,
    downloading,
    onWheelStep,
    currentSeries,
  });

  const incognito = useIncognitoMode(videoRef);

  useMinimizeAutoPause({
    videoRef,
    enabled: settings?.pauseOnMinimize ?? true,
  });

  const frameless = useUiStore((s) => s.playerFrameless);
  const setPlayerFrameless = useUiStore((s) => s.setPlayerFrameless);
  const narrow = useNarrowScreen();
  const landscapeWatch = frameless && narrow;
  const phoneDanmakuComposerOpen = phoneDanmakuSendOpen && !landscapeWatch;

  // 沉浸流悬浮层显隐状态机抽在 use-player-overlay
  const { paused, setPaused, chromeShown, wakeChrome, hideChrome, setControlsHovered } =
    usePlayerOverlay({
      seriesPanelOpen,
      commentPanelOpen,
      danmakuPanelOpen,
      volumeOpen,
      phoneMoreOpen,
      phoneSeriesOpen,
      phoneEnhanceOpen,
      phoneDanmakuSendOpen: phoneDanmakuComposerOpen,
      startVisible: !quietStart,
      silenced: quietArmed,
    });

  const wakeOnMove = useCallback(() => quietWakeOnMove(wakeChrome), [quietWakeOnMove, wakeChrome]);
  const wakeOnInput = useCallback(
    () => quietWakeOnInput(wakeChrome),
    [quietWakeOnInput, wakeChrome],
  );

  /** 简介展开态：切剧重挂载自然收回。 */
  const [introExpanded, setIntroExpanded] = useState(false);
  const { videoLive, videoStalled, markLive, markStalled } = useVideoLive(streamKey);

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

  const { miniScreen, enterMini, exitMini } = useMiniWindow({
    setControlsHovered,
    setCommentPanelOpen,
    setDanmakuPanelOpen,
    setVolumeOpen,
    setSeriesPanelOpen,
  });
  const { portrait, portraitFull, togglePortrait, togglePortraitFull } = usePortraitWindow({
    videoRef,
  });
  const { pinned, togglePinned } = usePinWindow();
  // 小窗一律 cover；窄屏仅竖屏片源 cover，横屏 contain
  const coverFill = miniScreen || (narrow && videoPortrait && !landscapeWatch);

  // 横屏全屏中切到竖片：官方也会退回竖屏信息流
  useEffect(() => {
    if (!frameless || !videoPortrait) return;
    setPlayerFrameless(false);
    setAndroidLandscape(false);
    void applyWindowFullscreen(false).catch(() => undefined);
  }, [frameless, videoPortrait, setPlayerFrameless]);

  const exitLandscapeWatch = useCallback(() => {
    setPlayerFrameless(false);
    setAndroidLandscape(false);
    setPhoneDanmakuSendOpen(false);
    void applyWindowFullscreen(false).catch(() => undefined);
  }, [setPlayerFrameless]);

  const onControlsEnter = useCallback(() => {
    setControlsHovered(true);
    wakeChrome();
  }, [setControlsHovered, wakeChrome]);
  const onControlsLeave = useCallback(() => setControlsHovered(false), [setControlsHovered]);

  const handleDefinitionChange = useCallback(
    (d: number | undefined) => {
      captureFrame();
      setDefinition(d);
    },
    [captureFrame, setDefinition],
  );

  const handleEnded = () => {
    const video = videoRef.current;
    if (!seriesId || !vidIndex || !video) return;
    // 评论面板开着 = 用户在读评论：循环重播本集，不自动连播、不落「看完」进度（官方同款）
    if (commentPanelOpen) {
      video.currentTime = 0;
      void video.play();
      return;
    }
    persist(video.currentTime, true);
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
    if (autoNext && !downloading) {
      markAutoAdvance();
      armQuiet();
      const total = currentSeries?.episodes.length ?? 0;
      if (total > 0 && vidIndex >= total) advanceAfterSeriesEnd();
      else {
        setBinge(seriesId);
        stepEpisode(1);
      }
    }
  };

  if (!seriesId || !vidIndex) {
    return (
      <div className="grid h-full place-items-center bg-black">
        <Loader2 className="size-6 animate-spin text-white/40" aria-label={t('common.loading')} />
      </div>
    );
  }

  const coverBackdrop = coverUrl ? (
    <CoverBackdrop key={coverUrl} src={coverUrl} hidden={holdCoverHidden} />
  ) : null;

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex min-w-0 flex-1 flex-col">
        <div
          onWheel={onStageWheel}
          onTouchStart={onStageTouchStart}
          onTouchEnd={onStageTouchEnd}
          onClick={onStageClickGuarded}
          onMouseEnter={wakeOnMove}
          onMouseMove={wakeOnMove}
          onPointerDown={wakeOnInput}
          onMouseLeave={hideChrome}
          className={cn(
            'relative min-h-0 flex-1 overflow-hidden bg-black',
            !chromeShown && 'cursor-none',
          )}
        >
          {(miniScreen || (frameless && !narrow)) && <MiniDragBar />}
          <div
            key={playSrc ?? episodeKey}
            className={cn(
              'animate-in fade-in absolute inset-0 duration-300 ease-out',
              slideDir === -1 ? 'slide-in-from-top-10' : 'slide-in-from-bottom-10',
            )}
          >
            {playSrc ? (
              <>
                <video
                  key={playSrc}
                  ref={videoRef}
                  src={playSrc}
                  crossOrigin="anonymous"
                  className={cn(
                    'absolute inset-0 size-full',
                    coverFill ? 'object-cover' : 'object-contain',
                  )}
                  autoPlay
                  onLoadedMetadata={handleLoadedMetadataWithDanmaku}
                  onPlay={() => {
                    markPlayed();
                    setPaused(false);
                  }}
                  onPlaying={() => {
                    markLive();
                    markStalled(false);
                    onFirstFrame?.();
                  }}
                  onPause={(e) => {
                    if (!playedOnceRef.current) return;
                    setPaused(true);
                    persist(e.currentTarget.currentTime, true);
                  }}
                  onLoadedData={() => markLive()}
                  onWaiting={() => markStalled(true)}
                  onCanPlay={() => markStalled(false)}
                  onTimeUpdate={(e) => {
                    if (e.currentTarget.currentTime > 0.1) {
                      markLive();
                      onFirstFrame?.();
                    }
                    markStalled(false);
                    persist(e.currentTarget.currentTime);
                    report(e.currentTarget.currentTime);
                  }}
                  onEnded={handleEnded}
                  onRateChange={handleRateChange}
                  onVolumeChange={handleVolumeChange}
                  onError={handleVideoError}
                />
                <EnhancementLayer
                  videoRef={videoRef}
                  src={playSrc}
                  available={videoLive}
                  cover={coverFill}
                  settings={enhanceSettings}
                  onStatus={setEnhanceStatus}
                />
                {coverBackdrop}
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
                {!landscapeWatch && (
                  <ImmersiveInfoOverlay
                    chromeShown={chromeShown && !phoneDanmakuComposerOpen}
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
                    onOpenDanmakuSend={
                      narrow && !portrait
                        ? () => {
                            setPhoneDanmakuSendOpen(true);
                            wakeChrome();
                          }
                        : undefined
                    }
                  />
                )}
                {paused && !error && !miniScreen && !portrait && !narrow && <KeyboardHint />}
                {commentPanelOpen && !miniScreen && (
                  <CommentPanel
                    vid={currentVidKey}
                    onClose={() => setCommentPanelOpen(false)}
                  />
                )}
                <InteractionRail
                  seriesId={seriesId}
                  vid={currentVidKey}
                  visible={
                    chromeShown && !commentPanelOpen && !miniScreen && !landscapeWatch
                  }
                  title={currentSeries?.title}
                  commentCount={currentEpisode?.commentCount}
                  diggCount={currentEpisode?.diggCount}
                  followCount={currentSeries?.followedCnt}
                  seriesStatus={meta?.seriesStatus}
                />
                {onWheelStep && currentSeries && !miniScreen && !inBinge && !landscapeWatch && (
                  <BingeEntryBar
                    chromeShown={chromeShown}
                    narrow={narrow}
                    portrait={portrait}
                    episodeCount={currentSeries.episodeCount}
                    onEnterBinge={() => {
                      wakeChrome();
                      setBinge(seriesId);
                    }}
                  />
                )}
                <PlayerStageChrome
                  videoRef={videoRef}
                  miniScreen={miniScreen}
                  portrait={portrait}
                  portraitFull={portraitFull}
                  narrow={narrow}
                  landscapeWatch={landscapeWatch}
                  chromeShown={chromeShown}
                  phoneDanmakuComposerOpen={phoneDanmakuComposerOpen}
                  phoneMoreOpen={phoneMoreOpen}
                  setPhoneMoreOpen={setPhoneMoreOpen}
                  phoneSeriesOpen={phoneSeriesOpen}
                  setPhoneSeriesOpen={setPhoneSeriesOpen}
                  phoneEnhanceOpen={phoneEnhanceOpen}
                  setPhoneEnhanceOpen={setPhoneEnhanceOpen}
                  setPhoneDanmakuSendOpen={setPhoneDanmakuSendOpen}
                  seriesId={seriesId}
                  vidIndex={vidIndex}
                  title={currentSeries?.title}
                  cover={currentSeries?.cover}
                  intro={meta?.intro}
                  episodeCount={currentSeries?.episodeCount ?? 0}
                  episodes={currentSeries?.episodes ?? []}
                  currentVidKey={currentVidKey}
                  inBinge={inBinge}
                  setBinge={setBinge}
                  setTarget={setTarget}
                  stepEpisode={stepEpisode}
                  exitMini={exitMini}
                  togglePortrait={togglePortrait}
                  togglePortraitFull={togglePortraitFull}
                  enterMini={enterMini}
                  incognitoOn={incognito.on}
                  toggleIncognito={incognito.toggle}
                  pinned={pinned}
                  togglePinned={togglePinned}
                  activeDefinition={activeDefinition}
                  definitions={definitions}
                  onDefinitionChange={handleDefinitionChange}
                  playSrc={playSrc}
                  danmakuOn={danmakuOn}
                  toggleDanmaku={toggleDanmaku}
                  danmakuDisplay={danmakuDisplay}
                  updateDanmakuDisplay={updateDanmakuDisplay}
                  enhanceSettings={enhanceSettings}
                  updateEnhancement={updateEnhancement}
                  enhanceStatus={enhanceStatus}
                  enhanceAvailable={videoLive}
                  downloading={downloading}
                  setDownloading={setDownloading}
                  exitLandscapeWatch={exitLandscapeWatch}
                  onControlsEnter={onControlsEnter}
                  onControlsLeave={onControlsLeave}
                />
                <PlayingRecoveryOverlays
                  compat={compat}
                  compatSrc={compatSrc}
                  hevcPlayable={hevcPlayable}
                  decodeBlocking={decodeBlocking}
                  videoLive={videoLive}
                  retryOnline={retryOnline}
                  clearDecodeBlocked={clearDecodeBlocked}
                  startCompat={startCompat}
                />
                {(switching || !videoLive || videoStalled) && !compat && <Loadbar />}
              </>
            ) : (
              <IdleRecoveryPanel
                coverBackdrop={coverBackdrop}
                compat={compat}
                chromeShown={chromeShown}
                miniScreen={miniScreen}
                vidIndex={vidIndex}
                error={error}
                autoRetry={autoRetry}
                buffering={buffering}
                retryOnline={retryOnline}
              />
            )}
          </div>
          {heldFrame && <FrameHold frame={heldFrame} hidden={!holdVisible} cover={coverFill} />}
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
