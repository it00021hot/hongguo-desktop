import { useEffect, useMemo, useRef, useState } from 'react';
import { ChevronDown, Loader2, Search, SlidersHorizontal, X } from 'lucide-react';
import { toast } from 'sonner';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { RefreshShade } from '@/components/refresh-shade';
import { ResolvingPill } from '@/components/resolving-pill';
import { SkeletonCardGrid } from '@/components/skeletons';
import { SeriesCardGrid } from './series-card-grid';
import {
  useBrowseFeed,
  useBrowsePanel,
  useDownloadTasks,
  useResolveSeries,
  useSearchSuggest,
  useSeriesSearchApp,
  useWebCover,
} from '@/lib/queries';
import { usePlaySeries } from '@/lib/use-play-series';
import { useUiStore } from '@/lib/stores/ui';
import { t, tf } from '@/i18n';
import { cn } from '@/lib/utils';
import type { BrowseFilters, FeedItem, SeriesCard, SuggestItem } from '@/lib/schema';

/**
 * 判断输入框里装的是「站内搜索词」还是「要解析的剧」。
 *
 * 链接与 ID 走 resolve 链路、按剧名才走搜索：站内搜索只匹配剧名，
 * 粘一段分享链接进去永远搜不到任何东西。
 *
 * 规则要窄而确定。分享语是整段自然语言（标题 + 链接 + 引导语），
 * 判据只能锚在 `series_id=` 参数和 `http` 前缀上；ID 则必须是纯数字串，
 * 且留足长度下限，否则「2024」这种词会被当成剧集号解析出不相干的东西。
 */
function detectInput(value: string): 'keyword' | 'resolve' {
  const v = value.trim();
  if (v.includes('series_id=') || v.startsWith('http')) return 'resolve';
  if (/^\d{6,}$/.test(v.replace(/\s+/g, ''))) return 'resolve';
  return 'keyword';
}

/** App 筛选条目转官网卡形态喂同一块网格。 */
function toCard(it: FeedItem): SeriesCard {
  return {
    seriesId: it.seriesId,
    seriesTitle: it.title,
    cover: it.cover,
    episodeCount: it.episodeCnt,
    tags: it.tags.slice(0, 3),
    url: '',
  };
}

/** 面板行 type → 行头标签（未知类型回落服务端行名去掉「全部」前缀）。 */
const FILTER_LABEL_KEYS: Record<string, string> = {
  genre: 'browse.fGenre',
  category_dim_theme: 'browse.fTheme',
  category_dim_role: 'browse.fRole',
  category_dim_epoch: 'browse.fEpoch',
  sort: 'browse.fSort',
  gender: 'browse.fGender',
  online_time: 'browse.fOnlineTime',
  duration: 'browse.fDuration',
};

/** 服务端面板缺「长度」行时的合成兜底（选项 id 来自 2026-10-07 抓包，
 * 实测 select_items.duration 服务端必认）。 */
const DURATION_FALLBACK = {
  rowType: 'duration',
  rowName: '全部长度',
  items: [
    { id: 'duration_0_60', name: '0-60分钟' },
    { id: 'duration_60_120', name: '60-120分钟' },
    { id: 'duration_120_plus', name: '120分钟以上' },
  ],
};

function withDurationFallback(
  rows: { rowType: string; rowName: string; items: { id: string; name: string }[] }[],
) {
  if (rows.some((r) => r.rowType === 'duration')) return rows;
  return [...rows, DURATION_FALLBACK];
}

/**
 * 浏览页：顶部一个搜索框，下面是官方筛选面板 + 结果网格。
 *
 * 数据走官方 App 的找剧接口（landpage 筛选面板 + 多维 select_items），
 * 与第三方客户端同款：体裁/主题/设定/背景/推荐/受众/时间/长度八行，
 * 每行单选，「全部」即空选；选项表随服务端下发，不写死。
 *
 * 搜索与浏览共用同一块结果区：提交关键词就原地切成搜索结果，
 * 清空或退出就回到筛选列表。
 */
