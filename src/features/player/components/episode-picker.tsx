//! 选集浮层内容卡（对齐 hgplayer：从控制栏按钮向上弹的紧凑数字网格）。
//!
//! 定位由父级负责（贴着控制栏「选集」按钮向上弹，弹幕设置面板同款），
//! 本组件只负责内容：范围 tab + 8 列数字网格。不放假选简介/封面/推荐。

import { useEffect, useMemo, useState } from 'react';
import { ChevronDown } from 'lucide-react';
import { cn } from '@/lib/utils';
import { useDownloadTasks, useSeriesEpisodes } from '@/lib/queries';
import { t, tf } from '@/i18n';

interface Props {
  seriesId: string;
  currentIndex: number;
  /** 底部提示文案（信息流里教「选一集 = 进入本剧连播」）；不传即无 */
  hint?: string;
  onSelect: (vidIndex: number) => void;
  onClose: () => void;
}

/** 每个范围段的集数（hgplayer 同款 30 个一段）。 */
const GROUP_SIZE = 30;

export function EpisodePicker({ seriesId, currentIndex, hint, onSelect, onClose }: Props) {
  const { data: series } = useSeriesEpisodes(seriesId);
  const { data: tasks } = useDownloadTasks();
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

  // 下载完成角标：按当前剧过滤
  const doneIdx = useMemo(() => {
    const set = new Set<number>();
    for (const task of tasks ?? []) {
      if (task.seriesId === seriesId && task.status === 'completed') set.add(task.vidIndex);
    }
    return set;
  }, [tasks, seriesId]);

  // Esc 关闭
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onClose]);

  return (
    <div
      role="dialog"
      aria-label={t('player.episodes')}
      className="w-full overflow-y-auto scrollbar-thin rounded-xl bg-neutral-900/95 p-4 text-neutral-100 shadow-2xl backdrop-blur-sm"
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

      {hint && (
        <p className="text-neutral-500 mt-3 border-t border-neutral-800 pt-2.5 text-xs">
          {hint}
        </p>
      )}
    </div>
  );
}
