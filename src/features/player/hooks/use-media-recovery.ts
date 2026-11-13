import { useRef } from 'react';
import { STREAM_SCHEME } from '@/utils/scheme';
import { t } from '@/locales';
import { classifyMediaError } from './media-error';
import type { MediaErrorKind } from './media-error';
import { emptyFailureCounts, recoveryAction } from './recovery-policy';
import type { FailureCounts } from './recovery-policy';

/**
 * 音轨断供按失败账本重新取流；内置视频解码失败由 `handleDirectError` 处理。
 */
export function useMediaRecovery(params: {
  videoRef: React.RefObject<HTMLVideoElement | null>;
  episodeKey: string;
  playSrc: string | null;
  error: string | null;
  setError: (msg: string | null) => void;
  setSrc: (src: string | null) => void;
  retryOnline: () => void;
}) {
  const { videoRef, episodeKey, playSrc, error, setError, setSrc, retryOnline } = params;

  // 失败账本：本集各类错误已发生几次（恢复动作升级用）
  const failuresRef = useRef<{ key: string; counts: FailureCounts } | null>(null);
  const failuresOf = () =>
    failuresRef.current?.key === episodeKey ? failuresRef.current.counts : emptyFailureCounts();
  const recordFailure = (kind: MediaErrorKind) => {
    const counts = failuresOf();
    failuresRef.current = { key: episodeKey, counts: { ...counts, [kind]: counts[kind] + 1 } };
  };

  // 故意不用 useCallback：账本 / episodeKey 每渲新鲜，包起来反而会钉死旧集
  const handleVideoError = () => {
    if (error) return;
    const me = videoRef.current?.error;
    const detail = me ? `（MediaError ${me.code}${me.message ? `: ${me.message}` : ''}）` : '';
    const online = playSrc?.includes(STREAM_SCHEME) ?? false;
    if (online) {
      const kind = classifyMediaError(me?.code);
      const action = recoveryAction(kind, failuresOf());
      recordFailure(kind);
      if (action === 'retry') {
        retryOnline();
        return;
      }
    }
    setError(`${online ? t('player.onlineInterrupted') : t('error.media')}${detail}`);
    if (online) setSrc(null);
  };

  return { handleVideoError };
}
