import { t } from '@/locales';
import { isAppInvokeError, localizedMessage } from '@/service/tauri/invoke';

export type PlaybackFailure = { kind: string; message: string };
export type EpisodePlaybackFailure = { key: string; failure: PlaybackFailure };

export function playbackFailureForEpisode(
  state: EpisodePlaybackFailure | null,
  episodeKey: string,
): PlaybackFailure | null {
  return state?.key === episodeKey ? state.failure : null;
}

const RETRY_DELAYS_MS = [2_000, 5_000, 10_000, 15_000];
const TRANSIENT_KINDS = new Set(['error.network', 'error.emptyResponse']);

export function playbackFailure(kind: string, message: string): PlaybackFailure {
  if (kind === 'error.auth') {
    return { kind, message: `${t('player.authRejected')}（${message}）` };
  }
  return {
    kind,
    message: kind === 'error.media' ? t('player.prepareFailed') : localizedMessage(kind, message),
  };
}

export function rejectedPlaybackFailure(error: Error): PlaybackFailure {
  if (!isAppInvokeError(error)) return { kind: 'error.internal', message: error.message };
  const detail = typeof error.cause === 'string' ? error.cause : error.message;
  return playbackFailure(error.kind, detail);
}

export function playbackRetryDelay(failure: PlaybackFailure, attempt: number): number | null {
  return TRANSIENT_KINDS.has(failure.kind) ? (RETRY_DELAYS_MS[attempt] ?? null) : null;
}

export function retryAfterRegistration(failure: PlaybackFailure): boolean {
  return failure.kind === 'error.auth' || TRANSIENT_KINDS.has(failure.kind);
}
