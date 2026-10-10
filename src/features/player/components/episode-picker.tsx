//! 选集浮层内容卡（对齐 hgplayer：「选集 | 系列剧」双 tab，从控制栏按钮向上弹）。
//!
//! 定位由父级负责（贴着控制栏「选集」按钮向上弹，弹幕设置面板同款），
//! 本组件只负责内容：选集 tab = 范围 tab + 8 列数字网格；系列剧 tab =
//! 同系列各季卡片（当前季高亮「播放中」，未上线季给预约）。
//! 不放假选简介/推荐。
//!
//! 根节点 onClick stopPropagation：浮层里的任何点击都不许落回舞台——
//! 舞台的单击裁决是播放/暂停切换，点选集把视频点停了就是这里漏的。

import { useEffect, useMemo, useState } from 'react';
import { BellRing, ChevronDown, Play } from 'lucide-react';
import { useNavigate } from '@tanstack/react-router';
import { toast } from 'sonner';
import { cn } from '@/lib/utils';
import { Badge } from '@/components/ui/badge';
import { SeriesCover } from '@/components/series-cover';
import { formatPlayCount } from '@/utils/format';
import {
  useDownloadTasks,
  useRelatedSeries,
  useReserveSeries,
  useReservations,
  useSeriesEpisodes,
} from '@/service/queries';
import { usePlayerStore } from '@/stores/player';
import { t, tf } from '@/locales';
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
  const [tab, setTab] = useState<'grid' | 'series'>('grid');
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

  // ---- 系列剧 tab（hgplayer 1.1.8 同款）：各季卡片，未上线季可预约 ----
  const { data: related } = useRelatedSeries(seriesId);
  const navigate = useNavigate();
  const setTarget = usePlayerStore((s) => s.setTarget);
  const { data: offlineReservations } = useReservations(false);
  const { mutate: reserve, isPending: reserving } = useReserveSeries();
  const seasons = useMemo(() => {
    const list = (related?.works ?? []).filter((w) => seasonNo(w.tag) != null);
    // plan/v 只回其他季，不回正在播的这部——hgplayer 的系列剧列表里
    // 当前季也在（红框 + 播放中），缺的从剧集档案合成一份补进头部。
    // 档案没有 tag/playCnt，卡片副标题留空即可，「播放中」就是它的说明。
    if (series && !list.some((w) => w.seriesId === series.seriesId)) {
      list.unshift({
        seriesId: series.seriesId,
        title: series.title,
        cover: series.cover,
        tag: '',
        score: 0,
        playCnt: 0,
        episodeCnt: series.episodeCount,
        videoDesc: '',
      });
    }
    list.sort((a, b) => (seasonNo(a.tag) ?? 0) - (seasonNo(b.tag) ?? 0));
    return list;
  }, [related?.works, series]);
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
    // 根节点拦 click：浮层内的点击（选集/切季/预约/收起）不冒泡成舞台的播放/暂停切换
    <div
      role="dialog"
      aria-label={t('player.episodes')}
      onClick={(e) => e.stopPropagation()}
      className="w-full scrollbar-thin overflow-y-auto rounded-xl bg-neutral-900/95 p-4 text-neutral-100 shadow-2xl backdrop-blur-sm"
    >
      {/* 头部：选集 | 系列剧 双 tab（hgplayer 同款）+ 全N集 + 收起 */}
      <div className="mb-3 flex items-center justify-between gap-2">
        <div className="flex items-end gap-4">
          {(
            [
              ['grid', t('player.episodes'), null],
              ['series', t('player.seriesList'), seasons.length > 0 ? seasons.length : null],
            ] as const
          ).map(([key, label, badge]) => (
            <button
              key={key}
              type="button"
              onClick={() => setTab(key)}
              className={cn(
                'relative cursor-pointer pb-1.5 text-sm font-semibold transition-colors',
                tab === key ? 'text-white' : 'text-neutral-400 hover:text-neutral-200',
              )}
            >
              {label}
              {badge != null && (
                // 字号/行高走 Badge 自带的 text-xs（12px/16px），居中机理与
                // 详情页标签完全一致；只收内边距，别再压字号
                <Badge className="ml-1 rounded-full bg-red-500 px-1.5 py-0">{badge}</Badge>
              )}
              {tab === key && (
                <span className="absolute inset-x-0 bottom-0 h-0.5 rounded-full bg-red-500" />
              )}
            </button>
          ))}
        </div>
        <div className="flex items-center gap-1.5">
          {tab === 'grid' && total > 0 && (
            <span className="text-xs text-neutral-400">
              {tf('player.totalEpisodes', { count: total })}
            </span>
          )}
          <button
            type="button"
            onClick={onClose}
            className="grid size-7 place-items-center rounded-md text-neutral-400 hover:bg-neutral-800 hover:text-white"
            aria-label={t('common.close')}
          >
            <ChevronDown className="size-4" />
          </button>
        </div>
      </div>

      {tab === 'grid' ? (
        <>
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
                    // 选中**不关**浮层（hgplayer 同款）：连选连跳，浮层常驻
                    // 鼠标下方，后续点击永远落在浮层内部——这是「选集穿透
                    // 成画面点击暂停」的结构性根治。关闭走 Esc/收起/点外部
                    onSelect(ep.vidIndex);
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
        </>
      ) : (
        /* 系列剧 tab：各季卡片（hgplayer 同款：封面 + 剧名 + 季·数据，右侧
           播放中/预约/播放）。plan 封面是 HEIC 签名 URL，必须走 SeriesCover。 */
        <div className="flex flex-col gap-2">
          {seasons.map((item) => {
            const current = item.seriesId === seriesId;
            const unreleased = item.episodeCnt === 0;
            const reserved = reservedIds.has(item.seriesId);
            return (
              <div
                key={item.seriesId}
                role={current || unreleased ? undefined : 'button'}
                tabIndex={current || unreleased ? undefined : 0}
                onClick={current || unreleased ? undefined : () => switchSeason(item)}
                onKeyDown={
                  current || unreleased
                    ? undefined
                    : (e) => {
                        if (e.key === 'Enter') switchSeason(item);
                      }
                }
                title={current || unreleased ? undefined : item.title}
                className={cn(
                  'flex items-center gap-2.5 rounded-lg border p-2.5 text-left',
                  current
                    ? 'border-red-500/80 bg-neutral-800/60'
                    : unreleased
                      ? 'border-transparent bg-neutral-800/40'
                      : 'cursor-pointer border-transparent bg-neutral-800/40 transition-colors hover:bg-neutral-800/80',
                )}
              >
                <div className="relative h-14 w-10 shrink-0 overflow-hidden rounded-md">
                  <SeriesCover cover={item.cover} alt={item.title} />
                </div>
                <div className="min-w-0 flex-1">
                  <p className="truncate text-sm font-medium">{item.title}</p>
                  <p className="mt-0.5 flex items-center gap-1.5 text-[11px] text-neutral-400">
                    {unreleased && (
                      // 与 tab 计数徽标同一套居中机理：行高>字号、不设固定高
                      <Badge className="rounded-sm bg-red-500/90 px-1 py-0 text-[10px] leading-4">
                        {t('player.comingSoonBadge')}
                      </Badge>
                    )}
                    <span className="truncate">
                      {item.tag}
                      {item.playCnt > 0 &&
                        ` · ${formatPlayCount(item.playCnt)}${t('detail.plays')}`}
                    </span>
                  </p>
                </div>
                {current ? (
                  <span className="shrink-0 text-xs font-medium text-red-500">
                    {t('player.nowPlaying')}
                  </span>
                ) : unreleased ? (
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
                    className={cn(
                      'inline-flex h-7 shrink-0 cursor-pointer items-center gap-1 rounded-full px-3 text-xs font-medium',
                      reserved
                        ? 'cursor-default bg-neutral-700 text-neutral-300'
                        : 'bg-red-500 text-white hover:bg-red-500/90 disabled:opacity-60',
                    )}
                  >
                    <BellRing className="size-3" />
                    {reserved ? t('player.reserved') : t('player.reserve')}
                  </button>
                ) : (
                  <span className="inline-flex h-7 shrink-0 items-center gap-1 rounded-full border border-neutral-600 px-3 text-xs text-neutral-300">
                    <Play className="size-3" />
                    {t('player.play')}
                  </span>
                )}
              </div>
            );
          })}
        </div>
      )}

      {hint && (
        <p className="mt-3 border-t border-neutral-800 pt-2.5 text-xs text-neutral-500">{hint}</p>
      )}
    </div>
  );
}
