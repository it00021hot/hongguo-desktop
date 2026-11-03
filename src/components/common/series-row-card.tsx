import { useState } from 'react';
import { Tv } from 'lucide-react';
import { enhancedCover, isRenderableCover } from '@/utils/cover';

/**
 * 剧集列表行卡骨架（排行榜行 / 上新日历行共用）：卡片整体进详情，播放/预约/详情是行尾动作列的明确动作。
 * 各页差异全走插槽：leading / titleExtra / metaLine / description / heatLine / trailing / actions。
 */
export function SeriesRowCard({
  cover,
  title,
  leading,
  titleExtra,
  metaLine,
  description,
  heatLine,
  trailing,
  actions,
  onOpen,
}: {
  cover: string;
  title: string;
  leading?: React.ReactNode;
  titleExtra?: React.ReactNode;
  metaLine?: React.ReactNode;
  description?: string;
  heatLine?: React.ReactNode;
  trailing?: React.ReactNode;
  actions?: React.ReactNode;
  onOpen: () => void;
}) {
  const webCover = enhancedCover(cover);
  const sourceRenderable = isRenderableCover(cover);
  const resolved = webCover ?? (sourceRenderable ? cover : '');
  const [brokenFor, setBrokenFor] = useState('');
  const imgBroken = brokenFor !== '' && brokenFor === resolved;
  const showImg = resolved !== '' && !imgBroken;

  return (
    <article
      role="button"
      tabIndex={0}
      aria-label={title}
      onClick={onOpen}
      onKeyDown={(e) => {
        if (e.key !== 'Enter' && e.key !== ' ') return;
        e.preventDefault();
        onOpen();
      }}
      className="bg-card hover:border-foreground/30 focus-visible:border-foreground/30 flex cursor-pointer items-center gap-4 rounded-xl border p-3 text-left transition-colors [contain-intrinsic-size:auto_112px] [content-visibility:auto] hover:shadow-md focus-visible:outline-none"
    >
      {leading}

      <div className="bg-muted relative aspect-[3/4] w-20 shrink-0 overflow-hidden rounded-lg">
        {showImg ? (
          <img
            src={resolved}
            alt=""
            loading="lazy"
            className="size-full object-cover"
            onError={() => setBrokenFor(resolved)}
          />
        ) : (
          <div className="text-muted-foreground grid size-full place-items-center">
            <Tv className="size-5" />
          </div>
        )}
      </div>

      <div className="flex min-w-0 flex-1 flex-col gap-1">
        <div className="flex items-center gap-1.5">
          <p className="truncate text-sm font-semibold" title={title}>
            {title}
          </p>
          {titleExtra}
        </div>
        {metaLine}
        {description !== '' && (
          <p className="text-muted-foreground/80 line-clamp-2 text-xs leading-relaxed">
            {description}
          </p>
        )}
        {heatLine}
      </div>

      {(trailing !== undefined || actions !== undefined) && (
        <div className="flex shrink-0 flex-col items-stretch gap-1.5 self-center">
          {trailing}
          {actions}
        </div>
      )}
    </article>
  );
}