export function BrowsePage() {
  const filtersCollapsed = useUiStore((s) => s.browseFiltersCollapsed);
  const setBrowseFiltersCollapsed = useUiStore((s) => s.setBrowseFiltersCollapsed);
  const [filters, setFilters] = useState<BrowseFilters>({
    genre: '',
    theme: '',
    role: '',
    epoch: '',
    sort: '',
    gender: '',
    onlineTime: '',
    duration: '',
  });

  const [keyword, setKeyword] = useState('');
  /** 已提交的搜索词：空串 = 浏览模式，非空 = 搜索模式 */
  const [submitted, setSubmitted] = useState('');
  const searching = submitted !== '';

  // ---- 输入联想（hgplayer 1.1.6 同款）：停 300ms 才发请求，
  //      有 seriesId 的条目点击直进播放器，纯词条目回填发起搜索 ----
  const [debounced, setDebounced] = useState('');
  useEffect(() => {
    const timer = setTimeout(() => setDebounced(keyword), 300);
    return () => clearTimeout(timer);
  }, [keyword]);
  const suggest = useSearchSuggest(debounced);
  const [suggestOpen, setSuggestOpen] = useState(false);
  const [suggestIndex, setSuggestIndex] = useState(0);
  const suggestions = suggest.data ?? [];
  const showSuggest =
    suggestOpen &&
    !searching &&
    keyword.trim().length >= 2 &&
    keyword === debounced &&
    suggestions.length > 0;
  const playSeries = usePlaySeries();
  const pickSuggest = (item: SuggestItem) => {
    setSuggestOpen(false);
    if (item.seriesId) {
      playSeries(item.seriesId);
      return;
    }
    setKeyword(item.word);
    setSubmitted(item.word);
  };

  const panel = useBrowsePanel();
  // 找剧流（无限滚动）：session_id 游标翻页，与 hgplayer 同款
  const browse = useBrowseFeed(filters);
  // 找剧搜索走官方 App API（站内官网搜索只匹配剧名且结果少；
  // App 搜索是综合 tab，首页精选 + 翻页全量）
  const found = useSeriesSearchApp(submitted);
  const { data: tasks } = useDownloadTasks();
  const { mutate: resolve, isPending: resolving } = useResolveSeries();

  // 搜索模式下用搜索结果盖掉分类结果，退出搜索再换回来。
  // App 搜索条目转成官网卡形态喂同一块网格：subTitle（"脑洞·全273集"）
  // 首段当题材 tag，url 无处消费填空串。
  const cards = useMemo(() => {
    if (!searching) return browse.items.map(toCard);
    return found.items.map((it) => ({
      seriesId: it.seriesId,
      seriesTitle: it.title,
      cover: it.cover,
      episodeCount: it.episodeCnt,
      tags: it.subTitle.split('·').slice(0, 1).filter(Boolean),
      url: '',
    }));
  }, [searching, browse.items, found.items]);
  const pending = searching ? found.isLoading : browse.isLoading;
  const refreshing = searching ? found.isRefreshing : browse.isRefreshing;
  const failed = searching ? found.error !== null : browse.error !== null;
  // 提交按钮态：同一关键词还在搜索中就灰掉防连点；改了词不拦（允许直接重提）
  const resubmitting = searching && found.isLoading && keyword.trim() === submitted;

  // 哨兵触发用的最新流状态镜像（browse 每次渲染是新对象）
  const browseRef = useRef(browse);
  useEffect(() => {
    browseRef.current = browse;
  });
  const sentinelRef = useRef<HTMLDivElement | null>(null);
  useEffect(() => {
    const el = sentinelRef.current;
    if (!el) return;
    const io = new IntersectionObserver(
      (entries) => {
        const b = browseRef.current;
        if (
          entries[0]?.isIntersecting &&
          !searching &&
          b.items.length > 0 &&
          !b.isLoading &&
          !b.isFetchingMore
        ) {
          void b.loadMore();
        }
      },
      { rootMargin: '400px' },
    );
    io.observe(el);
    return () => io.disconnect();
    // searching 进 deps：回调闭包里的它必须与当前模式同步，否则搜索中
    // 哨兵还会用旧值触发分类流的 loadMore；重建 observer 的代价可忽略
  }, [searching]);

  // App 搜索首页是「精选」少数条目，hasMore 翻页才是全量列表；
  // 结果还很少时自动续拉一页，避免用户看到 4 条就以为搜完了。
  const searchStateRef = useRef(found);
  useEffect(() => {
    searchStateRef.current = found;
  });
  useEffect(() => {
    if (!searching) return;
    const f = searchStateRef.current;
    if (f.hasMore && !f.isLoading && !f.isFetchingMore && f.items.length < 18) {
      void f.loadMore();
    }
    // items.length 变化会再次进入：靠 isFetchingMore 挡住并发，靠 hasMore 收尾
  }, [searching, found.items.length]);

  // 已下载集数：按剧聚合，供卡片角标使用
  const downloadedMap = useMemo(() => {
    const map: Record<string, number> = {};
    for (const task of tasks ?? []) {
      if (task.status !== 'completed') continue;
      map[task.seriesId] = (map[task.seriesId] ?? 0) + 1;
    }
    return map;
  }, [tasks]);

  const total = cards.length;

  // 退出搜索：置空提交词即可——浏览结果在 Query 缓存里秒回，
  // 搜索结果留在缓存，重复搜索同一关键词也秒出不再重拉
  const exitSearch = () => {
    setSubmitted('');
    setKeyword('');
  };

  /** 已选筛选条件数（折叠徽标用；空选=全部不计） */
  const activeFilterCount = useMemo(
    () => Object.values(filters).filter((v) => v !== '').length,
    [filters],
  );

  const pick = (key: keyof BrowseFilters, value: string) => {
    setFilters((prev) => (prev[key] === value ? prev : { ...prev, [key]: value }));
  };

  // 链接/ID 输入要先解析出剧集 id（顺带校验链接有效性），关键词搜索只改
  // 提交词：useSeriesSearchApp 随 queryKey 自动发起请求。解析前先退出搜索
  // 模式，否则解析成功后列表还停在上一轮搜索结果上。
  const handleSubmit = () => {
    const value = keyword.trim();
    if (!value) return;
    if (detectInput(value) === 'keyword') {
      setSubmitted(value);
      return;
    }
    setSubmitted('');
    resolve(value, {
      onSuccess: (series) => playSeries(series.seriesId),
      onError: (e) => toast.error(e.message),
    });
  };

  // 点卡片直接跳播放：分集解析由播放页自己拉（失败有整页错误态），
  // 这里不再经选集抽屉中转——下载也在播放器里做。
  const handleSelect = (card: SeriesCard) => playSeries(card.seriesId);

  return (
    <div className="flex flex-col gap-4 p-6">
      <div className="flex flex-wrap items-center gap-2">
        <form
          className="relative flex min-w-64 flex-1 gap-2"
          onSubmit={(e) => {
            e.preventDefault();
            setSuggestOpen(false);
            handleSubmit();
          }}
        >
          <div className="relative min-w-48 flex-1">
            <Input
              value={keyword}
              onChange={(e) => {
                setKeyword(e.target.value);
                setSuggestOpen(true);
              }}
              onFocus={() => setSuggestOpen(true)}
              onBlur={() => setSuggestOpen(false)}
              onKeyDown={(e) => {
                // 输入法组合中（拼音未上屏）：Enter 是字母上屏、方向键归
                // 输入法——不碰联想/提交
                if (e.nativeEvent.isComposing || e.nativeEvent.keyCode === 229) return;
                if (!showSuggest) return;
                if (e.key === 'ArrowDown') {
                  e.preventDefault();
                  setSuggestIndex((i) => (i + 1) % suggestions.length);
                } else if (e.key === 'ArrowUp') {
                  e.preventDefault();
                  setSuggestIndex((i) => (i - 1 + suggestions.length) % suggestions.length);
                } else if (e.key === 'Enter') {
                  // 联想开着时 Enter 选中高亮条目，不再走提交搜索
                  e.preventDefault();
                  const picked = suggestions[suggestIndex] ?? suggestions[0];
                  if (picked) pickSuggest(picked);
                } else if (e.key === 'Escape') {
                  setSuggestOpen(false);
                }
              }}
              placeholder={t('search.placeholder')}
              aria-label={t('search.placeholder')}
              className="min-w-48 flex-1"
              autoComplete="off"
            />
            {/* 联想下拉：条目用 onMouseDown(preventDefault) 选中——
                比 blur 早一拍，点条目不会先把下拉收掉 */}
            {showSuggest && (
              <div className="bg-popover text-popover-foreground absolute inset-x-0 top-full z-30 mt-1.5 max-h-80 scrollbar-thin overflow-y-auto rounded-xl border p-1.5 shadow-lg">
                {suggestions.slice(0, 8).map((item, i) => (
                  <SuggestRow
                    key={`${item.word}:${i}`}
                    item={item}
                    active={i === suggestIndex}
                    onHover={() => setSuggestIndex(i)}
                    onPick={pickSuggest}
                  />
                ))}
              </div>
            )}
          </div>
          <Button type="submit" disabled={!keyword.trim() || resolving || resubmitting}>
            {resubmitting ? (
              <Loader2 className="size-4 animate-spin" />
            ) : (
              <Search className="size-4" />
            )}
            {t('search.submit')}
          </Button>
          {resolving && (
            <span className="text-muted-foreground self-center text-sm whitespace-nowrap">
              {t('browse.resolving')}
            </span>
          )}
          {searching && (
            <Button type="button" variant="outline" onClick={exitSearch}>
              <X className="size-4" />
              {t('search.exit')}
            </Button>
          )}
        </form>

        {!searching && total > 0 && (
          <span className="text-muted-foreground ml-auto text-sm">
            {tf('browse.totalCount', { total })}
          </span>
        )}
      </div>

      {/* 官方筛选面板（第三方同款八行，可整块折叠）：选项表服务端下发，
          空选即「全部」。「长度」行部分设备不下发（2026-10-07 实测），但
          select_items.duration 服务端必认——缺行时用抓包锁定的选项 id 合成兜底。
          折叠态只留开关行：徽标显示已选条件数，一键展开不用从头找。 */}
      {!searching && (
        <div className="flex flex-col gap-1.5">
          <button
            type="button"
            onClick={() => setBrowseFiltersCollapsed(!filtersCollapsed)}
            aria-expanded={!filtersCollapsed}
            className="text-muted-foreground hover:text-foreground flex w-fit cursor-pointer items-center gap-1.5 text-sm transition-colors"
          >
            <SlidersHorizontal className="size-4" aria-hidden />
            {t('browse.filters')}
            {activeFilterCount > 0 && (
              <Badge variant="secondary" className="px-1.5 text-[10px]">
                {activeFilterCount}
              </Badge>
            )}
            <ChevronDown
              className={cn('size-4 transition-transform', filtersCollapsed ? '' : 'rotate-180')}
              aria-hidden
            />
          </button>
          {!filtersCollapsed && (
            <FilterPanel
              rows={withDurationFallback(panel.data ?? [])}
              filters={filters}
              loading={panel.isLoading}
              failed={panel.isError}
              onPick={pick}
            />
          )}
        </div>
      )}

      {failed && <p className="text-destructive text-sm">{t('browse.loadFailed')}</p>}

      {pending ? (
        <SkeletonCardGrid />
      ) : cards.length === 0 ? (
        <p className="text-muted-foreground py-16 text-center text-sm">
          {searching ? t('search.empty') : t('browse.empty')}
        </p>
      ) : (
        <RefreshShade refreshing={refreshing}>
          {/* 无限滚动：不再翻页补位，条目持续累积填满网格，
              「末行留空」的来源（18 条除不尽列数）自然消失 */}
          <SeriesCardGrid cards={cards} downloadedMap={downloadedMap} onSelect={handleSelect} />
        </RefreshShade>
      )}

      {!searching && cards.length > 0 && (
        <>
          <div ref={sentinelRef} className="h-px" aria-hidden />
          {browse.isFetchingMore && (
            <p className="text-muted-foreground flex items-center justify-center gap-2 py-2 text-sm">
              <Loader2 className="size-4 animate-spin" aria-hidden />
              {t('feed.loadingMore')}
            </p>
          )}
          {!browse.hasMore && !browse.isFetchingMore && (
            <p className="text-muted-foreground py-2 text-center text-sm">{t('feed.end')}</p>
          )}
        </>
      )}
      {/* 与首页同一交互：解析期间底部气泡，抽屉一开就是完整内容 */}
      {resolving && <ResolvingPill />}
    </div>
  );
}

