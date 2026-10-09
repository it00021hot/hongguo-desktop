import { useCallback, useEffect, useRef } from 'react';
import { useNavigate } from '@tanstack/react-router';
import { toast } from 'sonner';
import { useRelatedSeries } from '@/service/queries';
import { usePlayerStore } from '@/stores/player';
import { tf } from '@/locales';
import type { Series } from '@/service/schema';

/**
 * 剧终接力与连播锁定（hgplayer series-end 同款）。
 *
 * 本季最后一集播完（或连播中滚/点过末集）：优先自动接**下一季**第 1 集
 * ——「相关作品·系列」（官方 plan 接口）里当前剧的下一条，即 hgplayer
 * NextSeason 的数据面；没有下一季时：信息流上下文跟随宿主刷下一条推荐
 * （hgplayer K() 的 X(1) 同款），纯播放页回落「猜你喜欢」第一条。
 * 都落空才提示已是最后一集。起播时就拉好相关列表，剧终时通常已就绪。
 *
 * 连播锁定（bingeSeriesId）：信息流里主动选了某集 = 要追这部，锁定后
 * 滚轮/↑↓ 切集而不是跟随宿主页换剧。
 */
export function useBingeRelay(params: {
  seriesId: string | null;
  vidIndex: number | null;
  setTarget: (seriesId: string, vidIndex: number) => void;
  setSlideDir: (dir: 1 | -1) => void;
  /** 下载面板开着不接力（切剧会把面板连同勾选一起吃掉），退回提示 */
  downloading: boolean;
  /**
   * 滚轮/↑↓ 的语义由宿主页给：首页（沉浸流）是切**上一部/下一部剧**。
   * 有宿主上下文时剧终跟随宿主刷下一条推荐；没有则走猜你喜欢回落。
   */
  onWheelStep?: (dir: 1 | -1) => void;
  currentSeries?: Series;
}) {
  const { seriesId, vidIndex, setTarget, setSlideDir, downloading, onWheelStep, currentSeries } =
    params;
  const navigate = useNavigate();
  // 选中剧连播：锁定后滚轮/↑↓ 切集而不是跟随宿主页换剧
  const bingeSeriesId = usePlayerStore((s) => s.bingeSeriesId);
  const setBinge = usePlayerStore((s) => s.setBinge);
  const inBinge = bingeSeriesId != null && bingeSeriesId === seriesId;

  const { data: related } = useRelatedSeries(seriesId ?? '');
  /** 接力在途标记：setTarget 生效前后 ended 可能连发，防双跳 */
  const seriesEndBusy = useRef(false);
  // 接力完成后 seriesId 变化，重开闸门（信息流里 PlayerView 是复用的，
  // 不随切剧重挂载，ref 不会自己归零）
  useEffect(() => {
    seriesEndBusy.current = false;
  }, [seriesId]);

  const advanceAfterSeriesEnd = useCallback(() => {
    if (!seriesId || !vidIndex || seriesEndBusy.current) return;
    seriesEndBusy.current = true;
    const works = related?.works ?? [];
    const idx = works.findIndex((w) => w.seriesId === seriesId);
    const nextSeason = idx >= 0 ? works[idx + 1] : undefined;
    if (nextSeason) {
      // hgplayer 同款：toast「即将播放下一季」+ 直接开播第 1 集。信息流
      // 上下文顺势进纯播放页（第三方 push play 路由同款）——信息流宿主的
      // 游标/角标/滚轮语义都是按流条目算的，带进下一季只会错位
      toast.success(tf('player.playNextSeason', { title: nextSeason.title }));
      setSlideDir(1);
      setTarget(nextSeason.seriesId, 1);
      if (onWheelStep) void navigate({ to: '/player' });
      return;
    }
    if (onWheelStep) {
      // 信息流没有下一季：跟随宿主刷下一条推荐（hgplayer X(1) 同款；
      // 已到底时宿主原地驻留）。未接管成功，闸门保持开
      setSlideDir(1);
      onWheelStep(1);
      seriesEndBusy.current = false;
      return;
    }
    const guess = related?.guess ?? [];
    if (guess.length > 0) {
      toast.info(tf('player.autoPlayRecommend', { title: guess[0]?.title ?? '' }));
      setSlideDir(1);
      setTarget(guess[0]!.seriesId, 1);
      return;
    }
    seriesEndBusy.current = false;
    toast.info(tf('player.lastEpisode', { index: currentSeries?.episodes.length ?? vidIndex }));
  }, [seriesId, vidIndex, related, setTarget, onWheelStep, navigate, currentSeries, setSlideDir]);

  const stepEpisode = useCallback(
    (delta: number) => {
      if (!seriesId || !vidIndex) return;
      const next = vidIndex + delta;
      if (next < 1) return;
      // 连播模式下滚到尾部要有交代，静默不动像坏了。墙上的处置走剧终接力
      // （hgplayer Ns()→hc() series-end 同款：滚/点过末集也算剧终）；
      // 下载面板开着不接力（切剧会把面板连同勾选一起吃掉），退回提示。
      const total = currentSeries?.episodes.length ?? 0;
      if (total > 0 && next > total) {
        if (downloading) {
          toast.info(tf('player.lastEpisode', { index: total }));
          return;
        }
        advanceAfterSeriesEnd();
        return;
      }
      setSlideDir(delta > 0 ? 1 : -1);
      setTarget(seriesId, next);
    },
    [seriesId, vidIndex, currentSeries, downloading, setTarget, advanceAfterSeriesEnd, setSlideDir],
  );

  return { inBinge, setBinge, advanceAfterSeriesEnd, stepEpisode };
}
