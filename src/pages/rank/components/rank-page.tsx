import { useEffect, useMemo, useRef } from 'react';
import { ErrorState } from '@/components/common/error-state';
import { Empty, EmptyHeader, EmptyTitle } from '@/components/ui/empty';
import { Spinner } from '@/components/ui/spinner';
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group';
import { Skeleton } from '@/components/ui/skeleton';
import { RefreshShade } from '@/components/common/refresh-shade';
import { SkeletonRows } from '@/components/ui/skeletons';
import { TopBarTab, TopBarTabsPortal } from '@/components/layout/top-bar-tabs';
import { useRank } from '@/service/queries';
import { useScrollRestore, useSessionState } from '@/hooks/use-scroll-restore';
import { t } from '@/locales';
import { SENTINEL_ROOT_MARGIN_PX } from '@/utils/infinite-scroll';
import { FilterPanelButton } from './filter-panel-button';
import { RankRow } from './rank-row';
import { normalizeTabs } from './normalize-tabs';

/**
 * 排行榜页：
 * 内容 tab（全部/真人剧/漫剧/AI剧/系列剧）× 左侧子榜竖排 × 右上筛选面板
 * （panel_selected_items 单选），子榜名旁带「N月N日已更新·基于…」描述行。
 *
 * 选项表由接口随行下发（响应 cell_selector）。形态由客户端自报版本号
 * 决定（与登录态无关，设备档案已在后端对齐版本身份），normalizeTabs 保留
 * 一级形态兼容：
 * - 两级结构：tab=内容分类 → sub=子榜（含分组筛选面板行）；
 * - 一级（老版本身份，理论上不再出现）：tab=榜单 → sub=筛选选项平铺。
 *
 * 「演员」tab 响应无剧集数据（celebrity 形态），不提供入口。
 */

/** 首屏 schema 未到时 tab 行/子榜给骨架占位（形态未知，比整块空白好）。 */

