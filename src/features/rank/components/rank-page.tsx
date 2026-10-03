import { useState } from 'react';
import { Flame, Loader2, Star, Tv } from 'lucide-react';
import { toast } from 'sonner';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Skeleton } from '@/components/ui/skeleton';
import { Tabs, TabsList, TabsTrigger } from '@/components/ui/tabs';
import {
  SeriesDetailSheet,
  type SeriesRef,
} from '@/features/series/components/series-detail-sheet';
import { isRenderableCover, useRank, useResolveSeries, useWebCover } from '@/lib/queries';
import { t } from '@/i18n';
import type { RankItem, RankKind } from '@/lib/schema';

/**
 * 排行榜页：官方 8 个榜单，tab 切换、榜单行列表。
 *
 * 榜单是有名次的序列，用「序号 + 行」而不是封面网格——名次本身就是
 * 用户要扫的第一信息（与 hgplayer 同形态）。点行 = 先解析再开详情抽屉，
 * 与首页信息流同一交互。
 */

const RANK_TABS: { kind: RankKind; labelKey: string }[] = [
  { kind: 'recommend', labelKey: 'rank.kind.recommend' },
  { kind: 'hot_play', labelKey: 'rank.kind.hotPlay' },
  { kind: 'prestige', labelKey: 'rank.kind.prestige' },
  { kind: 'subscribe', labelKey: 'rank.kind.subscribe' },
  { kind: 'new_drama', labelKey: 'rank.kind.newDrama' },
  { kind: 'hot_search', labelKey: 'rank.kind.hotSearch' },
  { kind: 'must_watch', labelKey: 'rank.kind.mustWatch' },
  { kind: 'followed', labelKey: 'rank.kind.followed' },
];

export function RankPage() {
  const [kind, setKind] = useState<RankKind>('recommend');
  const { data, isLoading, error, isFetching, refetch } = useRank(kind);
  const { mutate: resolve, isPending: resolving } = useResolveSeries();
  const [detail, setDetail] = useState<{ card: SeriesRef; selected: number[] } | null>(null);

  const handleSelect = (item: RankItem) => {
    resolve(item.seriesId, {
      onSuccess: (series) =>
        setDetail({
          card: {
            seriesId: series.seriesId,
            seriesTitle: series.title,
            cover: series.cover || item.cover,
            episodeCount: series.episodeCount || item.episodeCnt,
            tags: series.tags.length > 0 ? series.tags : item.tags,
          },
          selected: [],
        }),
      onError: (e) =>
        toast.error(t('common.resolveFailed'), { description: String(e.message ?? e) }),
    });
  };

  return (
    <div className="flex flex-col gap-4">
      <Tabs value={kind} onValueChange={(v) => setKind(v as RankKind)}>
        {/* 8 个榜单横排，窄窗口下允许横向滚动（不换行挤压文字） */}
        <TabsList className="flex-wrap">
          {RANK_TABS.map(({ kind: k, labelKey }) => (
            <TabsTrigger key={k} value={k}>
              {t(labelKey)}
            </TabsTrigger>
          ))}
        </TabsList>
      </Tabs>

      {isLoading ? (
        <div className="flex flex-col gap-3">
          {Array.from({ length: 8 }, (_, i) => (
            <Skeleton key={i} className="h-24 rounded-xl" />
          ))}
        </div>
      ) : error ? (
        <div className="text-muted-foreground flex flex-col items-center gap-3 py-16">
          <p>{t('rank.loadFailed')}</p>
          <p className="text-destructive text-xs">{error.message}</p>
          <Button variant="outline" size="sm" onClick={() => void refetch()}>
            <RefreshButton />
            {t('feed.retry')}
          </Button>
        </div>
      ) : (
        <div className="flex flex-col gap-2">
          {(data?.items ?? []).map((item) => (
            <RankRow key={item.seriesId} item={item} onSelect={handleSelect} />
          ))}
          {(data?.items.length ?? 0) === 0 && (
            <p className="text-muted-foreground py-16 text-center text-sm">{t('rank.empty')}</p>
          )}
        </div>
      )}

      {isFetching && !isLoading && (
        <p className="text-muted-foreground flex items-center justify-center gap-2 py-2 text-sm">
          <Loader2 className="size-4 animate-spin" aria-hidden />
          {t('feed.loadingMore')}
        </p>
      )}

      <SeriesDetailSheet
        card={detail?.card ?? null}
        selected={detail?.selected ?? []}
        onSelectedChange={(next) => setDetail((d) => (d ? { ...d, selected: next } : d))}
        onOpenChange={(open) => !open && setDetail(null)}
      />

      {resolving && <ResolvingHint />}
    </div>
  );
}

/** 榜单一行：名次 | 封面 | 标题/副标题/简介 | 热度文案。 */
function RankRow({ item, onSelect }: { item: RankItem; onSelect: (item: RankItem) => void }) {
  const { data: webCover } = useWebCover(item.seriesId, item.cover);
  const sourceRenderable = isRenderableCover(item.cover);
  const cover = webCover ?? (sourceRenderable ? item.cover : '');
  const [brokenFor, setBrokenFor] = useState('');
  const imgBroken = brokenFor !== '' && brokenFor === cover;
  const showImg = cover !== '' && !imgBroken;

  const rankNo = item.rank > 0 ? item.rank : undefined;
  const heat = item.recText !== '' ? item.recText : (item.secondaryInfos[0] ?? '');

  return (
    <article
      role="button"
      tabIndex={0}
      aria-label={item.title}
      onClick={() => onSelect(item)}
      onKeyDown={(e) => {
        if (e.key !== 'Enter' && e.key !== ' ') return;
        e.preventDefault();
        onSelect(item);
      }}
      className="bg-card hover:border-foreground/30 focus-visible:border-foreground/30 flex cursor-pointer items-center gap-4 rounded-xl border p-3 text-left transition-colors hover:shadow-md focus-visible:outline-none"
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

      <div className="bg-muted relative aspect-[3/4] w-16 shrink-0 overflow-hidden rounded-lg">
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
        <div className="flex items-center gap-2">
          <p className="truncate text-sm font-semibold" title={item.title}>
            {item.title}
          </p>
          {item.score > 0 && (
            <span className="text-muted-foreground flex shrink-0 items-center gap-0.5 text-xs">
              <Star className="size-3 text-amber-400" aria-hidden />
              {item.score.toFixed(1)}
            </span>
          )}
        </div>
        {item.subTitle !== '' && (
          <p className="text-muted-foreground truncate text-xs">{item.subTitle}</p>
        )}
        {item.description !== '' && (
          <p className="text-muted-foreground/80 line-clamp-2 text-xs leading-relaxed">
            {item.description}
          </p>
        )}
      </div>

      {heat !== '' && (
        <Badge variant="secondary" className="shrink-0 gap-1">
          <Flame className="size-3 text-orange-400" aria-hidden />
          {heat}
        </Badge>
      )}
    </article>
  );
}

function RefreshButton() {
  return <Loader2 className="mr-1 size-4 animate-spin" aria-hidden />;
}

function ResolvingHint() {
  return (
    <p className="text-muted-foreground bg-card fixed bottom-4 left-1/2 flex -translate-x-1/2 items-center gap-2 rounded-full border px-4 py-2 text-sm shadow-lg">
      <Loader2 className="size-4 animate-spin" aria-hidden />
      {t('browse.resolving')}
    </p>
  );
}
