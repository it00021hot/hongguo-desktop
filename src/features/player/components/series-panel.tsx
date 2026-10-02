import { useMemo, useState } from 'react';
import { useNavigate } from '@tanstack/react-router';
import { Check, Loader2 } from 'lucide-react';
import { Badge } from '@/components/ui/badge';
import { cn } from '@/lib/utils';
import { useDownloadTasks, useSeriesEpisodes, useSeriesExtras } from '@/lib/queries';
import { usePlayerStore } from '@/lib/stores/player';
import { t, tf } from '@/i18n';
import type { RecommendItem, Series } from '@/lib/schema';

interface Props {
  seriesId: string;
  currentIndex: number;
  onSelect: (vidIndex: number) => void;
}

/** 一页显示多少集（对应参考图里 1-15 / 16-30 这样的分段）。 */
const PAGE_SIZE = 15;

/**
 * 播放器右侧面板：当前剧信息 + 简介 + 选集 + 推荐短剧。
 *
 * 选集用分段翻页而不是一屏铺几百个数字——短剧动辄上百集，
 * 铺开既找不到当前集也看不出下载状态。
 */
export function SeriesPanel({ seriesId, currentIndex, onSelect }: Props) {
  const { data: tasks } = useDownloadTasks();
  const { data: extras } = useSeriesExtras(seriesId);
  // 剧集档案要从 useSeriesEpisodes 拿，不能扫 useSeriesList：
  // 那个接口走的是 visible_series()，过滤掉了用户从磁盘清理页移除的剧，
  // 于是这类剧的选集整段不渲染（`total > 0` 不成立），右侧只剩简介和推荐。
  // useSeriesEpisodes 直接读单部档案、且本地没有会回落解析，不受移除标记影响。
  const { data: series } = useSeriesEpisodes(seriesId);
  /**
   * 手动翻到哪一段。记下当时看的集号：一旦当前集变了（自动连播、点别的集），
   * 这个覆盖就作废，选集自动跟到当前集所在的那一段。
   */
  const [pageOverride, setPageOverride] = useState<{ page: number; at: number } | null>(null);

  const byIndex = useMemo(() => {
    const map: Record<number, { status?: string }> = {};
    for (const task of tasks ?? []) {
      if (task.seriesId === seriesId) map[task.vidIndex] = task;
    }
    return map;
  }, [tasks, seriesId]);

  const episodes = series?.episodes ?? [];
  const total = episodes.length;
  const pages = Math.max(1, Math.ceil(total / PAGE_SIZE));
  const followPage = Math.min(pages - 1, Math.max(0, Math.ceil(currentIndex / PAGE_SIZE) - 1));
  const currentPage = pageOverride?.at === currentIndex ? pageOverride.page : followPage;
  const slice = episodes.slice(currentPage * PAGE_SIZE, currentPage * PAGE_SIZE + PAGE_SIZE);

  return (
    <aside className="flex w-80 shrink-0 scrollbar-thin flex-col gap-4 overflow-y-auto pr-1">
      {series && <SeriesHeadline series={series} currentIndex={currentIndex} />}

      {extras?.intro && <Intro text={extras.intro} />}

      {total > 0 && (
        <section className="grid gap-2">
          <h3 className="text-sm font-semibold">{t('player.episodes')}</h3>

          {pages > 1 && (
            <div className="flex flex-wrap gap-1">
              {Array.from({ length: pages }, (_, i) => (
                <button
                  key={i}
                  type="button"
                  onClick={() => setPageOverride({ page: i, at: currentIndex })}
                  className={
                    i === currentPage
                      ? 'bg-primary text-primary-foreground rounded px-2 py-0.5 text-xs tabular-nums'
                      : 'text-muted-foreground hover:bg-accent rounded px-2 py-0.5 text-xs tabular-nums'
                  }
                >
                  {i * PAGE_SIZE + 1}-{Math.min((i + 1) * PAGE_SIZE, total)}
                </button>
              ))}
            </div>
          )}

          <div className="grid grid-cols-5 gap-1.5">
            {slice.map((ep) => {
              const task = byIndex[ep.vidIndex];
              const active = ep.vidIndex === currentIndex;
              return (
                <button
                  key={ep.vidIndex}
                  type="button"
                  onClick={() => onSelect(ep.vidIndex)}
                  title={ep.title || tf('player.epShort', { index: ep.vidIndex })}
                  className={[
                    'relative grid h-12 place-items-center rounded-md border text-sm tabular-nums transition-colors',
                    active
                      ? 'border-primary bg-primary/10 text-primary font-semibold'
                      : 'hover:bg-accent',
                  ].join(' ')}
                >
                  {ep.vidIndex}
                  {task?.status === 'completed' && (
                    <Check className="text-success absolute top-1 right-1 size-3" />
                  )}
                  {task?.status === 'running' && (
                    <Loader2 className="text-warning absolute top-1 right-1 size-3 animate-spin" />
                  )}
                </button>
              );
            })}
          </div>
        </section>
      )}

      <RecommendList items={extras?.recommendations ?? []} />
    </aside>
  );
}

