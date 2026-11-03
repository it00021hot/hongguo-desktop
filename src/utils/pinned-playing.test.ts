import { describe, expect, it } from 'vitest';

import { withPinnedPlaying } from './pinned-playing';

interface Item {
  seriesId: string;
  title: string;
}

const item = (seriesId: string, title = ''): Item => ({ seriesId, title });

const placeholderFor = (pin: Item) => () => pin;

describe('withPinnedPlaying', () => {
  it('流里含在播剧时也钉到最前(旧逻辑此处不钉,正是误切根源)', () => {
    const playing = item('p', '在播剧');
    const out = withPinnedPlaying(
      [item('a'), playing, item('b')],
      true,
      'p',
      placeholderFor(playing),
    );
    expect(out.map((i) => i.seriesId)).toEqual(['p', 'a', 'b']);
  });

  it('闸关着不钉(切 tab 换片语义)', () => {
    const items = [item('a'), item('b')];
    expect(withPinnedPlaying(items, false, 'a', placeholderFor(item('a')))).toBe(items);
  });

  it('没有在播剧不钉', () => {
    const items = [item('a'), item('b')];
    expect(withPinnedPlaying(items, true, null, placeholderFor(item('a')))).toBe(items);
  });

  it('流里没有在播剧时钉占位到最前', () => {
    const playing = item('p', '在播剧');
    const out = withPinnedPlaying([item('a'), item('b')], true, 'p', placeholderFor(playing));
    expect(out.map((i) => i.seriesId)).toEqual(['p', 'a', 'b']);
  });

  it('空列表也能钉占位', () => {
    const playing = item('p');
    expect(withPinnedPlaying([], true, 'p', placeholderFor(playing))).toEqual([playing]);
  });

  it('钉后其余条目相对顺序不变且不重复', () => {
    const playing = item('p');
    const out = withPinnedPlaying(
      [item('a'), item('p'), item('b'), item('c')],
      true,
      'p',
      placeholderFor(playing),
    );
    expect(out.map((i) => i.seriesId)).toEqual(['p', 'a', 'b', 'c']);
  });

  it('占位工厂返回 null(档案缓存缺失)且流里也没有 → 不钉', () => {
    const items = [item('a')];
    expect(withPinnedPlaying(items, true, 'p', () => null)).toBe(items);
  });

  it('流里已有在播剧时不调占位工厂', () => {
    const playing = item('p');
    let called = false;
    withPinnedPlaying([playing], true, 'p', () => {
      called = true;
      return null;
    });
    expect(called).toBe(false);
  });
});
