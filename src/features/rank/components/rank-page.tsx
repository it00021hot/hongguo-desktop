import { useEffect, useMemo, useRef, useState } from 'react';
import { useNavigate } from '@tanstack/react-router';
import { Bell, Check, ChevronDown, Flame, Loader2, Play, SlidersHorizontal, Tv, X } from 'lucide-react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { cn } from '@/lib/utils';
import { Skeleton } from '@/components/ui/skeleton';
import { RefreshShade } from '@/components/refresh-shade';
import { SkeletonRows } from '@/components/skeletons';
import { TopBarTab, TopBarTabsPortal } from '@/components/layout/top-bar-tabs';
import { isRenderableCover, useRank, useReserveSeries, useWebCover } from '@/lib/queries';
import { usePlaySeries } from '@/lib/use-play-series';
import { t } from '@/i18n';
import type { RankItem, RankSubList, RankTab } from '@/lib/schema';

/**
 * 排行榜页（对齐 hgplayer 1.1.3 布局，2026-10-05 抓包）：
 * 内容 tab（全部/真人剧/漫剧/AI剧/系列剧）× 左侧子榜竖排 × 右上筛选面板
 * （panel_selected_items 单选），子榜名旁带「N月N日已更新·基于…」描述行。
 *
 * 选项表由接口随行下发（响应 cell_selector）。形态由客户端自报版本号
 * 决定（与登录态无关，设备档案已在后端对齐 73932），normalizeTabs 保留
 * 一级形态兼容：
 * - 两级（73932）：tab=内容分类 → sub=子榜（含分组筛选面板行）；
 * - 一级（老版本身份，理论上不再出现）：tab=榜单 → sub=筛选选项平铺。
 *
 * 「演员」tab 响应无剧集数据（celebrity 形态），不提供入口。
 */

