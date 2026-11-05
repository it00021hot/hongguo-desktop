import { isAndroid } from '@/utils/platform';
import { CompatOverlay } from './compat-overlay';
import { HevcInstallCard } from './hevc-install-card';
import { PlayerChromePills } from './immersive-info-overlay';
import { LoadingPanel } from './loading-panel';
import type { CompatProgress, OnlineProgress } from '@/service/schema';

/** 有流时：转码浮层 + HEVC 安装引导（仅 Windows）。 */
export function PlayingRecoveryOverlays({
  compat,
  compatSrc,
  hevcPlayable,
  decodeBlocking,
  videoLive,
  retryOnline,
  clearDecodeBlocked,
  startCompat,
}: {
  compat: CompatProgress | null;
  compatSrc: string | null;
  hevcPlayable: boolean | undefined;
  decodeBlocking: boolean;
  videoLive: boolean;
  retryOnline: () => void;
  clearDecodeBlocked: () => void;
  startCompat: () => void;
}) {
  return (
    <>
      {compat && <CompatOverlay compat={compat} />}
      {hevcPlayable === false && !compat && !compatSrc && !isAndroid() && (
        <HevcInstallCard
          hevcPlayable={false}
          variant={decodeBlocking ? 'blocking' : 'floating'}
          onInstalled={() => {
            if (!videoLive) retryOnline();
          }}
          onChooseCompat={() => {
            clearDecodeBlocked();
            startCompat();
          }}
        />
      )}
    </>
  );
}

/** 无流时：封面侧的加载/错误面板 + 常显导航 pill（取流失败时信息叠加与 Esc 都不在）。 */
export function IdleRecoveryPanel({
  coverBackdrop,
  compat,
  chromeShown,
  miniScreen,
  vidIndex,
  error,
  autoRetry,
  buffering,
  retryOnline,
}: {
  coverBackdrop: React.ReactNode;
  compat: CompatProgress | null;
  chromeShown: boolean;
  miniScreen: boolean;
  vidIndex: number;
  error: string | null;
  autoRetry: { attempt: number } | null;
  buffering: OnlineProgress | null;
  retryOnline: () => void;
}) {
  return (
    <>
      {coverBackdrop}
      {compat && <CompatOverlay compat={compat} />}
      {!compat && (
        <>
          <PlayerChromePills
            chromeShown={chromeShown}
            miniScreen={miniScreen}
            always
            vidIndex={vidIndex}
          />
          <LoadingPanel
            error={error}
            autoRetry={autoRetry}
            buffering={buffering}
            retryOnline={retryOnline}
          />
        </>
      )}
    </>
  );
}
