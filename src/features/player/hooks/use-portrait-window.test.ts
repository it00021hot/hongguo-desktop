import { describe, expect, it, vi } from 'vitest';

vi.mock('sonner', () => ({ toast: { error: () => undefined } }));
vi.stubGlobal('localStorage', { getItem: () => null, setItem: () => {} });
vi.stubGlobal('navigator', { language: 'zh-CN' });
vi.stubGlobal('document', { documentElement: { lang: '' } });

function fakeVideo(w: number, h: number): HTMLVideoElement {
  return { videoWidth: w, videoHeight: h } as HTMLVideoElement;
}

const PHONE = 9 / 19.5;

describe('portraitAspectOf', () => {
  it('9:16 短剧内容取手机比例地板（矮胖条→手机形竖条）', async () => {
    const { portraitAspectOf } = await import('./use-portrait-window');
    expect(portraitAspectOf(fakeVideo(1080, 1920))).toBeCloseTo(PHONE);
  });

  it('比手机更长的内容按自身比例包住（9:21 不上下留边）', async () => {
    const { portraitAspectOf } = await import('./use-portrait-window');
    expect(portraitAspectOf(fakeVideo(900, 2100))).toBeCloseTo(900 / 2100);
  });

  it('横屏内容回落手机比例（竖屏模式对横屏仍有意义：手机形窄条）', async () => {
    const { portraitAspectOf } = await import('./use-portrait-window');
    expect(portraitAspectOf(fakeVideo(1920, 1080))).toBeCloseTo(PHONE);
  });

  it('近方形（比例 ≥0.9）不算竖屏内容，回落手机比例', async () => {
    const { portraitAspectOf } = await import('./use-portrait-window');
    expect(portraitAspectOf(fakeVideo(1000, 1100))).toBeCloseTo(PHONE);
  });

  it('无元数据（0×0）与无元素都回落手机比例', async () => {
    const { portraitAspectOf } = await import('./use-portrait-window');
    expect(portraitAspectOf(fakeVideo(0, 0))).toBeCloseTo(PHONE);
    expect(portraitAspectOf(null)).toBeCloseTo(PHONE);
  });
});