export function RankPage() {
  // 选中态会话级保留（去播放再回来榜单还在原位置）
  const [selected, setSelected] = useSessionState('hongguo.rank.selected', 'all');
  // 子榜 id；tab 切换时重置为新 tab 的第一个子榜
  const [sub, setSub] = useSessionState('hongguo.rank.sub', 'ranklist_hot_sc');
  // 筛选面板选中项（'' = 总榜，即无筛选）
  const [panel, setPanel] = useSessionState('hongguo.rank.panel', '');
  // 榜单历史选中项（'' = 当前月；date_YYYYMM = 历史月份榜）
  const [background, setBackground] = useSessionState('hongguo.rank.background', '');
  const rank = useRank(selected, sub, panel, background);
  const { items: listItems, isLoading, error, isFetching, refetch } = rank;

  const filterTitle = t('rank.filter.title');
  const tabs = useMemo(() => normalizeTabs(rank.tabs, filterTitle), [rank.tabs, filterTitle]);
  // 首屏加载中先显示 tab 行骨架；形态确定后仅两级形态显示
  // （登录一级形态只有一个合成 tab，隐藏整行）；出错时不渲染
  const showTabsRow = rank.tabs.length > 0 ? rank.tabs.some((tab) => tab.id === 'all') : isLoading;
  const currentTab = tabs.find((tab) => tab.id === selected);
  const currentSub = currentTab?.subs.find((s) => s.id === sub) ?? currentTab?.subs[0];

  // schema 到达（或登录态形态切换）后，当前 sub 不在新 tab 的子榜里时
  // 落到第一个子榜（render 期调整 state 的官方模式，立即用新值重渲染；
  // subs 非空时 currentSub 必有值，undefined 只可能伴随空 subs）
  if (currentTab && currentSub && currentSub.id !== sub) {
    setSub(currentSub.id);
    setPanel('');
    setBackground('');
  }

  const switchTab = (id: string) => {
    if (id === selected) return;
    setSelected(id);
    setPanel('');
    setBackground('');
    const first = tabs.find((tab) => tab.id === id)?.subs[0];
    setSub(first?.id ?? '');
  };

  // 滚动续载：哨兵进视口（400px 提前量）就向服务端翻下一页（每页 10 条，
  // offset=响应的 next_offset + 回传 session_id/rank_version，见
  // service/queries/rank.ts）。API 分页天然一批 10 条，不再需要客户端渲染批量。
  const sentinelRef = useRef<HTMLDivElement | null>(null);
  useEffect(() => {
    const el = sentinelRef.current;
    if (!el) return;
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((e) => e.isIntersecting)) rank.loadMore();
      },
      { rootMargin: `${SENTINEL_ROOT_MARGIN_PX}px` },
    );
    observer.observe(el);
    return () => observer.disconnect();
  });

  const tabRow = showTabsRow ? tabs : [];

  return (
    // 内容分类 tab 已上移 AppShell 顶栏（TopBarTabsPortal，见下），
    // 页面里只剩子榜竖排 + 榜单内容。
    // h-full 锁在视口内：AppShell 的 main 是 overflow-y-auto，页面自身
    // 不产生滚动——只有右侧榜单列表滚（同首页的锁法），左侧子榜分类
    // 和子榜名/筛选行保持常驻。
    <div className="flex h-full min-h-0 flex-col gap-4">
      {/* 顶栏中部：内容 tab（全部/真人剧/漫剧/AI剧/系列剧）。
          schema 未到时先渲染胶囊骨架占位（形态未知，比空白好） */}
      <TopBarTabsPortal>
        {tabRow.map((tab) => (
          <TopBarTab key={tab.id} active={tab.id === selected} onClick={() => switchTab(tab.id)}>
            {tab.name}
          </TopBarTab>
        ))}
        {tabRow.length === 0 && <Skeleton className="h-6 w-64 rounded-full" />}
      </TopBarTabsPortal>
      <div className="flex min-h-0 flex-1 gap-4">
        {/* 左侧子榜竖排（首屏未到时骨架占位）。
            窗口过矮时子榜自己滚，不跟着右侧列表走 */}
        <nav
          className="flex w-36 shrink-0 scrollbar-thin flex-col gap-1 overflow-y-auto"
          aria-label={t('rank.subLists')}
        >
          <ToggleGroup
            type="single"
            spacing={1}
            value={sub}
            onValueChange={(v) => {
              if (v === '') return;
              setSub(v);
              setPanel('');
              setBackground('');
            }}
            className="flex flex-col gap-1"
          >
            {(currentTab?.subs ?? []).map((s) => (
              <ToggleGroupItem
                key={s.id}
                value={s.id}
                className="text-muted-foreground hover:bg-accent hover:text-accent-foreground data-[state=on]:bg-primary data-[state=on]:text-primary-foreground h-auto w-full justify-start rounded-md px-3 py-2 text-left text-sm transition-colors data-[state=on]:font-medium"
              >
                {s.name}
              </ToggleGroupItem>
            ))}
          </ToggleGroup>
          {(currentTab?.subs.length ?? 0) === 0 &&
            isLoading &&
            Array.from({ length: 5 }, (_, i) => <Skeleton key={i} className="h-9 rounded-md" />)}
        </nav>

        <div className="flex min-w-0 flex-1 flex-col gap-2">
          {/* 右上：子榜名 + 描述行 + 筛选面板入口（官方同款排版） */}
          <div className="flex items-center justify-between gap-2">
            <p className="text-muted-foreground min-w-0 truncate text-sm">
              <span>{currentSub?.name ?? currentTab?.name ?? ''}</span>
              {currentSub?.description && (
                <span className="text-muted-foreground/70 ml-2 text-xs">
                  {currentSub.description}
                </span>
              )}
            </p>
            {currentSub && (currentSub.panel.length > 0 || currentSub.background.length > 0) && (
              <div className="flex items-center gap-2">
                {/* 榜单历史（月榜）：仅月榜型子榜下发选项（必看/收藏/臻果…） */}
                {currentSub.background.length > 0 && (
                  <FilterPanelButton
                    rows={currentSub.background}
                    value={background}
                    onPick={(id) => setBackground(id)}
                    placeholder={t('rank.filter.month')}
                  />
                )}
                {currentSub.panel.length > 0 && (
                  <FilterPanelButton
                    rows={currentSub.panel}
                    value={panel}
                    onPick={(id) => setPanel(id)}
                  />
                )}
              </div>
            )}
          </div>

          {/* 列表滚动区（页面唯一会滚的地方）：切子榜/筛选用 key 重挂载
              归零滚动——新榜单从第 1 名看起，而不是停在旧榜单的滚动位置。
              去播放再回来时（同 sub/panel）恢复上次滚动位置 */}
          <RankListScroll
            sub={sub}
            panel={panel}
            background={background}
            ready={!isLoading && listItems.length > 0}
          >
            {isLoading ? (
              <SkeletonRows count={8} height="h-24 rounded-xl" />
            ) : error ? (
              <ErrorState
                message={t('rank.loadFailed')}
                detail={error.message}
                onRetry={() => void refetch()}
              />
            ) : (
              /* 切子榜/筛选时 keepPreviousData 保住旧列表：降透明度禁点，
                 而不是闪骨架屏——旧内容可看但不可点 */
              <RefreshShade refreshing={isFetching}>
                <div className="flex flex-col gap-2">
                  {listItems.map((item) => (
                    <RankRow key={item.seriesId} item={item} />
                  ))}
                  {/* 续载哨兵：常驻（滚动容器内 400px 提前量触发服务端翻页）。
                      到底后（!hasMore）不移除也无妨——回调里 loadMore 自会空转 */}
                  <div ref={sentinelRef} className="h-px" aria-hidden />
                  {rank.isFetchingMore && (
                    <p className="text-muted-foreground flex items-center justify-center gap-2 py-2 text-sm">
                      <Spinner aria-hidden />
                      {t('feed.loadingMore')}
                    </p>
                  )}
                  {listItems.length === 0 && (
                    <Empty className="py-16">
                      <EmptyHeader>
                        <EmptyTitle className="text-muted-foreground text-sm font-normal">
                          {t('rank.empty')}
                        </EmptyTitle>
                      </EmptyHeader>
                    </Empty>
                  )}
                </div>
              </RefreshShade>
            )}
          </RankListScroll>
        </div>
      </div>

      {/* 内容 tab 条：贴着页面底部常驻（sticky，列表长时滚动中也钉在底边）。
          居中胶囊组，选中态 = 主色胶囊（与左侧子榜选中同款）；
          两级形态才显示（登录一级形态只有一个合成 tab，隐藏整行） */}
      {/* 内容 tab 条已上移 AppShell 顶栏（本文件上方 TopBarTabsPortal） */}
    </div>
  );
}

/** 榜单滚动容器：保留 key 重挂载归零的语义，另在「去播放再回来」时
 *  （同 sub/panel/月榜 的重挂载）恢复上次滚动位置。 */
function RankListScroll({
  sub,
  panel,
  background,
  ready,
  children,
}: {
  sub: string;
  panel: string;
  background: string;
  ready: boolean;
  children: React.ReactNode;
}) {
  const ref = useRef<HTMLDivElement | null>(null);
  useScrollRestore('hongguo.rank.scroll', () => ref.current, ready);
  return (
    <div
      key={`${sub}:${panel}:${background}`}
      ref={ref}
      className="min-h-0 flex-1 scrollbar-thin overflow-y-auto"
    >
      {children}
    </div>
  );
}
