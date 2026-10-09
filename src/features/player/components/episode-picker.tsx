//! 选集浮层内容卡（对齐 hgplayer：从控制栏按钮向上弹的紧凑数字网格）。
//!
//! 定位由父级负责（贴着控制栏「选集」按钮向上弹，弹幕设置面板同款），
//! 本组件只负责内容：系列季切换 + 范围 tab + 8 列数字网格。
//! 不放假选简介/封面/推荐。

import { useEffect, useMemo, useState } from 'react';
import { BellRing, ChevronDown } from 'lucide-react';
import { useNavigate } from '@tanstack/react-router';
import { toast } from 'sonner';
import { cn } from '@/lib/utils';
import {
  useDownloadTasks,
  useRelatedSeries,
  useReserveSeries,
  useReservations,
  useSeriesEpisodes,
} from '@/lib/queries';
import { usePlayerStore } from '@/lib/stores/player';
import { t, tf } from '@/i18n';
import type { RelatedItem } from '@/service/schema';

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

/** 同系列各季的判定：角标是「第N季」形态（plan/v 响应里同 IP 作品不带季角标）。 */
function seasonNo(tag: string): number | null {
  const m = /^第(\d{1,2})季$/.exec(tag);
  return m ? Number(m[1]) : null;
}

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

  // ---- 系列季（hgplayer 1.1.6 同款）：同系列其他季一键切换，未上线季可预约 ----
  const { data: related } = useRelatedSeries(seriesId);
  const navigate = useNavigate();
  const setTarget = usePlayerStore((s) => s.setTarget);
  const { data: offlineReservations } = useReservations(false);
  const { mutate: reserve, isPending: reserving } = useReserveSeries();
  const seasons = useMemo(() => {
    const list = (related?.works ?? []).filter((w) => seasonNo(w.tag) != null);
    list.sort((a, b) => (seasonNo(a.tag) ?? 0) - (seasonNo(b.tag) ?? 0));
    return list;
  }, [related?.works]);
  const reservedIds = useMemo(
    () => new Set((offlineReservations?.items ?? []).map((r) => r.seriesId)),
    [offlineReservations],
  );
  const switchSeason = (item: RelatedItem) => {
    // 与推荐列表同一套换剧机制：登记目标 → 播放页按需解析详情与分集
    setTarget(item.seriesId, 1);
    void navigate({ to: '/player' });
    onClose();
  };

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
      className="w-full scrollbar-thin overflow-y-auto rounded-xl bg-neutral-900/95 p-4 text-neutral-100 shadow-2xl backdrop-blur-sm"
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

      {/* 系列季横排：当前季高亮，其他季点击直达，未上线季只给预约 */}
      {seasons.length > 1 && (
        <div className="mb-3 flex flex-wrap items-center gap-1.5">
          {seasons.map((item) => {
            const current = item.seriesId === seriesId;
            const unreleased = item.episodeCnt === 0;
            const reserved = reservedIds.has(item.seriesId);
            if (current) {
              return (
                <span
                  key={item.seriesId}
                  className="rounded-full bg-red-500 px-2.5 py-0.5 text-xs font-semibold text-white"
                >
                  {item.tag}
                </span>
              );
            }
            if (unreleased) {
              return (
                <span
                  key={item.seriesId}
                  title={t('player.comingSoon')}
                  className="flex items-center gap-1 rounded-full border border-dashed border-neutral-700 px-2 py-0.5 text-xs text-neutral-500"
                >
                  {item.tag}
                  <button
                    type="button"
                    disabled={reserving || reserved}
                    onClick={(e) => {
                      e.stopPropagation();
                      reserve(
                        { seriesId: item.seriesId, reserve: !reserved },
                        {
                          onSuccess: () =>
                            toast.success(reserved ? t('player.unreserved') : t('player.reserved')),
                          onError: (err) => toast.error(String(err)),
                        },
                      );
                    }}
                    className="cursor-pointer text-amber-300 hover:text-amber-200 disabled:cursor-default disabled:text-neutral-600"
                  >
                    <BellRing className="size-3" />
                    {reserved ? t('player.reserved') : t('player.reserve')}
                  </button>
                </span>
              );
            }
            return (
              <button
                key={item.seriesId}
                type="button"
                onClick={() => switchSeason(item)}
                title={item.title}
                className="cursor-pointer rounded-full bg-neutral-800/80 px-2.5 py-0.5 text-xs text-neutral-300 transition-colors hover:bg-neutral-800 hover:text-white"
              >
                {item.tag}
              </button>
            );
          })}
        </div>
      )}

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
        <p className="mt-3 border-t border-neutral-800 pt-2.5 text-xs text-neutral-500">{hint}</p>
      )}
    </div>
  );
}
