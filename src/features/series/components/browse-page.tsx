import { useMemo, useState } from 'react';
import { ChevronLeft, ChevronRight, Search, X } from 'lucide-react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { Skeleton } from '@/components/ui/skeleton';
import { SeriesCardGrid } from './series-card-grid';
import { SeriesDetailSheet, type SeriesRef } from './series-detail-sheet';
import {
  useBrowseCategories,
  useBrowseList,
  useDownloadTasks,
  useResolveSeries,
  useSearch,
} from '@/lib/queries';
import { useUiStore } from '@/lib/stores/ui';
import { t, tf } from '@/i18n';
import type { Series, SeriesCard } from '@/lib/schema';

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

/**
 * 浏览页：顶部一个搜索框，下面是分类 + 题材分页浏览。
 *
 * 搜索与浏览共用同一块结果区（和官网一致）：提交关键词就原地切成搜索结果，
 * 清空或退出就回到分类列表，不再单独开一个搜索页。
 *
 * 搜索框同时是链接/ID 入口，删掉独立下载页后「粘贴分享链接」没有别的落点。
 */
export function BrowsePage() {
  // 分类与题材直接读 zustand：本地 useState 拷贝只在首次挂载时取一次初值，
  // 写成 state 就和 store 里的真值分家了。
  const category = useUiStore((s) => s.lastCategory);
  const genre = useUiStore((s) => s.lastGenre);
  const setFilter = useUiStore((s) => s.setBrowseFilter);
  const [page, setPage] = useState(1);
  const [detail, setDetail] = useState<{ card: SeriesRef; selected: number[] } | null>(null);

  const [keyword, setKeyword] = useState('');
  /** 已提交的搜索词：空串 = 浏览模式，非空 = 搜索模式 */
  const [submitted, setSubmitted] = useState('');
  const searching = submitted !== '';

  const { data: categories } = useBrowseCategories();
  const browse = useBrowseList(category, genre, page);
  const found = useSearch(submitted);
  const { data: tasks } = useDownloadTasks();
  const { mutate: resolve, isPending: resolving } = useResolveSeries();

  // 搜索模式下用搜索结果盖掉分类结果，退出搜索再换回来
  const cards = searching ? (found.data?.results ?? []) : (browse.data?.results ?? []);
  const pending = searching ? found.isPending : browse.isPending && !browse.data;
  const failed = searching ? found.isError : browse.isError;

  // 已下载集数：按剧聚合，供卡片角标使用
  const downloadedMap = useMemo(() => {
    const map: Record<string, number> = {};
    for (const task of tasks ?? []) {
      if (task.status !== 'completed') continue;
      map[task.seriesId] = (map[task.seriesId] ?? 0) + 1;
    }
    return map;
  }, [tasks]);

  // 分类与题材来自嗅探结果里的 meta
  const genres = browse.data?.meta.genres ?? [];
  const totalPages = browse.data?.meta.totalPages ?? 0;
  const total = searching ? cards.length : (browse.data?.meta.total ?? 0);

  const exitSearch = () => {
    setSubmitted('');
    setKeyword('');
  };

  const handleCategory = (slug: string) => {
    exitSearch();
    setPage(1);
    setFilter(slug, '');
  };

  const handleGenre = (slug: string) => {
    setPage(1);
    setFilter(category, slug === 'all' ? '' : slug);
  };

  // 链接/ID 直接解析并打开详情抽屉，抽屉内部会自己拉分集。
  // 解析前先退出搜索模式，否则解析成功后列表还停在上一轮搜索结果上。
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

  // 点卡片只打开详情抽屉：选集、立即播放、提交下载都在抽屉里做。
  // 这里再顺手跳转的话，抽屉会「刚打开就被路由切走」，用户连集数都来不及点。
  const handleSelect = (card: SeriesCard) => openDetail(card);

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
          <Button type="submit" disabled={!keyword.trim() || resolving}>
            <Search className="size-4" />
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

        {!searching && (
          <>
            <Select value={category} onValueChange={handleCategory}>
              <SelectTrigger className="w-32">
                <SelectValue placeholder={t('browse.category')} />
              </SelectTrigger>
              <SelectContent>
                {(categories ?? []).map((c) => (
                  <SelectItem key={c.slug} value={c.slug}>
                    {c.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>

            <Select value={genre || 'all'} onValueChange={handleGenre}>
              <SelectTrigger className="w-32">
                <SelectValue placeholder={t('browse.allGenres')} />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="all">{t('browse.allGenres')}</SelectItem>
                {genres.map((g) => (
                  <SelectItem key={g.slug} value={g.slug}>
                    {g.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </>
        )}

        <span className="text-muted-foreground ml-auto text-sm">
          {searching ? tf('search.resultCount', { total }) : tf('browse.totalCount', { total })}
        </span>
      </div>

      {failed && <p className="text-destructive text-sm">{t('browse.loadFailed')}</p>}

      {pending ? (
        <div className="grid grid-cols-2 gap-4 md:grid-cols-4 lg:grid-cols-5">
          {Array.from({ length: 10 }, (_, i) => (
            <Skeleton key={i} className="h-56" />
          ))}
        </div>
      ) : cards.length === 0 ? (
        <p className="text-muted-foreground py-16 text-center text-sm">
          {searching ? t('search.empty') : t('browse.empty')}
        </p>
      ) : (
        <SeriesCardGrid cards={cards} downloadedMap={downloadedMap} onSelect={handleSelect} />
      )}

      {!searching && totalPages > 1 && (
        <div className="flex items-center justify-center gap-3 py-2">
          <Button
            variant="outline"
            size="sm"
            disabled={page <= 1}
            onClick={() => setPage((p) => Math.max(1, p - 1))}
          >
            <ChevronLeft className="size-4" />
          </Button>
          <span className="text-sm tabular-nums">
            {browse.data?.meta.page ?? page} / {totalPages}
          </span>
          <Button
            variant="outline"
            size="sm"
            disabled={page >= totalPages}
            onClick={() => setPage((p) => Math.min(totalPages, p + 1))}
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
    </div>
  );
}
