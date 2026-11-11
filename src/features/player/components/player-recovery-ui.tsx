import { PlayerChromePills } from './immersive-info-overlay';
import { LoadingPanel } from './loading-panel';
import type { OnlineProgress } from '@/service/schema';

/** 无流时：封面侧的加载/错误面板 + 常显导航 pill（取流失败时信息叠加与 Esc 都不在）。 */
export function IdleRecoveryPanel({
  coverBackdrop,
  chromeShown,
  miniScreen,
  vidIndex,
  error,
  autoRetry,
  buffering,
  retryOnline,
}: {
  coverBackdrop: React.ReactNode;
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
  );
}
