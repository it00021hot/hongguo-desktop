import { useState } from 'react';
import { Flame, MessageSquare, Star, Tv } from 'lucide-react';
import { Badge } from '@/components/ui/badge';
import { t, tf } from '@/i18n';
import { isRenderableCover, useWebCover } from '@/lib/queries';
import { formatPlayCount } from '@/lib/format';
import type { FeedItem } from '@/lib/schema';

interface Props {
  items: FeedItem[];
  /** 每部剧已下载的集数，用于卡片角标 */
  downloadedMap?: Record<string, number>;
  /** 热榜模式：显示名次与播放量 */
  ranked?: boolean;
  onSelect: (item: FeedItem) => void;
  trailing?: React.ReactNode;
}

/** 信息流卡片网格（首页推荐 / 热榜共用）。 */
export function FeedCardGrid({ items, downloadedMap, ranked, onSelect, trailing }: Props) {
  return (
    <div className="grid grid-cols-2 gap-4 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 2xl:grid-cols-8">
      {items.map((item, i) => (
        <FeedCard
          key={item.seriesId}
          item={item}
          rank={ranked ? i + 1 : undefined}
          downloaded={downloadedMap?.[item.seriesId] ?? 0}
          onSelect={onSelect}
        />
      ))}
      {trailing}
    </div>
  );
}

interface CardProps {
  item: FeedItem;
  rank?: number;
  downloaded: number;
  onSelect: (item: FeedItem) => void;
}

/** 单卡：封面经 webp 增强后渲染（HEIC 源在 WebView2 里是裂图）。 */
function FeedCard({ item, rank, downloaded, onSelect }: CardProps) {
  const { data: webCover } = useWebCover(item.seriesId, item.cover);
  // 源 HEIC 渲染不了且 webp 还在路上的过渡期，先不急着挂 <img>：
  // 挂了也必然 onError，闪一下 TV 图标再换图反而更晃眼
  const sourceRenderable = isRenderableCover(item.cover);
  const cover = webCover ?? (sourceRenderable ? item.cover : '');
  // 加载失败要受控且能自动复位：记住「失败时的那格 cover」，cover 换成
  // webp 后对不上号即视为未失败（webp 晚到也能正常换图）。
  const [brokenFor, setBrokenFor] = useState('');
  const imgBroken = brokenFor !== '' && brokenFor === cover;
  const showImg = cover !== '' && !imgBroken;
  return (
    <article
      role="button"
      tabIndex={0}
      aria-label={item.title}
      onClick={() => onSelect(item)}
      onKeyDown={(e) => {
        if (e.key !== 'Enter' && e.key !== ' ') return;
        // 空格默认会滚动页面，按钮不该有滚动副作用
        e.preventDefault();
        onSelect(item);
      }}
      className="group bg-card hover:border-foreground/30 focus-visible:border-foreground/30 flex w-full cursor-pointer flex-col overflow-hidden rounded-xl border text-left transition-colors hover:shadow-md focus-visible:outline-none"
    >
      <div className="bg-muted relative aspect-[3/4] w-full overflow-hidden">
        {showImg ? (
          <img
            src={cover}
            alt={item.title}
            loading="lazy"
            className="size-full object-cover"
            onError={() => setBrokenFor(cover)}
          />
        ) : (
          <div className="text-muted-foreground grid size-full place-items-center">
            <Tv className="size-8" />
          </div>
        )}

        {downloaded > 0 && (
          <Badge
            variant="success"
            className="absolute top-2 right-2 shadow-sm"
            title={tf('browse.downloadedBadge', { count: downloaded })}
          >
            {downloaded}
          </Badge>
        )}

        {rank !== undefined && (
          <span
            className="absolute top-2 left-2 grid size-6 place-items-center rounded bg-black/70 text-xs font-bold text-amber-400"
            aria-hidden
          >
            {rank}
          </span>
        )}

        {item.episodeCnt > 0 && (
          <span className="absolute bottom-2 left-2 rounded bg-black/70 px-1.5 py-0.5 text-xs text-white">
            {tf('common.episodeCount', { count: item.episodeCnt })}
          </span>
        )}
        {item.playCnt > 0 && (
          <span className="absolute right-2 bottom-2 flex items-center gap-0.5 rounded bg-black/70 px-1.5 py-0.5 text-xs text-white">
            <Flame className="size-3 text-orange-400" aria-hidden />
            {formatPlayCount(item.playCnt)}
          </span>
        )}
      </div>

      <div className="flex flex-col gap-1.5 p-3">
        {/* 热度行（hgplayer 1.1.6 同款：剧名上方显示热度文本，官方配火焰） */}
        {item.heatText && (
          <p className="text-muted-foreground flex items-center gap-1 text-xs">
            <Flame className="size-3 text-orange-400" aria-hidden />
            {item.heatText}
          </p>
        )}
        <p className="flex items-center gap-1.5 text-sm font-semibold">
          {/* 官方运营角标（新剧/爆剧/红果首发，hgplayer 1.1.6 同款剧名前标签） */}
          {item.badge && (
            <Badge variant="destructive" className="shrink-0 px-1.5 text-[10px]">
              {item.badge}
            </Badge>
          )}
          {/* 季角标（「第1季」，多季剧的身份提示） */}
          {item.seasonTag && (
            <Badge variant="secondary" className="shrink-0 px-1.5 text-[10px]">
              {item.seasonTag}
            </Badge>
          )}
          <span className="truncate" title={item.title}>
            {item.title}
          </span>
        </p>
        <div className="text-muted-foreground flex items-center gap-2 text-xs">
          {item.score > 0 && (
            <span className="flex items-center gap-0.5" title={t('feed.scoreTitle')}>
              <Star className="size-3 text-amber-400" aria-hidden />
              {item.score.toFixed(1)}
            </span>
          )}
          {item.commentCount > 0 && (
            <span className="flex items-center gap-0.5">
              <MessageSquare className="size-3" aria-hidden />
              {formatPlayCount(item.commentCount)}
            </span>
          )}
        </div>
        {item.tags.length > 0 && (
          <div className="flex flex-nowrap gap-1 overflow-hidden">
            {item.tags.slice(0, 3).map((tag) => (
              <Badge key={tag} variant="secondary" className="shrink-0 text-[10px]">
                {tag}
              </Badge>
            ))}
          </div>
        )}
      </div>
    </article>
  );
}
