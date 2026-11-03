import { useCallback, useEffect, useRef } from 'react';
import { toast } from 'sonner';
import { useRelatedSeries } from '@/service/queries';
import { usePlayerStore } from '@/stores/player';
import { tf } from '@/locales';
import type { Series } from '@/service/schema';
import { pickNextSeason } from '../season';

/**
 * 剧终接力与连播锁定：末集播完优先自动接下一季第 1 集（plan 相关作品里大于当前季的最小已上线季），
 * 没有下一季跟随宿主刷下一条推荐、无宿主上下文回落「猜你喜欢」。锁定后滚轮/↑↓ 切集不换剧。
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
  // 选中剧连播：锁定后滚轮/↑↓ 切集而不是跟随宿主页换剧
  const bingeSeriesId = usePlayerStore((s) => s.bingeSeriesId);
  const setBinge = usePlayerStore((s) => s.setBinge);
  // 外部点名（剧终接下一季）：首页消费信号对齐游标，不再逃去独立播放页
  const requestGoto = usePlayerStore((s) => s.requestGoto);
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
    // 下一季按季数挑（服务端顺序不可信会跳季），判不出才退回「列表下一条」并加未上线护栏
    const selfIdx = works.findIndex((w) => w.seriesId === seriesId);
    const legacyNext = selfIdx >= 0 ? works[selfIdx + 1] : undefined;
    const nextSeason =
      pickNextSeason(works, seriesId, currentSeries?.title ?? '') ??
      (legacyNext && legacyNext.episodeCnt > 0 ? legacyNext : undefined);
    if (nextSeason) {
      // toast 后直接开播第 1 集；requestGoto 让首页把游标对到下一季，锁连播延续切集语义
      toast.success(tf('player.playNextSeason', { title: nextSeason.title }));
      setSlideDir(1);
      requestGoto(nextSeason.seriesId, 1);
      setBinge(nextSeason.seriesId);
      return;
    }
    if (onWheelStep) {
      // 信息流没有下一季：跟随宿主刷下一条推荐；未接管成功，闸门保持开
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
  }, [
    seriesId,
    vidIndex,
    related,
    setTarget,
    requestGoto,
    setBinge,
    onWheelStep,
    currentSeries,
    setSlideDir,
  ]);

  const stepEpisode = useCallback(
    (delta: number) => {
      if (!seriesId || !vidIndex) return;
      const next = vidIndex + delta;
      if (next < 1) return;
      // 连播滚到尾部要有交代：走剧终接力（滚/点过末集也算剧终）；下载面板开着退回提示
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