/**
 * 联想行，结构对齐 hgplayer：放大镜图标常驻（纯词行就只有它 + 词）；
 * 带剧集的行前置竖版封面（40×54，HEIC 走 webp/转码链）；词按服务端
 * 命中位切片、命中片段上高亮色（#ff7a1a ≈ orange-500）、rich 行加粗；
 * 摘要行有才渲染。
 */
function SuggestRow({
  item,
  active,
  onHover,
  onPick,
}: {
  item: SuggestItem;
  active: boolean;
  onHover: () => void;
  onPick: (item: SuggestItem) => void;
}) {
  // 封面闸门对齐 hgplayer（cover 有无）：纯词联想不渲染封面，只有放大镜。
  const { data: webCover } = useWebCover(item.cover);
  return (
    <button
      type="button"
      // mousedown + preventDefault：抢在输入框 blur 收起下拉之前选中
      onMouseDown={(e) => {
        e.preventDefault();
        onPick(item);
      }}
      onMouseEnter={onHover}
      className={cn(
        'flex w-full cursor-pointer items-center gap-2.5 rounded-lg px-2.5 py-[7px] text-left transition-colors',
        active ? 'bg-accent' : 'hover:bg-accent/60',
      )}
    >
      <Search className="text-muted-foreground size-3.5 shrink-0" aria-hidden />
      {item.cover && webCover && (
        <img
          src={webCover}
          alt=""
          loading="lazy"
          className="bg-muted h-[54px] w-10 shrink-0 rounded-md object-cover"
        />
      )}
      <span className="flex min-w-0 flex-1 flex-col gap-0.5">
        <span className={cn('block truncate text-sm', item.seriesId && 'font-semibold')}>
          {item.parts.length > 0
            ? item.parts.map((part, i) =>
                part.hl ? (
                  <span key={i} className="text-orange-500">
                    {part.text}
                  </span>
                ) : (
                  <span key={i}>{part.text}</span>
                ),
              )
            : item.word}
        </span>
        {item.abstract && (
          <span className="text-muted-foreground block truncate text-xs">{item.abstract}</span>
        )}
      </span>
    </button>
  );
}

