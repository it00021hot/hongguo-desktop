import { describe, expect, it, vi } from 'vitest';
import { dedupBy, fuzzyStaleTime } from './common';

vi.stubGlobal('localStorage', { getItem: () => null, setItem: () => {} });
vi.stubGlobal('navigator', { language: 'zh-CN' });
vi.stubGlobal('document', { documentElement: { lang: '' } });

function page(ids: string[]): import('../schema').FeedPage {
  return {
    items: ids.map((seriesId) => ({
      seriesId,
      title: `剧 ${seriesId}`,
      cover: '',
      horizCover: '',
      vid: `v${seriesId}`,
      episodeCnt: 1,
      playCnt: 0,
      commentCount: 0,
      score: 0,
      tags: [],
      seasonTag: '',
      heatText: '',
      badge: '',
      contentType: 0,
    })),
    hasMore: true,
    nextOffset: 0,
    sessionId: 's',
  };
}

describe('feedItems', () => {
  it('跨页去重：推荐位轮换的重复条目只留首个', async () => {
    const { feedItems } = await import('./feed');
    const pages = [page(['1', '2', '3']), page(['2', '3', '4']), page(['4', '5'])];
    expect(feedItems(pages).map((i) => i.seriesId)).toEqual(['1', '2', '3', '4', '5']);
  });

  it('空页列表返回空数组', async () => {
    const { feedItems } = await import('./feed');
    expect(feedItems([])).toEqual([]);
  });
});

describe('dedupBy', () => {
  it('按 key 去重并保序（首个出现位置为准）', () => {
    const items = [
      { id: 'a', v: 1 },
      { id: 'b', v: 2 },
      { id: 'a', v: 3 },
    ];
    expect(dedupBy(items, (i) => i.id)).toEqual([
      { id: 'a', v: 1 },
      { id: 'b', v: 2 },
    ]);
  });

  it('空输入返回空数组', () => {
    expect(dedupBy([], (i) => i)).toEqual([]);
  });
});

describe('fuzzyStaleTime', () => {
  it('值域在 [10s, 30s) 内', () => {
    for (let i = 0; i < 200; i++) {
      const v = fuzzyStaleTime(['probe', String(i)]);
      expect(v).toBeGreaterThanOrEqual(10_000);
      expect(v).toBeLessThan(30_000);
    }
  });

  it('同一 key 跨调用稳定（重渲染不漂移）', () => {
    const key = ['series-episodes', '7687919221593885758'];
    expect(fuzzyStaleTime(key)).toBe(fuzzyStaleTime(key));
    expect(fuzzyStaleTime(key)).toBe(fuzzyStaleTime([...key]));
  });

  it('不同 key 撒开（避免集体过期）', () => {
    const values = new Set(Array.from({ length: 64 }, (_, i) => fuzzyStaleTime(['k', String(i)])));
    expect(values.size).toBeGreaterThan(1);
  });
});
