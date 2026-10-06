import { useEffect, useMemo, useRef, useState } from 'react';
import { ChevronLeft, ChevronRight, Loader2, Search, X } from 'lucide-react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { RefreshShade } from '@/components/refresh-shade';
import { ResolvingPill } from '@/components/resolving-pill';
import { SkeletonCardGrid } from '@/components/skeletons';
import { SeriesCardGrid } from './series-card-grid';
import { SeriesDetailSheet, type SeriesRef } from './series-detail-sheet';
import {
  useBrowsePage,
  useBrowsePanel,
  useDownloadTasks,
  useResolveSeries,
  useSeriesSearchApp,
} from '@/lib/queries';
import { t, tf } from '@/i18n';
import { cn } from '@/lib/utils';
import type { BrowseFilters, FeedItem, Series, SeriesCard } from '@/lib/schema';

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

/** 解析接口返回的是 Series，抽屉只吃 SeriesRef，这里做一次字段改名。 */
function toRef(series: Series): SeriesRef {
  return {
    seriesId: series.seriesId,
    seriesTitle: series.title,
    cover: series.cover,
    episodeCount: series.episodeCount,
    tags: series.tags,
  };
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
  const [page, setPage] = useState(1);
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
  const [detail, setDetail] = useState<{ card: SeriesRef; selected: number[] } | null>(null);

  const [keyword, setKeyword] = useState('');
  /** 已提交的搜索词：空串 = 浏览模式，非空 = 搜索模式 */
  const [submitted, setSubmitted] = useState('');
  const searching = submitted !== '';

  const panel = useBrowsePanel();
  const browse = useBrowsePage(filters, page);
  // 找剧搜索走官方 App API（站内官网搜索只匹配剧名且结果少；
  // App 搜索是综合 tab，首页精选 + 翻页全量）
  const found = useSeriesSearchApp(submitted);
  const { data: tasks } = useDownloadTasks();
  const { mutate: resolve, isPending: resolving } = useResolveSeries();

  // 搜索模式下用搜索结果盖掉分类结果，退出搜索再换回来。
  // App 搜索条目转成官网卡形态喂同一块网格：subTitle（"脑洞·全273集"）
  // 首段当题材 tag，url 无处消费填空串。
  const cards = useMemo(() => {
    if (!searching) return (browse.data?.items ?? []).map(toCard);
    return found.items.map((it) => ({
      seriesId: it.seriesId,
      seriesTitle: it.title,
      cover: it.cover,
      episodeCount: it.episodeCnt,
      tags: it.subTitle.split('·').slice(0, 1).filter(Boolean),
      url: '',
    }));
  }, [searching, browse.data, found.items]);
  // 换筛选/翻页时 queryKey 变了，placeholderData 把上一份结果留着。
  // 反馈分两层：真没数据（isPending）才整块骨架屏；
  // 手里有旧数据、后台在取新数据（isPlaceholderData && isFetching）时
  // 旧内容降透明度禁点——切换即时可感，又不闪白屏。
  const pending = searching ? found.isLoading : browse.isPending;
  const refreshing = searching
    ? found.isRefreshing
    : browse.isPlaceholderData && browse.isFetching;
  const failed = searching ? found.error !== null : browse.isError;
  // 提交按钮态：同一关键词还在搜索中就灰掉防连点；改了词不拦（允许直接重提）
  const resubmitting = searching && found.isLoading && keyword.trim() === submitted;

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

  const hasMore = browse.data?.hasMore ?? false;
  const total = searching ? cards.length : 0;

  // 退出搜索：置空提交词即可——浏览结果在 Query 缓存里秒回，
  // 搜索结果留在缓存，重复搜索同一关键词也秒出不再重拉
  const exitSearch = () => {
    setSubmitted('');
    setKeyword('');
  };

  const pick = (key: keyof BrowseFilters, value: string) => {
    setPage(1);
    setFilters((prev) => (prev[key] === value ? prev : { ...prev, [key]: value }));
  };

  // 链接/ID 直接解析并打开详情抽屉，抽屉内部会自己拉分集。
  // 解析前先退出搜索模式，否则解析成功后列表还停在上一轮搜索结果上。
  // 关键词搜索只改提交词：useSeriesSearchApp 随 queryKey 自动发起请求。
  const handleSubmit = () => {
    const value = keyword.trim();
    if (!value) return;
    setPage(1);
    if (detectInput(value) === 'keyword') {
      setSubmitted(value);
      return;
    }
    setSubmitted('');
    resolve(value, {
      onSuccess: (series) => openDetail(toRef(series)),
      onError: (e) => toast.error(e.message),
    });
  };

  // 打开抽屉的唯一入口。勾选状态和「当前是哪部剧」绑在同一个对象上，
  // 换剧时整体换掉、关闭时整体丢掉，两个场景共用这一处重置。
  const openDetail = (card: SeriesRef) => setDetail({ card, selected: [] });

  // 点卡片先解析、拿到分集再开抽屉（与首页信息流同一交互）：
  // 直接开抽屉的话，冷门剧的分集请求要几秒，用户面对的是一屏骨架
  // 不知道在等什么；先给底部气泡，抽屉一开就是完整内容。
  // 已解析过的剧在档案里直接命中，走这条路不增加可感知延迟。
  const handleSelect = (card: SeriesCard) => {
    resolve(card.seriesId, {
      onSuccess: (series) =>
        openDetail({
          seriesId: series.seriesId,
          seriesTitle: series.title,
          // 解析结果可能带换好的 webp 封面，卡片原封面兜底
          cover: series.cover || card.cover,
          episodeCount: series.episodeCount || card.episodeCount,
          tags: series.tags.length > 0 ? series.tags : card.tags,
        }),
      onError: (e) => toast.error(t('common.resolveFailed'), { description: e.message }),
    });
  };

  return (
    <div className="flex flex-col gap-4 p-6">
      <div className="flex flex-wrap items-center gap-2">
        <form
          className="flex min-w-64 flex-1 gap-2"
          onSubmit={(e) => {
            e.preventDefault();
            handleSubmit();
          }}
        >
          <Input
            value={keyword}
            onChange={(e) => setKeyword(e.target.value)}
            placeholder={t('search.placeholder')}
            aria-label={t('search.placeholder')}
            className="min-w-48 flex-1"
          />
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

      {/* 官方筛选面板（第三方同款八行）：选项表服务端下发，空选即「全部」。
          「长度」行部分设备不下发（2026-10-07 实测），但 select_items.duration
          服务端必认——缺行时用抓包锁定的选项 id 合成兜底。 */}
      {!searching && (
        <FilterPanel
          rows={withDurationFallback(panel.data ?? [])}
          filters={filters}
          loading={panel.isLoading}
          failed={panel.isError}
          onPick={pick}
        />
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
          <SeriesCardGrid
            cards={cards}
            downloadedMap={downloadedMap}
            onSelect={handleSelect}
            trailing={
              // 末行空位拿来放「下一页」，而不是留一个看起来像漏加载的白格子。
              // 搜索页没有分页（官网那边就不分），所以只在浏览模式出现。
              !searching && hasMore ? (
                <button
                  type="button"
                  onClick={() => setPage((p) => p + 1)}
                  className="text-muted-foreground hover:text-foreground hover:border-foreground/30 flex aspect-[3/4] w-full cursor-pointer flex-col items-center justify-center gap-2 rounded-xl border border-dashed text-sm transition-colors focus-visible:outline-none"
                >
                  <ChevronRight className="size-6" />
                  {t('browse.nextPage')}
                </button>
              ) : null
            }
          />
        </RefreshShade>
      )}

      {!searching && (page > 1 || hasMore) && (
        <div className="flex items-center justify-center gap-3 py-2">
          <Button
            variant="outline"
            size="sm"
            disabled={page <= 1}
            onClick={() => setPage((p) => Math.max(1, p - 1))}
          >
            <ChevronLeft className="size-4" />
          </Button>
          <span className="text-sm tabular-nums">{page}</span>
          <Button
            variant="outline"
            size="sm"
            disabled={!hasMore}
            onClick={() => setPage((p) => p + 1)}
          >
            <ChevronRight className="size-4" />
          </Button>
        </div>
      )}

      <SeriesDetailSheet
        card={detail?.card ?? null}
        selected={detail?.selected ?? []}
        onSelectedChange={(next) =>
          setDetail((prev) => (prev ? { ...prev, selected: next } : prev))
        }
        onOpenChange={(open) => !open && setDetail(null)}
      />

      {/* 与首页同一交互：解析期间底部气泡，抽屉一开就是完整内容 */}
      {resolving && <ResolvingPill />}
    </div>
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
        const fallbackLabel =
          FILTER_LABEL_KEYS[row.rowType] ?? row.rowName.replace(/^全部/, '');
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