/**
 * 筛选面板：八行维度，每行「全部」+ 服务端选项，单选。
 * 行头标签按 type 映射 i18n（服务端 row_name 是中文，不适合多语言）。
 */
function FilterPanel({
  rows,
  filters,
  loading,
  failed,
  onPick,
}: {
  rows: { rowType: string; rowName: string; items: { id: string; name: string }[] }[];
  filters: BrowseFilters;
  loading: boolean;
  failed: boolean;
  onPick: (key: keyof BrowseFilters, value: string) => void;
}) {
  if (loading) {
    return (
      <div className="text-muted-foreground flex items-center gap-2 py-2 text-sm">
        <Loader2 className="size-3.5 animate-spin" />
        {t('common.loading')}
      </div>
    );
  }
  if (failed || rows.length === 0) return null;

  const rowValue = (key: string): string => {
    switch (key) {
      case 'genre':
        return filters.genre;
      case 'category_dim_theme':
        return filters.theme;
      case 'category_dim_role':
        return filters.role;
      case 'category_dim_epoch':
        return filters.epoch;
      case 'sort':
        return filters.sort;
      case 'gender':
        return filters.gender;
      case 'online_time':
        return filters.onlineTime;
      case 'duration':
        return filters.duration;
      default:
        return '';
    }
  };

  const pill = (key: keyof BrowseFilters, id: string, label: string, active: boolean) => (
    <button
      key={id || '__all__'}
      type="button"
      onClick={() => onPick(key, id)}
      aria-pressed={active}
      className={cn(
        'cursor-pointer rounded-full border px-3 py-0.5 text-xs transition-colors',
        active
          ? 'border-primary text-primary bg-primary/10 font-medium'
          : 'text-muted-foreground hover:bg-accent hover:text-foreground border-border',
      )}
    >
      {label}
    </button>
  );

  return (
    <div className="grid gap-1.5">
      {rows.map((row) => {
        const key = row.rowType as keyof BrowseFilters;
        const current = rowValue(row.rowType);
        const fallbackLabel = FILTER_LABEL_KEYS[row.rowType] ?? row.rowName.replace(/^全部/, '');
        return (
          <div key={row.rowType} className="flex items-start gap-3 text-sm">
            <span className="text-muted-foreground w-10 shrink-0 pt-1 text-xs">
              {t(fallbackLabel)}
            </span>
            <div className="flex flex-wrap gap-x-1 gap-y-1.5">
              {pill(key, '', t('browse.all'), current === '')}
              {row.items.map((it) => pill(key, it.id, it.name, current === it.id))}
            </div>
          </div>
        );
      })}
    </div>
  );
}
