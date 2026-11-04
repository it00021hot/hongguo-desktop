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

  it('decode 复发才升级兜底——0.4.3 曾把首次 decode 直接送进整集转码', () => {
    expect(recoveryAction('decode', { ...emptyFailureCounts(), decode: 1 })).toBe('fallback');
  });

  it('network/other 复发落错误页——0.4.2 语义不变', () => {
    const counts = { ...emptyFailureCounts(), network: 1, other: 1 };
    expect(recoveryAction('network', counts)).toBe('errorPage');
    expect(recoveryAction('other', counts)).toBe('errorPage');
  });

  it('各类预算独立——network 用掉重试后 decode 仍有自己的', () => {
    const counts = { ...emptyFailureCounts(), network: 1 };
    expect(recoveryAction('network', counts)).toBe('errorPage');
    expect(recoveryAction('decode', counts)).toBe('retry');
  });

  it('已知不可解时 decode 径直兜底——不再烧一次必败的原生重试', () => {
    expect(recoveryAction('decode', emptyFailureCounts(), true)).toBe('fallback');
    expect(recoveryAction('network', emptyFailureCounts(), true)).toBe('retry');
  });
});
