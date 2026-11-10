import { describe, expect, it } from 'vitest';
import { switchSilenceAction } from './use-switch-silence';

describe('switchSilenceAction', () => {
  it('切换在途且旧流在播：暂停', () => {
    expect(
      switchSilenceAction({ switching: true, playing: true, owed: false, sameElement: false }),
    ).toBe('pause');
  });

  it('切换在途但旧流本来就停着（用户自己按停的）：不动，也不欠', () => {
    expect(
      switchSilenceAction({ switching: true, playing: false, owed: false, sameElement: false }),
    ).toBe('none');
  });

  it('切回原集、元素没换：把欠的那次播放还上', () => {
    expect(
      switchSilenceAction({ switching: false, playing: false, owed: true, sameElement: true }),
    ).toBe('resume');
  });

  it('新流到达换了元素：欠的那次作废，交给新元素自动起播', () => {
    expect(
      switchSilenceAction({ switching: false, playing: true, owed: true, sameElement: false }),
    ).toBe('forget');
  });

  it('没有切换也没欠：什么都不做（清晰度切换、正常播放不受影响）', () => {
    expect(
      switchSilenceAction({ switching: false, playing: true, owed: false, sameElement: false }),
    ).toBe('none');
    expect(
      switchSilenceAction({ switching: false, playing: false, owed: false, sameElement: false }),
    ).toBe('none');
  });
});
