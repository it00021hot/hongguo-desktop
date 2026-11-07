import { describe, expect, it } from 'vitest';

import { decideCompatPlayback, resolvePlayBaseSrc } from './compat-policy';

describe('decideCompatPlayback', () => {
  it('Android + 不可播：主动兼容并挡住原生流', () => {
    expect(
      decideCompatPlayback({ hevcPlayable: false, android: true, compatAuthorized: false }),
    ).toEqual({ forceCompat: true, holdNative: true });
  });

  it('Android 能力未回：先挡住原生流，不主动转（等结论）', () => {
    expect(
      decideCompatPlayback({ hevcPlayable: undefined, android: true, compatAuthorized: false }),
    ).toEqual({ forceCompat: false, holdNative: true });
  });

  it('Android 可播：原生直通', () => {
    expect(
      decideCompatPlayback({ hevcPlayable: true, android: true, compatAuthorized: false }),
    ).toEqual({ forceCompat: false, holdNative: false });
  });

  it('桌面不可播且未授权：仍先走引导卡，不主动转、不挡原生', () => {
    expect(
      decideCompatPlayback({ hevcPlayable: false, android: false, compatAuthorized: false }),
    ).toEqual({ forceCompat: false, holdNative: false });
  });

  it('桌面不可播且已授权：连播径直主动兼容并挡原生', () => {
    expect(
      decideCompatPlayback({ hevcPlayable: false, android: false, compatAuthorized: true }),
    ).toEqual({ forceCompat: true, holdNative: true });
  });

  it('桌面可播：原生直通（授权记号无影响）', () => {
    expect(
      decideCompatPlayback({ hevcPlayable: true, android: false, compatAuthorized: true }),
    ).toEqual({ forceCompat: false, holdNative: false });
  });

  it('forces compat when backend marks needsCompat (bytevc2)', () => {
    expect(
      decideCompatPlayback({
        hevcPlayable: true,
        android: false,
        compatAuthorized: false,
        needsCompat: true,
      }),
    ).toEqual({ forceCompat: true, holdNative: true });
  });
});

describe('resolvePlayBaseSrc', () => {
  it('兼容产物优先', () => {
    expect(resolvePlayBaseSrc('native', 'compat', true)).toBe('compat');
    expect(resolvePlayBaseSrc('native', 'compat', false)).toBe('compat');
  });

  it('holdNative 且无产物时不喂原生', () => {
    expect(resolvePlayBaseSrc('native', null, true)).toBeNull();
  });

  it('不 hold 时回落原生', () => {
    expect(resolvePlayBaseSrc('native', null, false)).toBe('native');
    expect(resolvePlayBaseSrc(null, null, false)).toBeNull();
  });
});
