import { useNavigate } from '@tanstack/react-router';
import { Button } from '@/components/ui/button';
import { TopBarTab, TopBarTabsPortal } from '@/components/layout/top-bar-tabs';
import { t } from '@/locales';
import { usePlaySeries } from '@/hooks/use-play-series';
import { useSessionState } from '@/hooks/use-scroll-restore';
import { NewCalendarView } from './new-calendar-view';
import { NewDramaRecommends } from './new-drama-recommends';
import type { RankItem } from '@/service/schema';

/**
 * 新剧页：新剧推荐 + 上新日历两个视图。
 *
 * 推荐 = `firstonlinetime_new` 按上架时间倒序的翻页流；
 * 日历 = 官方按日期排布的上新表（前后各一周，含未上线条目）。
 */

const GENDERS: { value: number; labelKey: string }[] = [
  { value: 2, labelKey: 'newDrama.gender.all' },
  { value: 1, labelKey: 'newDrama.gender.male' },
  { value: 0, labelKey: 'newDrama.gender.female' },
];

export function NewDramaPage() {
  // 频道筛选在页面层：官方把它放在标题行右侧，对推荐/日历两个视图都可见。
  // 会话级保留（hgplayer 1.1.7 同款：去播放再回来频道与位置都在）
  const [gender, setGender] = useSessionState('hongguo.new.gender', 2);
  // 视图 tab（推荐/日历）：状态自持——tab 胶囊 portal 进 AppShell 顶栏，
  // 不能再依赖 Radix Tabs 的组件树上下文（Trigger 必须长在 Tabs 里）
  const [view, setView] = useSessionState<'recommend' | 'calendar'>(
    'hongguo.new.view',
    'recommend',
  );

  const playSeries = usePlaySeries();
  const navigate = useNavigate();
  // 点击分流（对齐排行榜预约榜口径）：未上线剧进详情——详情解析分集必然
  // 失败，带上行内档案快照撑起「即将上线」降级视图 + 预约；已上线直接播
  const goDetailUpcoming = (item: {
    seriesId: string;
    title: string;
    cover: string;
    tags?: string[];
    description: string;
  }) =>
    void navigate({
      to: '/detail',
      search: {
        seriesId: item.seriesId,
        title: item.title,
        cover: item.cover,
        tags: (item.tags ?? []).join(','),
        desc: item.description,
      },
    });
  const handleSelect = (item: RankItem) => {
    if (item.upcoming) goDetailUpcoming(item);
    else playSeries(item.seriesId);
  };

  return (
    // 视图 tab 已上移 AppShell 顶栏（TopBarTabsPortal，见下）。
    // h-full 锁在视口内（同排行榜/首页）：页面自身不滚，频道胶囊行常驻，
    // 只有视图内容（推荐网格/日历列表）在自己的滚动区里滚
    <div className="flex h-full min-h-0 flex-col">
      <TopBarTabsPortal>
        <TopBarTab active={view === 'recommend'} onClick={() => setView('recommend')}>
          {t('newDrama.tabs.recommend')}
        </TopBarTab>
        <TopBarTab active={view === 'calendar'} onClick={() => setView('calendar')}>
          {t('newDrama.tabs.calendar')}
        </TopBarTab>
      </TopBarTabsPortal>

      <div className="flex min-h-0 flex-1 flex-col gap-3">
        {/* 顶行只留频道胶囊；页标题由侧栏高亮表达，不重复 */}
        <div className="mt-3 flex flex-wrap items-center justify-end gap-2">
          {GENDERS.map(({ value, labelKey }) => (
            <Button
              key={value}
              size="sm"
              variant={gender === value ? 'default' : 'outline'}
              className="rounded-full px-4"
              onClick={() => setGender(value)}
            >
              {t(labelKey)}
            </Button>
          ))}
        </div>

        {view === 'recommend' ? (
          // key=gender：换频道重挂载，滚动归零（同排行榜切子榜的口径）
          <NewDramaRecommends key={gender} gender={gender} onSelect={handleSelect} />
        ) : (
          <NewCalendarView />
        )}
      </div>
    </div>
  );
}
