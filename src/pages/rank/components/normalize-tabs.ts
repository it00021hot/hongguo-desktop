/** 榜单 tab 归一：把服务端两种 selector 形态折算成统一的页面内结构。 */
import { t } from '@/locales';
import type { RankSubList, RankTab } from '@/service/schema';

/** 服务端两种 selector 形态归一成统一的「内容tab → 子榜(含筛选面板)」。 */
export function normalizeTabs(tabs: RankTab[]): RankTab[] {
  if (tabs.length === 0) return [];
  // 两级形态的标志是「全部」tab（id=all）；其余 tab id 都是内容分类
  if (tabs.some((tab) => tab.id === 'all')) {
    return tabs.filter((tab) => tab.id !== 'ranklist_celebrity' && tab.subs.length > 0);
  }
  // 一级形态（老版本身份，理论不再出现）：榜单当子榜，平铺选项包成单行面板
  return [
    {
      id: 'all',
      name: '全部',
      subs: tabs.map<RankSubList>((tab) => ({
        id: tab.id,
        name: tab.name,
        description: '',
        panel:
          tab.subs.length > 0
            ? [
                {
                  name: t('rank.filter.title'),
                  items: tab.subs.map((s) => ({ id: s.id, name: s.name })),
                },
              ]
            : [],
      })),
    },
  ];
}