/** 服务端两种 selector 形态归一成统一的「内容tab → 子榜(含筛选面板)」。 */
function normalizeTabs(tabs: RankTab[]): RankTab[] {
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

/** 首屏 schema 未到时 tab 行/子榜给骨架占位（形态未知，比整块空白好）。 */

export function RankPage() {
  const [selected, setSelected] = useState('all');
  // 子榜 id；tab 切换时重置为新 tab 的第一个子榜
  const [sub, setSub] = useState('ranklist_hot_sc');
  // 筛选面板选中项（'' = 总榜，即无筛选）
  const [panel, setPanel] = useState('');
  const { data, isLoading, error, isFetching, refetch } = useRank(selected, sub, panel);

  const tabs = useMemo(() => normalizeTabs(data?.tabs ?? []), [data?.tabs]);
  // 首屏加载中先显示 tab 行骨架；形态确定后仅两级形态显示
  // （登录一级形态只有一个合成 tab，隐藏整行）；出错时不渲染
  const showTabsRow = data !== undefined ? data.tabs.some((tab) => tab.id === 'all') : isLoading;
  const currentTab = tabs.find((tab) => tab.id === selected);
  const currentSub = currentTab?.subs.find((s) => s.id === sub) ?? currentTab?.subs[0];

  // schema 到达（或登录态形态切换）后，当前 sub 不在新 tab 的子榜里时
  // 落到第一个子榜（render 期调整 state 的官方模式，立即用新值重渲染；
  // subs 非空时 currentSub 必有值，undefined 只可能伴随空 subs）
  if (currentTab && currentSub && currentSub.id !== sub) {
    setSub(currentSub.id);
    setPanel('');
  }

  const switchTab = (id: string) => {
    if (id === selected) return;
    setSelected(id);
    setPanel('');
    const first = tabs.find((tab) => tab.id === id)?.subs[0];
    setSub(first?.id ?? '');
  };

  // 渐进渲染（历史页同款）：榜单一次可到百条，整表挂载白卡切换瞬间。
  // 首批 20 行 + 哨兵续载；子榜/筛选切换重置批量
  const [visibleCount, setVisibleCount] = useState(20);
  const [prevListKey, setPrevListKey] = useState('');
  const listKey = `${selected}:${sub}:${panel}`;
  if (prevListKey !== listKey) {
    setPrevListKey(listKey);
    setVisibleCount(20);
  }
  const sentinelRef = useRef<HTMLDivElement | null>(null);
  const listItems = data?.items ?? [];
  useEffect(() => {
    const el = sentinelRef.current;
    if (!el) return;
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((e) => e.isIntersecting)) {
          setVisibleCount((n) => (n < listItems.length ? n + 40 : n));
        }
      },
      { rootMargin: '600px' },
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
        {/* 左侧子榜竖排（hgplayer 同款形态；首屏未到时骨架占位）。
            窗口过矮时子榜自己滚，不跟着右侧列表走 */}
        <nav
          className="flex w-36 shrink-0 scrollbar-thin flex-col gap-1 overflow-y-auto"
          aria-label={t('rank.subLists')}
        >
          {(currentTab?.subs ?? []).map((s) => (
            <button
              key={s.id}
              type="button"
              onClick={() => {
                if (s.id === sub) return;
                setSub(s.id);
                setPanel('');
              }}
              className={cn(
                'rounded-md px-3 py-2 text-left text-sm transition-colors',
                s.id === sub
                  ? 'bg-primary text-primary-foreground font-medium'
                  : 'text-muted-foreground hover:bg-accent hover:text-accent-foreground',
              )}
            >
              {s.name}
            </button>
          ))}
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
            {currentSub && currentSub.panel.length > 0 && (
              <FilterPanelButton
                rows={currentSub.panel}
                value={panel}
                onPick={(id) => setPanel(id)}
              />
            )}
          </div>

          {/* 列表滚动区（页面唯一会滚的地方）：切子榜/筛选用 key 重挂载
              归零滚动——新榜单从第 1 名看起，而不是停在旧榜单的滚动位置 */}
          <div key={`${sub}:${panel}`} className="min-h-0 flex-1 scrollbar-thin overflow-y-auto">
            {isLoading ? (
              <SkeletonRows count={8} height="h-24 rounded-xl" />
            ) : error ? (
              <div className="text-muted-foreground flex flex-col items-center gap-3 py-16">
                <p>{t('rank.loadFailed')}</p>
                <p className="text-destructive text-xs">{error.message}</p>
                <Button variant="outline" size="sm" onClick={() => void refetch()}>
                  <Loader2 className="mr-1 size-4 animate-spin" aria-hidden />
                  {t('feed.retry')}
                </Button>
              </div>
            ) : (
              /* 切子榜/筛选时 keepPreviousData 保住旧列表：降透明度禁点，
                 而不是闪骨架屏——旧内容可看但不可点 */
              <RefreshShade refreshing={isFetching}>
                <div className="flex flex-col gap-2">
                  {listItems.slice(0, visibleCount).map((item) => (
                    <RankRow key={item.seriesId} item={item} />
                  ))}
                  {/* 续载哨兵（600px 提前量） */}
                  {visibleCount < listItems.length && <div ref={sentinelRef} className="h-px" />}
                  {listItems.length === 0 && (
                    <p className="text-muted-foreground py-16 text-center text-sm">
                      {t('rank.empty')}
                    </p>
                  )}
                </div>
              </RefreshShade>
            )}
          </div>
        </div>
      </div>

      {/* 内容 tab 条：贴着页面底部常驻（sticky，列表长时滚动中也钉在底边）。
          居中胶囊组，选中态 = 主色胶囊（与左侧子榜选中同款）；
          两级形态才显示（登录一级形态只有一个合成 tab，隐藏整行） */}
      {/* 内容 tab 条已上移 AppShell 顶栏（本文件上方 TopBarTabsPortal） */}
    </div>
  );
}

