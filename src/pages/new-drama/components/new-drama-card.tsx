/** 新剧卡：字段比信息流少（无 tags/commentCount），subTitle 直接展示。 */
import { useState } from 'react';
import { Flame, Star, Tv } from 'lucide-react';
import { Badge } from '@/components/ui/badge';
import { useWebCover } from '@/service/queries';
import { isRenderableCover } from '@/utils/cover';
import { t, tf } from '@/locales';
import type { RankItem } from '@/service/schema';

export function NewDramaCard({
  item,
  downloaded,
  onSelect,
}: {
  item: RankItem;
  downloaded: number;
  onSelect: (item: RankItem) => void;
}) {
  const { data: webCover } = useWebCover(item.cover);
  const sourceRenderable = isRenderableCover(item.cover);
  const cover = webCover ?? (sourceRenderable ? item.cover : '');
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
        {/* 官方同款封面角标：左下热度（黄）、右下总集数 */}
        {item.recText !== '' && (
          <span className="absolute bottom-1 left-1 flex items-center gap-0.5 text-[10px] font-semibold text-amber-300 drop-shadow-[0_1px_2px_rgba(0,0,0,0.9)]">
            <Flame className="size-3" aria-hidden />
            {item.recText}
          </span>
        )}
        {item.episodeCnt > 0 && (
          <span className="absolute right-1 bottom-1 rounded bg-black/60 px-1 py-0.5 text-[10px] text-white">
            {tf('newDrama.episodeTotal', { count: item.episodeCnt })}
          </span>
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
      </div>
      {/* 官方同款信息排布：标题 / 分类标签 / 分数 */}
      <div className="flex flex-col gap-1 p-2">
        <p className="truncate text-sm font-semibold" title={item.title}>
          {item.title}
        </p>
        <p className="text-muted-foreground truncate text-xs">
          {[...item.tags, item.subTitle !== '' ? item.subTitle.split('·')[0] : '']
            .filter((x) => x !== '')
            .slice(0, 4)
            .join('·')}
        </p>
        {item.score > 0 && (
          <p className="flex items-center gap-1 text-xs font-semibold">
            <Star className="size-3.5 text-amber-400" aria-hidden />
            {item.score.toFixed(1)}
            {t('newDrama.scoreSuffix')}
          </p>
        )}
      </div>
    </article>
  );
}
