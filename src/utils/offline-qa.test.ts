import { describe, expect, it } from 'vitest';
import { enhancedCover, isRenderableCover } from './cover';
import { normalizeAvatarUrl } from './image-url';
import { externalPageUrl, offlineQa, pageImageSrc, requestHost } from './offline-qa';

function withFlag(flag: string | undefined, run: () => void) {
  const holder = globalThis as { __HONGGUO_OFFLINE_QA?: string };
  const previous = holder.__HONGGUO_OFFLINE_QA;
  if (flag === undefined) delete holder.__HONGGUO_OFFLINE_QA;
  else holder.__HONGGUO_OFFLINE_QA = flag;
  try {
    run();
  } finally {
    if (previous === undefined) delete holder.__HONGGUO_OFFLINE_QA;
    else holder.__HONGGUO_OFFLINE_QA = previous;
  }
}

describe('offline host', () => {
  it('userinfo 里的 localhost 不是主机', () => {
    expect(requestHost('http://localhost:80@example.com/a')).toBe('example.com');
    expect(requestHost('http://127.0.0.1:80@example.com/a')).toBe('example.com');
    expect(requestHost('http://user:pass@127.0.0.1/a')).toBe('127.0.0.1');
    expect(requestHost('http://[::1]:80/a')).toBe('::1');
  });

  it('只有精确的 1 才离线，外部地址不会交给 img', () => {
    withFlag('0', () => {
      expect(offlineQa()).toBe(false);
      expect(externalPageUrl('https://cdn.example/a.jpeg')).toBe(false);
    });
    withFlag('', () => expect(offlineQa()).toBe(false));
    withFlag('true', () => expect(offlineQa()).toBe(false));
    withFlag('1', () => {
      expect(offlineQa()).toBe(true);
      expect(externalPageUrl('http://localhost:80@example.com/a.jpeg')).toBe(true);
      expect(isRenderableCover('https://cdn.example/a.heic')).toBe(false);
      expect(isRenderableCover('https://cdn.example/a.jpeg')).toBe(false);
      expect(enhancedCover('https://cdn.example/a.heic')).toBeUndefined();
      expect(normalizeAvatarUrl('https://cdn.example/a.jpeg')).toBe('');
      expect(isRenderableCover('http://127.0.0.1/a.jpeg')).toBe(true);
      expect(pageImageSrc('http://localhost:80@example.com/a.jpeg')).toBe('');
      expect(pageImageSrc('blob:http://localhost/preview')).toBe('blob:http://localhost/preview');
    });
  });
});