/** 「总榜 ▾」筛选按钮 + 弹出面板（row_name 分行，选项单选整组替换）。 */
function FilterPanelButton({
  rows,
  value,
  onPick,
}: {
  rows: { name: string; items: { id: string; name: string }[] }[];
  value: string;
  onPick: (id: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement | null>(null);
  const selectedName = rows
    .flatMap((r) => r.items)
    .find((it) => it.id === value && it.id !== '')?.name;

  // 点击面板外部即收起（hgplayer 同款交互）
  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (rootRef.current && !rootRef.current.contains(e.target as Node)) setOpen(false);
    };
    window.addEventListener('mousedown', onDown);
    return () => window.removeEventListener('mousedown', onDown);
  }, [open]);

  return (
    <div ref={rootRef} className="relative">
      <Button variant="outline" size="sm" onClick={() => setOpen((o) => !o)}>
        {selectedName ? (
          <>
            <SlidersHorizontal className="size-4" aria-hidden />
            {selectedName}
          </>
        ) : (
          <>
            <SlidersHorizontal className="size-4" aria-hidden />
            {rows[0]?.items.find((it) => it.id === '')?.name ?? t('rank.filter.all')}
          </>
        )}
        <ChevronDown
          className={cn('size-4 transition-transform', open && 'rotate-180')}
          aria-hidden
        />
      </Button>

      {open && (
        <div className="bg-popover text-popover-foreground absolute right-0 z-20 mt-1 max-h-[60vh] w-96 overflow-y-auto rounded-lg border p-3 shadow-lg">
          <div className="mb-2 flex items-center justify-between">
            <p className="text-sm font-medium">{t('rank.filter.title')}</p>
            <button
              type="button"
              className="text-muted-foreground hover:text-foreground"
              onClick={() => setOpen(false)}
              aria-label={t('common.close')}
            >
              <X className="size-4" aria-hidden />
            </button>
          </div>
          {value !== '' && (
            <div className="mb-2 flex justify-end">
              <Button variant="ghost" size="sm" onClick={() => onPick('')}>
                {t('rank.filter.reset')}
              </Button>
            </div>
          )}
          <div className="flex flex-col gap-3">
            {rows.map((row) => (
              <div key={row.name} className="flex flex-col gap-1.5">
                <p className="text-muted-foreground text-xs">{row.name}</p>
                <div className="flex flex-wrap gap-1.5">
                  {row.items.map((it) => (
                    <button
                      key={it.id === '' ? '__all__' : it.id}
                      type="button"
                      onClick={() => {
                        onPick(it.id);
                        setOpen(false);
                      }}
                      className={cn(
                        'rounded-full border px-2.5 py-1 text-xs transition-colors',
                        it.id === value
                          ? 'border-primary bg-primary text-primary-foreground'
                          : 'hover:bg-accent hover:text-accent-foreground',
                      )}
                    >
                      {it.name}
                    </button>
                  ))}
                </div>
              </div>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}

/** 榜单一行：名次 | 封面 | 标题/副标题/简介 | 热度文案。 */
function RankRow({ item }: { item: RankItem }) {
  const navigate = useNavigate();
  const playSeries = usePlaySeries();
  const { data: webCover } = useWebCover(item.cover);
  const sourceRenderable = isRenderableCover(item.cover);
  const cover = webCover ?? (sourceRenderable ? item.cover : '');
  const [brokenFor, setBrokenFor] = useState('');
  const imgBroken = brokenFor !== '' && brokenFor === cover;
  const showImg = cover !== '' && !imgBroken;
  // 预约态本地乐观翻转（详情页 ReserveButton 同款；榜单条目自带
  // online_subscribed 初始值，跨客户端操作过也能显示）
  const reserve = useReserveSeries();
  const [reserved, setReserved] = useState(item.reserved);

  // 卡片一律进详情（hgplayer 同款：详情看档案，播放/预约是右侧明确动作）。
  // 带上榜单自带的档案快照：未上线剧详情解析分集必然失败，prefill 支撑
  // 详情页渲染「即将上线」降级视图（而非整页报错）
  const openDetail = () =>
    void navigate({
      to: '/detail',
      search: {
        seriesId: item.seriesId,
        title: item.title,
        cover: item.cover,
        tags: item.tags.join(','),
        desc: item.description,
      },
    });
  const onToggleReserve = () => {
    const next = !reserved;
    reserve.mutate(
      { seriesId: item.seriesId, reserve: next },
      {
        onSuccess: () => {
          setReserved(next);
          toast.success(t(next ? 'player.interact.reserved' : 'player.interact.unreserved'));
        },
        onError: (err) => toast.error(String(err)),
      },
    );
  };

  const rankNo = item.rank > 0 ? item.rank : undefined;
  // 官方条目双信息：🔥主热词（recText，如 "995万推荐"）+ 次信息（"5490万热度"）
  const rec = item.recText;
  const secondary = item.secondaryInfos.filter((s) => s !== '' && s !== rec);
  // 元信息一行：副标题 · 评分 · 题材标签（hgplayer 同款行）
  const meta = [item.subTitle, item.score > 0 ? item.score.toFixed(1) : '', ...item.tags]
    .filter((s) => s !== '')
    .join(' · ');

  return (
    <article
      role="button"
      tabIndex={0}
      aria-label={item.title}
      onClick={openDetail}
      onKeyDown={(e) => {
        if (e.key !== 'Enter' && e.key !== ' ') return;
        e.preventDefault();
        openDetail();
      }}
      className="bg-card hover:border-foreground/30 focus-visible:border-foreground/30 flex cursor-pointer items-center gap-4 rounded-xl border p-3 text-left transition-colors hover:shadow-md focus-visible:outline-none [content-visibility:auto] [contain-intrinsic-size:auto_112px]"
    >
      {rankNo !== undefined && (
        <span
          className={`w-8 shrink-0 text-center text-2xl font-black tabular-nums ${
            rankNo <= 3 ? 'text-amber-500' : 'text-muted-foreground/50'
          }`}
          aria-hidden
        >
          {rankNo}
        </span>
      )}

      <div className="bg-muted relative aspect-[3/4] w-20 shrink-0 overflow-hidden rounded-lg">
        {showImg ? (
          <img
            src={cover}
            alt=""
            loading="lazy"
            className="size-full object-cover"
            onError={() => setBrokenFor(cover)}
          />
        ) : (
          <div className="text-muted-foreground grid size-full place-items-center">
            <Tv className="size-5" />
          </div>
        )}
      </div>

      <div className="flex min-w-0 flex-1 flex-col gap-1">
        <div className="flex items-center gap-1.5">
          <p className="truncate text-sm font-semibold" title={item.title}>
            {item.title}
          </p>
          {item.season !== '' && (
            <span className="text-muted-foreground shrink-0 rounded border px-1 text-[10px] leading-4">
              {item.season}
            </span>
          )}
        </div>
        {meta !== '' && (
          <p className="text-muted-foreground truncate text-xs">{meta}</p>
        )}
        {item.description !== '' && (
          <p className="text-muted-foreground/80 line-clamp-2 text-xs leading-relaxed">
            {item.description}
          </p>
        )}
        {(rec !== '' || secondary.length > 0) && (
          <p className="flex items-center gap-2 text-xs">
            {rec !== '' && (
              <span className="flex shrink-0 items-center gap-0.5 text-orange-400">
                <Flame className="size-3" aria-hidden />
                {rec}
              </span>
            )}
            {secondary.length > 0 && (
              <span className="text-muted-foreground truncate whitespace-nowrap">
                {secondary.join('  ')}
              </span>
            )}
          </p>
        )}
      </div>

      {/* 行尾动作列（按剧状态分）：未上线 = 预约/已预约；已上线 = 播放 +
          详情链接。按钮统一定位风格（与历史页「继续播放」同款 outline）。 */}
      <div className="flex shrink-0 flex-col items-stretch gap-1.5 self-center">
        {item.upcoming ? (
          <Button
            size="sm"
            variant="outline"
            className="shrink-0 gap-1"
            disabled={reserve.isPending}
            onClick={(e) => {
              e.stopPropagation();
              onToggleReserve();
            }}
          >
            {reserve.isPending ? (
              <Loader2 className="size-3.5 animate-spin" aria-hidden />
            ) : reserved ? (
              <Check className="size-3.5" aria-hidden />
            ) : (
              <Bell className="size-3.5" aria-hidden />
            )}
            {t(reserved ? 'player.reserved' : 'player.reserve')}
          </Button>
        ) : (
          <>
            <Button
              size="sm"
              variant="outline"
              className="shrink-0 gap-1"
              onClick={(e) => {
                e.stopPropagation();
                playSeries(item.seriesId);
              }}
            >
              <Play className="size-3.5" aria-hidden />
              {t('player.play')}
            </Button>
            <button
              type="button"
              className="text-muted-foreground hover:text-foreground cursor-pointer text-center text-xs transition-colors"
              onClick={(e) => {
                e.stopPropagation();
                openDetail();
              }}
            >
              {t('rank.detail')}
            </button>
          </>
        )}
      </div>
    </article>
  );
}
