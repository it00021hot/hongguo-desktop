import { MiniScreenControls } from './mini-screen-controls';
import { PhonePlayerChrome } from './phone-player-chrome';
import { PhoneLandscapeChrome } from './phone-landscape-chrome';
import { PhoneDanmakuSend } from './phone-danmaku-send';
import { PhoneSeriesSheet } from './phone-series-sheet';
import { DownloadSheet } from './download-sheet';
import { PlayerControls } from './controls/player-controls';
import type { DanmakuDisplaySettings, EnhancementSettings } from '@/utils/playback-prefs';
import type { EnhancementStatus } from '../hooks/use-enhancement';
import type { Episode, VideoDefinition } from '@/service/schema';

/**
 * 舞台控制栏接线：小屏 / 竖屏 / 窄屏（竖屏信息流 + 横屏播映壳）/ 宽屏全量栏，
 * 共用同一个 `<video>`，切换形态播放零中断。
 */
export function PlayerStageChrome({
  videoRef,
  miniScreen,
  portrait,
  portraitFull,
  narrow,
  landscapeWatch,
  chromeShown,
  phoneDanmakuComposerOpen,
  phoneMoreOpen,
  setPhoneMoreOpen,
  phoneSeriesOpen,
  setPhoneSeriesOpen,
  phoneEnhanceOpen,
  setPhoneEnhanceOpen,
  setPhoneDanmakuSendOpen,
  seriesId,
  vidIndex,
  title,
  cover,
  intro,
  episodeCount,
  episodes,
  currentVidKey,
  inBinge,
  setBinge,
  setTarget,
  stepEpisode,
  exitMini,
  togglePortrait,
  togglePortraitFull,
  enterMini,
  incognitoOn,
  toggleIncognito,
  pinned,
  togglePinned,
  activeDefinition,
  definitions,
  onDefinitionChange,
  playSrc,
  danmakuOn,
  toggleDanmaku,
  danmakuDisplay,
  updateDanmakuDisplay,
  enhanceSettings,
  updateEnhancement,
  enhanceStatus,
  enhanceAvailable,
  downloading,
  setDownloading,
  exitLandscapeWatch,
  onControlsEnter,
  onControlsLeave,
}: {
  videoRef: React.RefObject<HTMLVideoElement | null>;
  miniScreen: boolean;
  portrait: boolean;
  portraitFull: boolean;
  narrow: boolean;
  landscapeWatch: boolean;
  chromeShown: boolean;
  phoneDanmakuComposerOpen: boolean;
  phoneMoreOpen: boolean;
  setPhoneMoreOpen: (open: boolean) => void;
  phoneSeriesOpen: boolean;
  setPhoneSeriesOpen: (open: boolean) => void;
  phoneEnhanceOpen: boolean;
  setPhoneEnhanceOpen: (open: boolean) => void;
  setPhoneDanmakuSendOpen: (open: boolean) => void;
  seriesId: string;
  vidIndex: number;
  title?: string;
  cover?: string;
  intro?: string;
  episodeCount: number;
  episodes: Episode[];
  currentVidKey: string;
  inBinge: boolean;
  setBinge: (id: string | null) => void;
  setTarget: (seriesId: string, vidIndex: number) => void;
  stepEpisode: (delta: number) => void;
  exitMini: () => void;
  togglePortrait: () => void;
  togglePortraitFull: () => void;
  enterMini: () => void;
  incognitoOn: boolean;
  toggleIncognito: () => void;
  pinned: boolean;
  togglePinned: () => void;
  activeDefinition: number;
  definitions: VideoDefinition[];
  onDefinitionChange: (d: number | undefined) => void;
  playSrc: string | null;
  danmakuOn: boolean;
  toggleDanmaku: () => void;
  danmakuDisplay: DanmakuDisplaySettings;
  updateDanmakuDisplay: (patch: Partial<DanmakuDisplaySettings>) => void;
  enhanceSettings: EnhancementSettings;
  updateEnhancement: (patch: Partial<EnhancementSettings>) => void;
  enhanceStatus: EnhancementStatus;
  enhanceAvailable: boolean;
  downloading: boolean;
  setDownloading: (open: boolean) => void;
  exitLandscapeWatch: () => void;
  onControlsEnter: () => void;
  onControlsLeave: () => void;
}) {
  const total = episodes.length;
  const hasNext = episodes.some((e) => e.vidIndex === vidIndex + 1);

  if (miniScreen) {
    return (
      <MiniScreenControls
        videoRef={videoRef}
        title={title}
        intro={intro}
        vidIndex={vidIndex}
        total={total}
        hasNext={hasNext}
        onStepEpisode={(d) => stepEpisode(d)}
        onExpand={exitMini}
        incognitoOn={incognitoOn}
        onToggleIncognito={toggleIncognito}
        pinned={pinned}
        onTogglePinned={togglePinned}
        visible={chromeShown}
        onControlsEnter={onControlsEnter}
        onControlsLeave={onControlsLeave}
      />
    );
  }

  if (portrait) {
    return (
      <MiniScreenControls
        variant="portrait"
        videoRef={videoRef}
        total={total}
        hasNext={hasNext}
        onStepEpisode={(d) => stepEpisode(d)}
        onExpand={togglePortrait}
        incognitoOn={incognitoOn}
        onToggleIncognito={toggleIncognito}
        portraitFull={portraitFull}
        onTogglePortraitFull={togglePortraitFull}
        visible={chromeShown}
        onControlsEnter={onControlsEnter}
        onControlsLeave={onControlsLeave}
      />
    );
  }

  if (narrow) {
    return (
      <>
        {landscapeWatch ? (
          <PhoneLandscapeChrome
            videoRef={videoRef}
            visible={chromeShown}
            title={title}
            vidIndex={vidIndex}
            episodeCount={episodeCount}
            definition={activeDefinition}
            definitions={definitions}
            onDefinitionChange={onDefinitionChange}
            danmakuOn={danmakuOn}
            onToggleDanmaku={toggleDanmaku}
            onOpenMore={() => setPhoneMoreOpen(true)}
            onOpenSeries={() => {
              setPhoneSeriesOpen(true);
              if (!inBinge) setBinge(seriesId);
            }}
            onExit={exitLandscapeWatch}
            vid={currentVidKey}
            onControlsEnter={onControlsEnter}
            onControlsLeave={onControlsLeave}
          />
        ) : (
          <PhonePlayerChrome
            videoRef={videoRef}
            visible={chromeShown && !phoneDanmakuComposerOpen}
            moreOpen={phoneMoreOpen}
            onMoreOpenChange={setPhoneMoreOpen}
            enhanceOpen={phoneEnhanceOpen}
            onEnhanceOpenChange={setPhoneEnhanceOpen}
            onOpenSeries={() => {
              setPhoneSeriesOpen(true);
              if (!inBinge) setBinge(seriesId);
            }}
            episodeCount={episodeCount}
            definition={activeDefinition}
            definitions={definitions}
            onDefinitionChange={onDefinitionChange}
            danmakuOn={danmakuOn}
            onToggleDanmaku={toggleDanmaku}
            danmakuDisplay={danmakuDisplay}
            onDanmakuDisplayChange={updateDanmakuDisplay}
            enhanceSettings={enhanceSettings}
            onEnhanceChange={updateEnhancement}
            enhanceStatus={enhanceStatus}
            enhanceAvailable={enhanceAvailable}
            onDownloadingChange={setDownloading}
            onControlsEnter={onControlsEnter}
            onControlsLeave={onControlsLeave}
            onEnterLandscape={() => setPhoneDanmakuSendOpen(false)}
          />
        )}
        {/* 横屏全屏时 Sheet 仍挂在这里：播映壳只负责触发 moreOpen */}
        {landscapeWatch && (
          <PhonePlayerChrome
            sheetsOnly
            videoRef={videoRef}
            visible={false}
            moreOpen={phoneMoreOpen}
            onMoreOpenChange={setPhoneMoreOpen}
            enhanceOpen={phoneEnhanceOpen}
            onEnhanceOpenChange={setPhoneEnhanceOpen}
            onOpenSeries={() => {
              setPhoneSeriesOpen(true);
              if (!inBinge) setBinge(seriesId);
            }}
            episodeCount={episodeCount}
            definition={activeDefinition}
            definitions={definitions}
            onDefinitionChange={onDefinitionChange}
            danmakuOn={danmakuOn}
            onToggleDanmaku={toggleDanmaku}
            danmakuDisplay={danmakuDisplay}
            onDanmakuDisplayChange={updateDanmakuDisplay}
            enhanceSettings={enhanceSettings}
            onEnhanceChange={updateEnhancement}
            enhanceStatus={enhanceStatus}
            enhanceAvailable={enhanceAvailable}
            onDownloadingChange={setDownloading}
            onControlsEnter={() => undefined}
            onControlsLeave={() => undefined}
          />
        )}
        {!landscapeWatch && (
          <PhoneDanmakuSend
            open={phoneDanmakuComposerOpen}
            onOpenChange={setPhoneDanmakuSendOpen}
            videoRef={videoRef}
            vid={currentVidKey}
          />
        )}
        <PhoneSeriesSheet
          open={phoneSeriesOpen}
          onOpenChange={setPhoneSeriesOpen}
          seriesId={seriesId}
          title={title}
          cover={cover}
          intro={intro}
          episodeCount={episodeCount}
          currentIndex={vidIndex}
          onPickEpisode={(idx) => {
            setTarget(seriesId, idx);
          }}
        />
        <DownloadSheet
          seriesId={seriesId}
          episodes={episodes}
          defaultSelected={vidIndex ? [vidIndex] : []}
          open={downloading}
          onOpenChange={setDownloading}
        />
      </>
    );
  }

  return (
    <PlayerControls
      videoRef={videoRef}
      onControlsEnter={onControlsEnter}
      onControlsLeave={onControlsLeave}
      seriesId={seriesId}
      episodes={episodes}
      currentIndex={vidIndex}
      downloading={downloading}
      onDownloadingChange={setDownloading}
      onStepEpisode={stepEpisode}
      onOpenMini={enterMini}
      onEnterPortrait={togglePortrait}
      incognito={incognitoOn}
      onToggleIncognito={toggleIncognito}
      definition={activeDefinition}
      definitions={definitions}
      onDefinitionChange={onDefinitionChange}
      src={playSrc}
      danmakuOn={danmakuOn}
      onToggleDanmaku={toggleDanmaku}
      danmakuDisplay={danmakuDisplay}
      onDanmakuDisplayChange={updateDanmakuDisplay}
      enhanceSettings={enhanceSettings}
      onEnhanceChange={updateEnhancement}
      enhanceStatus={enhanceStatus}
      enhanceAvailable={enhanceAvailable}
      immersive
      visible={chromeShown}
      onPickEpisode={(idx) => {
        setTarget(seriesId, idx);
      }}
    />
  );
}
