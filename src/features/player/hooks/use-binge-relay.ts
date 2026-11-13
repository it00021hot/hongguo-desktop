import { useCallback, useEffect, useRef } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { toast } from 'sonner';
import { series } from '@/service/commands';
import { keys, useRelatedSeries } from '@/service/queries';
import { fuzzyStaleTime } from '@/service/queries/common';
import { usePlayerStore } from '@/stores/player';
import { t, tf } from '@/locales';
import type { Series } from '@/service/schema';
import { pickNextSeason, seasonFromTitle, seasonNoFromTag } from '../season';
import { firstEpisodeIndex, resolveEpisodeStep } from '../episode-step';

/** 分集和相关季独立到达，前进必须等就绪；过期请求不再提交播放目标。 */
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

  useRelatedSeries(seriesId ?? '');
  const queryClient = useQueryClient();
  const pendingStep = useRef<object | null>(null);
  useEffect(() => {
    return () => {
      pendingStep.current = null;
    };
  }, [seriesId, vidIndex]);

  const readArchive = useCallback(
    (id: string) => {
      if (currentSeries?.seriesId === id) return Promise.resolve(currentSeries);
      const queryKey = keys.seriesEpisodes(id);
      return queryClient.fetchQuery({
        queryKey,
        queryFn: () => series.episodes(id),
        staleTime: fuzzyStaleTime(queryKey),
        retry: false,
      });
    },
    [currentSeries, queryClient],
  );

  const advanceAfterSeriesEnd = useCallback(
    async (archive: Series, isCurrent: () => boolean) => {
      const queryKey = keys.relatedSeries(archive.seriesId);
      const related = await queryClient.fetchQuery({
        queryKey,
        queryFn: () => series.related(archive.seriesId),
        staleTime: fuzzyStaleTime(queryKey),
        retry: false,
      });
      if (!isCurrent()) return;
      const self = related.works.find((work) => work.seriesId === archive.seriesId);
      const hasSeasons = related.works.some((work) => seasonNoFromTag(work.tag) != null);
      if (
        hasSeasons &&
        self &&
        seasonNoFromTag(self.tag) == null &&
        seasonFromTitle(archive.title) == null
      ) {
        throw new Error(t('player.seasonUnavailable'));
      }
      const nextSeason = pickNextSeason(related.works, archive.seriesId, archive.title);
      const nextSeries = nextSeason ?? (!onWheelStep ? related.guess[0] : undefined);
      if (nextSeries) {
        const nextArchive = await readArchive(nextSeries.seriesId);
        if (!isCurrent()) return;
        const firstIndex = firstEpisodeIndex(nextArchive.episodes);
        if (firstIndex == null) throw new Error(t('player.episodeListUnavailable'));
        setSlideDir(1);
        if (nextSeason) {
          requestGoto(nextSeries.seriesId, firstIndex);
          setBinge(nextSeries.seriesId);
        } else {
          setTarget(nextSeries.seriesId, firstIndex);
        }
        toast.info(
          tf(nextSeason ? 'player.playNextSeason' : 'player.autoPlayRecommend', {
            title: nextSeries.title,
          }),
        );
        return;
      }
      if (onWheelStep) {
        setSlideDir(1);
        onWheelStep(1);
        return;
      }
      toast.info(tf('player.lastEpisode', { index: vidIndex ?? 0 }));
    },
    [
      queryClient,
      readArchive,
      onWheelStep,
      setSlideDir,
      requestGoto,
      setBinge,
      setTarget,
      vidIndex,
    ],
  );

  const stepEpisode = useCallback(
    function stepEpisode(delta: number) {
      if (!seriesId || !vidIndex || pendingStep.current) return;
      const target = usePlayerStore.getState();
      if (target.seriesId !== seriesId || target.vidIndex !== vidIndex) return;
      const request = {};
      pendingStep.current = request;
      const isCurrent = () => {
        const current = usePlayerStore.getState();
        return (
          pendingStep.current === request &&
          current.seriesId === seriesId &&
          current.vidIndex === vidIndex
        );
      };
      let stage: 'episodes' | 'season' = 'episodes';
      const advance = async () => {
        const archive = await readArchive(seriesId);
        if (!isCurrent()) return;
        const next = resolveEpisodeStep(archive.episodes, vidIndex, delta);
        if (next.kind === 'first') return;
        if (next.kind === 'unavailable' || next.kind === 'missing') {
          throw new Error(t('player.episodeListUnavailable'));
        }
        if (next.kind === 'last') {
          if (downloading) {
            toast.info(tf('player.lastEpisode', { index: vidIndex }));
            return;
          }
          stage = 'season';
          await advanceAfterSeriesEnd(archive, isCurrent);
          return;
        }
        setSlideDir(delta > 0 ? 1 : -1);
        setTarget(seriesId, next.vidIndex);
      };
      void advance()
        .catch((error: unknown) => {
          if (!isCurrent()) return;
          toast.error(
            t(stage === 'episodes' ? 'player.episodeListUnavailable' : 'player.seasonUnavailable'),
            {
              description: error instanceof Error ? error.message : undefined,
              action: { label: t('common.retry'), onClick: () => stepEpisode(delta) },
            },
          );
        })
        .finally(() => {
          if (pendingStep.current === request) pendingStep.current = null;
        });
    },
    [seriesId, vidIndex, readArchive, downloading, setTarget, advanceAfterSeriesEnd, setSlideDir],
  );

  return { inBinge, setBinge, stepEpisode };
}
