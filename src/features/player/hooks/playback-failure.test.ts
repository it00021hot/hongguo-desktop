import { describe, expect, it } from 'vitest';
import { AppInvokeError } from '@/service/tauri/invoke';
import {
  playbackFailure,
  rejectedPlaybackFailure,
  playbackRetryDelay,
  retryAfterRegistration,
  playbackFailureForEpisode,
} from './playback-failure';

describe('playback recovery', () => {
  it('does not carry an old episode failure or its retry policy into a new target', () => {
    const failure = playbackFailure('error.network', 'old stream failed');
    const state = { key: 'series:1', failure };
    expect(playbackFailureForEpisode(state, 'series:2')).toBeNull();
    expect(playbackFailureForEpisode(state, 'series:1')).toBe(failure);
    expect(playbackFailureForEpisode(null, 'series:1')).toBeNull();
  });
  it.each([
    'error.auth',
    'error.notFound',
    'error.playbackMaterials',
    'error.codecUnsupported',
    'error.invalidArgs',
    'error.streamMiss',
  ])('does not retry a deterministic %s failure in either error channel', (kind) => {
    const response = playbackFailure(kind, 'server detail');
    const rejected = rejectedPlaybackFailure(new AppInvokeError(kind, 'server detail'));
    expect(rejected).toEqual(response);
    expect(playbackRetryDelay(response, 0)).toBeNull();
    expect(playbackRetryDelay(rejected, 0)).toBeNull();
  });

  it('preserves the confirmed authentication code without suggesting a download', () => {
    const failure = playbackFailure('error.auth', '101108: USER_NOT_LOGIN');
    expect(failure.message).toContain('101108');
    expect(failure.message).toContain('USER_NOT_LOGIN');
    expect(failure.kind).toBe('error.auth');
  });

  it.each(['error.network', 'error.emptyResponse'])('limits transient %s retries', (kind) => {
    const failure = playbackFailure(kind, 'temporary');
    const delays = [0, 1, 2, 3, 4].map((attempt) => playbackRetryDelay(failure, attempt));
    expect(delays.slice(0, 4).every((delay) => delay != null && delay > 0)).toBe(true);
    expect(delays[4]).toBeNull();
    expect(playbackRetryDelay(failure, 100)).toBeNull();
  });

  it('only retries identity-sensitive failures after registration changes', () => {
    expect(retryAfterRegistration(playbackFailure('error.auth', 'rejected'))).toBe(true);
    expect(retryAfterRegistration(playbackFailure('error.network', 'offline'))).toBe(true);
    for (const kind of ['error.notFound', 'error.codecUnsupported', 'error.playbackMaterials']) {
      expect(retryAfterRegistration(playbackFailure(kind, 'unchanged'))).toBe(false);
    }
  });

  it('does not invent a retry policy for an unclassified exception', () => {
    const failure = rejectedPlaybackFailure(new Error('unknown failure'));
    expect(failure.message).toBe('unknown failure');
    expect(playbackRetryDelay(failure, 0)).toBeNull();
  });
});
