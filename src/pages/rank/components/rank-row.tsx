/** 榜单行卡片：进详情为主动作，右侧按上线状态给播放/预约。 */
import { useState } from 'react';
import { useNavigate } from '@tanstack/react-router';
import { Bell, Check, Flame, Loader2, Play, Tv } from 'lucide-react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { useReserveSeries, useWebCover } from '@/service/queries';
import { isRenderableCover } from '@/utils/cover';
import { usePlaySeries } from '@/hooks/use-play-series';
import { t } from '@/locales';
import type { RankItem } from '@/service/schema';

/** 榜单一行：名次 | 封面 | 标题/副标题/简介 | 热度文案。 */
export function RankRow({ item }: { item: RankItem }) {
  const navigate = useNavigate();
  const playSeries = usePlaySeries();
  const { data: webCover } = useWebCover(item.cover);
  const sourceRenderable = isRenderableCover(item.cover);
  const cover = webCover ?? (sourceRenderable ? item.cover : '');
  const [brokenFor, setBrokenFor] = useState('');
  const imgBroken = brokenFor !== '' && brokenFor === cover;
  const showImg = cover !== '' && !imgBroken;
  // 预约态本地乐观翻转（详情页 ReserveButton 同款；榜单条目自带
  // online_subscribed 初始值，跨客户端操作过也能显示）
  const reserve = useReserveSeries();
  const [reserved, setReserved] = useState(item.reserved);

  // 卡片一律进详情（hgplayer 同款：详情看档案，播放/预约是右侧明确动作）。
  // 带上榜单自带的档案快照：未上线剧详情解析分集必然失败，prefill 支撑
  // 详情页渲染「即将上线」降级视图（而非整页报错）
  const openDetail = () =>
    void navigate({
      to: '/detail',
      search: {
        seriesId: item.seriesId,
        title: item.title,
        cover: item.cover,
        tags: item.tags.join(','),
        desc: item.description,
      },
    });
  const onToggleReserve = () => {
    const next = !reserved;
    reserve.mutate(
      { seriesId: item.seriesId, reserve: next },
      {
        onSuccess: () => {
          setReserved(next);
          toast.success(t(next ? 'player.interact.reserved' : 'player.interact.unreserved'));
        },
        onError: (err) => toast.error(String(err)),
      },
    );
  };

  const rankNo = item.rank > 0 ? item.rank : undefined;
  // 官方条目双信息：🔥主热词（recText，如 "995万推荐"）+ 次信息（"5490万热度"）
  const rec = item.recText;
  const secondary = item.secondaryInfos.filter((s) => s !== '' && s !== rec);
  // 元信息一行：副标题 · 评分 · 题材标签（hgplayer 同款行）
  const meta = [item.subTitle, item.score > 0 ? item.score.toFixed(1) : '', ...item.tags]
    .filter((s) => s !== '')
    .join(' · ');

  return (
    <article
      role="button"
      tabIndex={0}
      aria-label={item.title}
      onClick={openDetail}
      onKeyDown={(e) => {
        if (e.key !== 'Enter' && e.key !== ' ') return;
        e.preventDefault();
        openDetail();
      }}
      className="bg-card hover:border-foreground/30 focus-visible:border-foreground/30 flex cursor-pointer items-center gap-4 rounded-xl border p-3 text-left transition-colors [contain-intrinsic-size:auto_112px] [content-visibility:auto] hover:shadow-md focus-visible:outline-none"
    >
      {rankNo !== undefined && (
        <span
          className={`w-8 shrink-0 text-center text-2xl font-black tabular-nums ${
            rankNo <= 3 ? 'text-amber-500' : 'text-muted-foreground/50'
          }`}
          aria-hidden
        >
          {rankNo}
        </span>
      )}

      <div className="bg-muted relative aspect-[3/4] w-20 shrink-0 overflow-hidden rounded-lg">
        {showImg ? (
          <img
            src={cover}
            alt=""
            loading="lazy"
            className="size-full object-cover"
            onError={() => setBrokenFor(cover)}
          />
        ) : (
          <div className="text-muted-foreground grid size-full place-items-center">
            <Tv className="size-5" />
          </div>
        )}
      </div>

      <div className="flex min-w-0 flex-1 flex-col gap-1">
        <div className="flex items-center gap-1.5">
          <p className="truncate text-sm font-semibold" title={item.title}>
            {item.title}
          </p>
          {item.season !== '' && (
            <span className="text-muted-foreground shrink-0 rounded border px-1 text-[10px] leading-4">
              {item.season}
            </span>
          )}
        </div>
        {meta !== '' && <p className="text-muted-foreground truncate text-xs">{meta}</p>}
        {item.description !== '' && (
          <p className="text-muted-foreground/80 line-clamp-2 text-xs leading-relaxed">
            {item.description}
          </p>
        )}
        {(rec !== '' || secondary.length > 0) && (
          <p className="flex items-center gap-2 text-xs">
            {rec !== '' && (
              <span className="flex shrink-0 items-center gap-0.5 text-orange-400">
                <Flame className="size-3" aria-hidden />
                {rec}
              </span>
            )}
            {secondary.length > 0 && (
              <span className="text-muted-foreground truncate whitespace-nowrap">
                {secondary.join('  ')}
              </span>
            )}
          </p>
        )}
      </div>

      {/* 行尾动作列（按剧状态分）：未上线 = 预约/已预约；已上线 = 播放 +
          详情链接。按钮统一定位风格（与历史页「继续播放」同款 outline）。 */}
      <div className="flex shrink-0 flex-col items-stretch gap-1.5 self-center">
        {item.upcoming ? (
          <Button
            size="sm"
            variant="outline"
            className="shrink-0 gap-1"
            disabled={reserve.isPending}
            onClick={(e) => {
              e.stopPropagation();
              onToggleReserve();
            }}
          >
            {reserve.isPending ? (
              <Loader2 className="size-3.5 animate-spin" aria-hidden />
            ) : reserved ? (
              <Check className="size-3.5" aria-hidden />
            ) : (
              <Bell className="size-3.5" aria-hidden />
            )}
            {t(reserved ? 'player.reserved' : 'player.reserve')}
          </Button>
        ) : (
          <>
            <Button
              size="sm"
              variant="outline"
              className="shrink-0 gap-1"
              onClick={(e) => {
                e.stopPropagation();
                playSeries(item.seriesId);
              }}
            >
              <Play className="size-3.5" aria-hidden />
              {t('player.play')}
            </Button>
            <button
              type="button"
              className="text-muted-foreground hover:text-foreground cursor-pointer text-center text-xs transition-colors"
              onClick={(e) => {
                e.stopPropagation();
                openDetail();
              }}
            >
              {t('rank.detail')}
            </button>
          </>
        )}
      </div>
    </article>
  );
}
