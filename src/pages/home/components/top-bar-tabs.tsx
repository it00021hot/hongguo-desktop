/** 首页内容区分类 tab 栏：portal 进 AppShell 顶栏中部（portal 进 components/layout/top-bar-tabs 的共享插槽）。
 *  tab 列表由服务端下发（bookmall/tab 响应 tab_item[]：tab_type/标题/顺序），
 *  本地三件套仅作拉取前/失败的回落。
 *  列表纯逻辑在 home-tabs.ts（无组件依赖，node 环境可测）。 */
import { TopBarTab, TopBarTabsPortal } from '@/components/layout/top-bar-tabs';

import type { HomeTab } from './home-tabs';

/**
 * 分类 tab 栏（顶栏中部插槽，共享件见 top-bar-tabs.tsx）。
 */
export function TopBarTabs({
  tabs,
  source,
  onPick,
}: {
  tabs: HomeTab[];
  source: string;
  onPick: (id: string) => void;
}) {
  return (
    <TopBarTabsPortal>
      {tabs.map((tab) => (
        <TopBarTab key={tab.id} active={source === tab.id} onClick={() => onPick(tab.id)}>
          {tab.label}
        </TopBarTab>
      ))}
    </TopBarTabsPortal>
  );
}
