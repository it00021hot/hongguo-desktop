/** 首页内容区分类 tab 栏：portal 进 AppShell 顶栏中部（portal 进 components/layout/top-bar-tabs 的共享插槽）。 */
import { TopBarTab, TopBarTabsPortal } from '@/components/layout/top-bar-tabs';
import { t } from '@/locales';

/** 顶部 tab 的三个源（书城 cell 换一换：推荐 16 / 漫剧 36 / 真人剧 39）。 */
export type StreamSource = 'feed' | 'comic' | 'human';

export const TABS: { id: StreamSource; labelKey: string }[] = [
  { id: 'feed', labelKey: 'home.tabFeed' },
  { id: 'comic', labelKey: 'home.tabComic' },
  { id: 'human', labelKey: 'home.tabHuman' },
];

/**
 * 分类 tab 栏（顶栏中部插槽，共享件见 top-bar-tabs.tsx）。
 */
export function TopBarTabs({
  source,
  onPick,
}: {
  source: StreamSource;
  onPick: (id: StreamSource) => void;
}) {
  return (
    <TopBarTabsPortal>
      {TABS.map((tab) => (
        <TopBarTab key={tab.id} active={source === tab.id} onClick={() => onPick(tab.id)}>
          {t(tab.labelKey)}
        </TopBarTab>
      ))}
    </TopBarTabsPortal>
  );
}
