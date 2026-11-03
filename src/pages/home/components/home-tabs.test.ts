import { describe, expect, it, vi } from 'vitest';

vi.stubGlobal('localStorage', { getItem: () => null, setItem: () => {} });
vi.stubGlobal('navigator', { language: 'zh-CN' });
vi.stubGlobal('document', { documentElement: { lang: '' } });

const { buildTabList, RECOMMEND_TAB_TYPE } = await import('./home-tabs');

function serverTabs() {
  return [
    { tabType: '45', title: '关注', bookstoreId: null },
    { tabType: '36', title: '漫剧', bookstoreId: null },
    { tabType: '39', title: '真人剧', bookstoreId: null },
    { tabType: '16', title: '推荐', bookstoreId: '7294257258253254693' },
  ];
}

describe('buildTabList', () => {
  it('服务端列表保序直用，标题用服务端原文', () => {
    const tabs = buildTabList(serverTabs());
    expect(tabs.map((t) => t.id)).toEqual(['36', '39', '16']);
    expect(tabs[0]?.label).toBe('漫剧');
  });

  it('关注 tab 一律不显示（追更由收藏页承载）', () => {
    expect(buildTabList(serverTabs()).map((t) => t.id)).toEqual(['36', '39', '16']);
  });

  it('服务端标题为空时回落 i18n 名（已知 id）', () => {
    const tabs = buildTabList([{ tabType: '16', title: '', bookstoreId: 'x' }]);
    expect(tabs).toEqual([{ id: '16', label: '推荐' }]);
  });

  it('拉取前/失败/空列表回落本地三件套（i18n 标题）', () => {
    for (const empty of [undefined, []]) {
      expect(buildTabList(empty).map((t) => t.id)).toEqual(['16', '36', '39']);
    }
    expect(buildTabList(undefined)[0]?.label).toBe('推荐');
  });

  it('推荐 tab 常量即回落表首项（种子语义锚点）', () => {
    expect(RECOMMEND_TAB_TYPE).toBe('16');
  });
});
