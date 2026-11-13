import { describe, expect, it } from 'vitest';

import { overlayVisibility } from './use-frame-hold';

describe('overlayVisibility', () => {
  const base = {
    transition: 'episode' as const,
    veiled: false,
    videoLive: true,
    switching: false,
    hasFrame: true,
    hasPlaySrc: true,
    error: false,
  };

  it('同剧切集的取流窗口（veil 未拉时）：旧画面还在播，封面与定格帧都不出', () => {
    const v = overlayVisibility({ ...base, switching: true });
    expect(v.holdVisible).toBe(false);
    expect(v.coverVisible).toBe(false);
  });

  it('黑幕在场（同剧切集间隙）：定格帧与封面全部退场，观感交给暗转', () => {
    const v = overlayVisibility({ ...base, veiled: true, videoLive: false, switching: true });
    expect(v.holdVisible).toBe(false);
    expect(v.coverVisible).toBe(false);
  });

  it('切剧接管：黑幕不在场，旧画面还在播也要立即盖新剧封面', () => {
    const v = overlayVisibility({ ...base, transition: 'series', switching: true });
    expect(v.coverVisible).toBe(true);
    expect(v.holdVisible).toBe(false);
  });

  it('初次挂载（transition 未定、不可能有帧）等同切剧：取流窗口出封面', () => {
    const v = overlayVisibility({
      ...base,
      transition: null,
      hasFrame: false,
      switching: true,
      videoLive: false,
    });
    expect(v.holdVisible).toBe(false);
    expect(v.coverVisible).toBe(true);
  });

  it('抓不到帧（老 WebView/黑屏切换）回落封面，不裸黑屏', () => {
    const v = overlayVisibility({ ...base, hasFrame: false, videoLive: false, switching: true });
    expect(v.holdVisible).toBe(false);
    expect(v.coverVisible).toBe(true);
  });

  it('清晰度切换（episodeKey 不变、veil 不置）：换流间隙定格帧照旧接棒', () => {
    const fetchWindow = overlayVisibility({ ...base, switching: false });
    expect(fetchWindow.coverVisible).toBe(false);
    const swapGap = overlayVisibility({ ...base, videoLive: false });
    expect(swapGap.holdVisible).toBe(true);
    expect(swapGap.coverVisible).toBe(false);
  });

  it('黑幕收口后的播放中：定格帧与封面都不出', () => {
    const v = overlayVisibility({ ...base });
    expect(v.holdVisible).toBe(false);
    expect(v.coverVisible).toBe(false);
  });

  it('出错或没有流时定格帧让位（错误面板与封面照旧接管）', () => {
    for (const patch of [{ error: true }, { hasPlaySrc: false }] as const) {
      const v = overlayVisibility({ ...base, videoLive: false, ...patch });
      expect(v.holdVisible).toBe(false);
      expect(v.coverVisible).toBe(true);
    }
  });
});
