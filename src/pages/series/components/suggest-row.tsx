/** 搜索联想下拉行：放大镜 + 命中高亮词条（+ 有剧集的行前置封面）。 */
import { Search } from 'lucide-react';
import { useWebCover } from '@/service/queries';
import { cn } from '@/lib/utils';
import type { SuggestItem } from '@/service/schema';

/**
 * 联想行，结构对齐 hgplayer：放大镜图标常驻（纯词行就只有它 + 词）；
 * 带剧集的行前置竖版封面（40×54，HEIC 走 webp/转码链）；词按服务端
 * 命中位切片、命中片段上高亮色（#ff7a1a ≈ orange-500）、rich 行加粗；
 * 摘要行有才渲染。
 */
export function SuggestRow({
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