/**
 * 剧情简介。
 *
 * 默认只给两行 + 「展开」——简介动辄两三百字，全展开会把选集和推荐整个
 * 顶到面板折叠线以下，看剧的人反而找不到「选集」在哪。
 */
function Intro({ text }: { text: string }) {
  const [open, setOpen] = useState(false);
  // 两行装不下才给展开按钮；短简介不该挂一个点不开的「展开」
  const expandable = text.length > 80;

  return (
    <section className="grid gap-1.5">
      <h3 className="text-sm font-semibold">{t('player.intro')}</h3>
      <p className={cn('text-muted-foreground text-xs leading-relaxed', !open && 'line-clamp-2')}>
        {text}
      </p>
      {expandable && (
        <button
          type="button"
          onClick={() => setOpen((v) => !v)}
          className="text-muted-foreground hover:text-foreground self-start text-xs underline-offset-2 hover:underline"
        >
          {open ? t('player.introCollapse') : t('player.introExpand')}
        </button>
      )}
    </section>
  );
}

function SeriesHeadline({ series, currentIndex }: { series: Series; currentIndex: number }) {
  return (
    <section className="flex gap-3">
      {series.cover && (
        <img
          src={series.cover}
          alt=""
          loading="lazy"
          className="h-20 w-15 shrink-0 rounded-md object-cover"
        />
      )}
      <div className="grid min-w-0 content-start gap-1.5">
        {/* 剧名用前景色不用 muted：官方是白色主标题，灰字会显得跟封面脱节 */}
        <p className="line-clamp-2 text-sm leading-snug font-semibold">{series.title}</p>
        <div className="flex flex-wrap items-center gap-1">
          <Badge variant="secondary" className="text-[10px]">
            {tf('player.epShort', { index: currentIndex })}
          </Badge>
          {/* 题材用实心 chip：outline 是透明底，贴在封面上像浮着的描边框 */}
          <Badge variant="secondary" className="text-[10px]">
            {tf('player.totalEpisodes', { count: series.episodeCount })}
          </Badge>
        </div>
        {series.tags.length > 0 && (
          <div className="flex flex-wrap gap-1">
            {series.tags.slice(0, 5).map((tag) => (
              <Badge key={tag} variant="secondary" className="text-[10px]">
                {tag}
              </Badge>
            ))}
          </div>
        )}
      </div>
    </section>
  );
}

function RecommendList({ items }: { items: RecommendItem[] }) {
  const navigate = useNavigate();
  const setTarget = usePlayerStore((s) => s.setTarget);

  if (items.length === 0) return null;

  return (
    <section className="grid gap-2">
      <h3 className="text-sm font-semibold">{t('player.recommend')}</h3>
      <div className="grid grid-cols-3 gap-2">
        {items.map((item) => (
          <button
            key={item.seriesId}
            type="button"
            onClick={() => {
              // 推荐的是另一部剧：先登记播放目标，详情与分集由播放页按需解析
              setTarget(item.seriesId, 1);
              void navigate({ to: '/player' });
            }}
            className="group grid cursor-pointer gap-1 text-left"
          >
            <span className="bg-muted relative block aspect-3/4 overflow-hidden rounded-md">
              {item.seriesCover && (
                <img
                  src={item.seriesCover}
                  alt=""
                  loading="lazy"
                  className="size-full object-cover"
                />
              )}
            </span>
            <span className="line-clamp-2 text-[11px] leading-tight font-medium">
              {item.seriesName}
            </span>
            <span className="text-muted-foreground text-[10px]">
              {tf('player.totalEpisodes', { count: item.episodeCount })}
            </span>
          </button>
        ))}
      </div>
    </section>
  );
}
