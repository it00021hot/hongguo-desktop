import { useCallback, useEffect, useRef, useState } from 'react';
import { isAndroid } from '@/utils/platform';
import { STREAM_SCHEME } from '@/utils/scheme';
import { t } from '@/locales';
import { classifyMediaError } from './media-error';
import type { MediaErrorKind } from './media-error';
import { emptyFailureCounts, recoveryAction } from './recovery-policy';
import type { FailureCounts } from './recovery-policy';
import { hevcSession } from './hevc-session';

/**
 * 解码失败代理：必须在 useTranscodeFallback 之前创建（hook 要恒定 onDecodeFailure），
 * 真正裁决在 useMediaRecovery 里经 ref 接上。
 */
export function useDecodeFailureProxy() {
  const onDecodeFailureRef = useRef<() => void>(() => {});
  const onDecodeFailure = useCallback(() => onDecodeFailureRef.current(), []);
  return { onDecodeFailure, onDecodeFailureRef };
}

/**
 * 解码失败裁决 + 在线断供重试账本：Windows 无 HEVC 先出主卡；Android / 已授权会话直接转兼容。
 */
export function useMediaRecovery(params: {
  videoRef: React.RefObject<HTMLVideoElement | null>;
  episodeKey: string;
  hevcPlayable: boolean | undefined;
  startCompat: () => void;
  forceCompat: boolean;
  playSrc: string | null;
  compatSrc: string | null;
  compat: unknown;
  error: string | null;
  setError: (msg: string | null) => void;
  setSrc: (src: string | null) => void;
  retryOnline: () => void;
  /** useDecodeFailureProxy 的 ref：每次渲染同步最新裁决 */
  onDecodeFailureRef: React.MutableRefObject<() => void>;
}) {
  const {
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
  } = params;

  /** 解码失败待裁决：带 key 读时过滤而非换集清空（同步 setState 会触发级联渲染）。 */
  const [decodeBlocked, setDecodeBlocked] = useState<{ key: string } | null>(null);
  const decodeBlocking = decodeBlocked?.key === episodeKey;

  const requestDecodeFallback = useCallback(() => {
    if (isAndroid()) {
      hevcSession.authorizeCompat();
      startCompat();
      return;
    }
    if (hevcPlayable === false && !hevcSession.compatAuthorized()) {
      setDecodeBlocked({ key: episodeKey });
      return;
    }
    startCompat();
  }, [hevcPlayable, startCompat, episodeKey]);

  // 每次渲染同步最新裁决到代理（effect 时机足够：轮询首发至少 1.2s 后）
  useEffect(() => {
    onDecodeFailureRef.current = requestDecodeFallback;
  }, [requestDecodeFallback, onDecodeFailureRef]);

  // 失败账本：本集各类错误已发生几次（recovery-action 升级用）
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
    if (online && !compatSrc && !compat) {
      const kind = classifyMediaError(me?.code);
      const action = recoveryAction(kind, failuresOf(), forceCompat || hevcPlayable === false);
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

  const clearDecodeBlocked = useCallback(() => setDecodeBlocked(null), []);

  return {
    decodeBlocking,
    requestDecodeFallback,
    handleVideoError,
    clearDecodeBlocked,
  };
}
