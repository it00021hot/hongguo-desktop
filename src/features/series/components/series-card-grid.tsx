import { Badge } from '@/components/ui/badge';
import { SeriesCover } from '@/components/series-cover';
import { tf } from '@/i18n';
import type { SeriesCard } from '@/service/schema';

interface Props {
  cards: SeriesCard[];
  /** 每部剧已下载的集数，用于卡片角标 */
  downloadedMap?: Record<string, number>;
  /** 选中这部剧：打开详情抽屉，而不是直接起播 */
  onSelect: (card: SeriesCard) => void;
  /**
   * 网格末尾追加的一块内容，用来补齐末行的空位。
   *
   * 官网分类页固定 24 条一页、搜索页固定 10 条，而列数是响应式的：24 能被
   * 2/3/4/6/8 整除但除不尽 5，10 正好相反。所以**没有任何一列能同时让两种
   * 页面排满**，调列数只会把缺口从这页挪到那页。末行留一格空白看着像「少加载了
   * 一张」，把这一格用起来（加载下一页）既补齐了视觉，也是真功能。
   */
  trailing?: React.ReactNode;
}

/** 剧集卡片网格。浏览页与搜索页共用。 */
export function SeriesCardGrid({ cards, downloadedMap, onSelect, trailing }: Props) {
  return (
    // 列数一路加到 2xl：屏幕越宽应该一行塞下更多剧，而不是把每张卡放大到
    // 一屏只能看三张。断点按「卡片保持 ~200px 宽」来定。
    <div className="grid grid-cols-2 gap-4 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 2xl:grid-cols-8">
      {cards.map((card) => {
        const downloaded = downloadedMap?.[card.seriesId] ?? 0;
        return (
          // 整张卡可点。不用 <button> 包：button 的内容模型只允许 phrasing content，
          // 而卡里有 <div> 和 <h3>，塞进去是非法嵌套。role="button" + tabIndex 是
          // 可点击卡片的标准做法，<h3> 也能留在 article 里保住标题语义。
          <article
            key={card.seriesId}
            role="button"
            tabIndex={0}
            aria-label={card.seriesTitle}
            onClick={() => onSelect(card)}
            onKeyDown={(e) => {
              if (e.key !== 'Enter' && e.key !== ' ') return;
              // 空格默认会滚动页面，按钮不该有滚动副作用
              e.preventDefault();
              onSelect(card);
            }}
            className="group bg-card hover:border-foreground/30 focus-visible:border-foreground/30 flex w-full cursor-pointer flex-col overflow-hidden rounded-xl border text-left transition-colors hover:shadow-md focus-visible:outline-none"
          >
            <div className="bg-muted relative aspect-[3/4] w-full overflow-hidden">
              {/* 封面统一走 SeriesCover：筛选流/搜索结果的封面是 HEIC 裸 URL，
                  直挂 <img> 在 Windows（无 HEVC 扩展的 WebView2）上全是裂图 */}
              <SeriesCover cover={card.cover} alt={card.seriesTitle} />

              {downloaded > 0 && (
                <Badge
                  variant="success"
                  className="absolute top-2 right-2 shadow-sm"
                  title={tf('browse.downloadedBadge', { count: downloaded })}
                >
                  {downloaded}
                </Badge>
              )}

              {card.episodeCount > 0 && (
                <span className="absolute bottom-2 left-2 rounded bg-black/70 px-1.5 py-0.5 text-xs text-white">
                  {tf('common.episodeCount', { count: card.episodeCount })}
                </span>
              )}
            </div>

            <div className="flex flex-col gap-1.5 p-3">
              {/* 单行截断而不是折两行：卡片高度一致才好排，折行会把标签顶得高低不齐 */}
              <p className="truncate text-sm font-semibold" title={card.seriesTitle}>
                {card.seriesTitle}
              </p>
              {card.tags.length > 0 && (
                // 单行不折行：标签一旦换行，这张卡就比旁边高，整排参差不齐。
                // 放不下就裁掉，悬停标题能看全。
                <div className="flex flex-nowrap gap-1 overflow-hidden">
                  {/* 题材用实心 chip：outline 透明底贴在白卡片上像三个浮着的空框 */}
                  {card.tags.slice(0, 3).map((tag) => (
                    <Badge key={tag} variant="secondary" className="shrink-0 text-[10px]">
                      {tag}
                    </Badge>
                  ))}
                </div>
              )}
            </div>
          </article>
        );
      })}
      {trailing}
    </div>
  );
}
