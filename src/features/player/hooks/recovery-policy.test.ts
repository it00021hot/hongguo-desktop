import { describe, expect, it } from 'vitest';

import type { MediaErrorKind } from './media-error';
import { emptyFailureCounts, recoveryAction } from './recovery-policy';

describe('recoveryAction', () => {
  it('各类错误首次一律先重试——瞬时故障（含瞬时 decode）的自愈入口', () => {
    const kinds: MediaErrorKind[] = ['decode', 'network', 'other'];
    for (const kind of kinds) {
      expect(recoveryAction(kind, emptyFailureCounts())).toBe('retry');
    }
  });

  it('复发一律落错误页——播放不转码，没有换路兜底', () => {
    const counts = { ...emptyFailureCounts(), decode: 1, network: 1, other: 1 };
    expect(recoveryAction('decode', counts)).toBe('errorPage');
    expect(recoveryAction('network', counts)).toBe('errorPage');
    expect(recoveryAction('other', counts)).toBe('errorPage');
  });

  it('各类预算独立——network 用掉重试后 decode 仍有自己的', () => {
    const counts = { ...emptyFailureCounts(), network: 1 };
    expect(recoveryAction('network', counts)).toBe('errorPage');
    expect(recoveryAction('decode', counts)).toBe('retry');
  });
});
