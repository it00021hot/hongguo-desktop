//! 沉浸流选集浮层（对齐 hgplayer：视频中央的紧凑数字网格）。
//!
//! 与播放页右侧的 `SeriesPanel` 是两回事：这里**只有选集**——范围 tab +
//! 8 列数字网格，深色半透明卡片浮在画面中央；不放假选简介/封面/推荐，
//! 那些内容在沉浸流里只会挡画面。

import { useEffect, useMemo, useRef, useState } from 'react';
import { ChevronDown } from 'lucide-react';
import { cn } from '@/lib/utils';
import { useDownloadTasks, useSeriesEpisodes } from '@/lib/queries';
import { t, tf } from '@/i18n';

interface Props {
  seriesId: string;
  currentIndex: number;
  onSelect: (vidIndex: number) => void;
  onClose: () => void;
}

/** 每个范围段的集数（hgplayer 同款 30 个一段）。 */
const GROUP_SIZE = 30;

export function EpisodePicker({ seriesId, currentIndex, onSelect, onClose }: Props) {
  const { data: series } = useSeriesEpisodes(seriesId);
  const { data: tasks } = useDownloadTasks();
  const boxRef = useRef<HTMLDivElement>(null);
  /**
   * 手动翻到哪个范围段：记下当时的当前集——一旦当前集变了（连播/点选），
   * 覆盖作废，自动跟回当前集所在段。
   */
  const [groupOverride, setGroupOverride] = useState<{ group: number; at: number } | null>(null);

  const episodes = series?.episodes ?? [];
  const total = episodes.length;
  const groupCount = Math.max(1, Math.ceil(total / GROUP_SIZE));
  const followGroup = Math.min(
    groupCount - 1,
    Math.max(0, Math.floor((currentIndex - 1) / GROUP_SIZE)),
  );
  const activeGroup = groupOverride?.at === currentIndex ? groupOverride.group : followGroup;
  const slice = episodes.slice(activeGroup * GROUP_SIZE, (activeGroup + 1) * GROUP_SIZE);

  // 下载状态角标（已下载 ✓）：按当前剧过滤
  const doneIdx = useMemo(() => {
    const set = new Set<number>();
    for (const task of tasks ?? []) {
      if (task.seriesId === seriesId && task.status === 'completed') set.add(task.vidIndex);
    }
    return set;
  }, [tasks, seriesId]);

  // Esc 关闭；打开时焦点进容器以便键盘可用
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onClose]);

  return (
    // 透明遮罩：点击浮层外的任何地方都关闭（hgplayer 同款行为）
    <div className="absolute inset-0 z-30" onClick={onClose} aria-hidden>
      <div
        ref={boxRef}
        role="dialog"
        aria-label={t('player.episodes')}
        onClick={(e) => e.stopPropagation()}
        className="absolute top-1/2 left-1/2 w-[min(560px,92%)] max-h-[72%] -translate-x-1/2 -translate-y-1/2 overflow-y-auto scrollbar-thin rounded-xl bg-neutral-900/95 p-4 text-neutral-100 shadow-2xl backdrop-blur-sm"
      >
        {/* 头部：选集 · 全N集 · 收起 */}
        <div className="mb-2 flex items-center justify-between">
          <div className="flex items-baseline gap-2">
            <span className="text-sm font-semibold">{t('player.episodes')}</span>
            {total > 0 && (
              <span className="text-xs text-neutral-400">
                {tf('player.totalEpisodes', { count: total })}
              </span>
            )}
          </div>
          <button
            type="button"
            onClick={onClose}
            className="grid size-7 place-items-center rounded-md text-neutral-400 hover:bg-neutral-800 hover:text-white"
            aria-label={t('common.close')}
          >
            <ChevronDown className="size-4" />
          </button>
        </div>

        {/* 范围段 tab（1-30 / 31-60 / …） */}
        {groupCount > 1 && (
          <div className="mb-3 flex flex-wrap gap-x-4 gap-y-1">
            {Array.from({ length: groupCount }, (_, i) => (
              <button
                key={i}
                type="button"
                onClick={() => setGroupOverride({ group: i, at: currentIndex })}
                className={cn(
                  'text-[13px] tabular-nums transition-colors',
                  i === activeGroup
                    ? 'font-semibold text-white'
                    : 'text-neutral-400 hover:text-neutral-200',
                )}
              >
                {i * GROUP_SIZE + 1}-{Math.min((i + 1) * GROUP_SIZE, total)}
              </button>
            ))}
          </div>
        )}

        {/* 8 列数字网格 */}
        <div className="grid grid-cols-8 gap-2">
          {slice.map((ep) => {
            const active = ep.vidIndex === currentIndex;
            return (
              <button
                key={ep.vidIndex}
                type="button"
                onClick={() => {
                  onSelect(ep.vidIndex);
                  onClose();
                }}
                title={ep.title || tf('player.epShort', { index: ep.vidIndex })}
                className={cn(
                  'relative grid h-9 place-items-center rounded-md text-sm tabular-nums transition-colors',
                  active
                    ? 'bg-red-500 font-semibold text-white'
                    : 'bg-neutral-800/80 text-neutral-200 hover:bg-neutral-700',
                )}
              >
                {ep.vidIndex}
                {doneIdx.has(ep.vidIndex) && (
                  <span
                    className={cn(
                      'absolute top-1 right-1 size-1.5 rounded-full',
                      active ? 'bg-white/90' : 'bg-emerald-400',
                    )}
                  />
                )}
              </button>
            );
          })}
        </div>
      </div>
    </div>
  );
}
