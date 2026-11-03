import { useCallback, useEffect, useRef } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { keys, useSavePosition } from '@/service/queries';
import { watchHistory } from '@/service/commands';
import type { Series } from '@/service/schema';
import { FIRST_REPORT_MS, HistoryCadence } from './history-cadence';

/** 进度保存间隔（毫秒）。太频繁会写爆磁盘，太稀疏丢进度。 */
const SAVE_INTERVAL = 5_000;

/**
 * 播放进度持久化：5 秒节流落盘 + 历史上报心跳 + 卸载补写；srcKeyRef/lastKnownRef 由取流侧持有、这里只读。
 * 上报与落盘是两条独立节拍：persist 管落盘与收尾上报，report 由 timeupdate 驱动、节拍判定在 HistoryCadence。
 */
export function usePlaybackProgress(params: {
  videoRef: React.RefObject<HTMLVideoElement | null>;
  /** store 里的目标未恢复前是 null——persist/补写内部自带空值护栏 */
  seriesId: string | null;
  vidIndex: number | null;
  episodeKey: string;
  srcKeyRef: React.RefObject<string>;
  lastKnownRef: React.RefObject<{ key: string; time: number; duration: number }>;
  currentVid: string;
  currentSeries?: Series;
}) {
  const {
    videoRef,
    seriesId,
    vidIndex,
    episodeKey,
    srcKeyRef,
    lastKnownRef,
    currentVid,
    currentSeries,
  } = params;
  const { mutate: savePosition } = useSavePosition();
  const queryClient = useQueryClient();
  const lastSaved = useRef(0);
  /** 上报节拍记账（一次播放生命周期一份，随组件卸载丢弃） */
  const cadence = useRef<HistoryCadence | null>(null);
  cadence.current ??= new HistoryCadence();

  /** 发一条观看历史（本地必记 + 登录态顺路上报云端，后端失败只记日志）。 */
  const reportHistory = useCallback(
    (time: number, total: number, vid: string) => {
      if (!seriesId || !vidIndex || !vid) return;
      void watchHistory
        .record({
          seriesId,
          vid,
          vidIndex,
          positionMs: Math.round(time * 1000),
          durationMs: Math.round(total * 1000),
          meta: {
            title: currentSeries?.title ?? '',
            cover: currentSeries?.cover ?? '',
            episodeCnt: currentSeries?.episodeCount ?? 0,
          },
        })
        .then(() => {
          // 上报落定把历史查询标记过期（refetchType: 'none'）：历史页下次挂载必拉新；
          // 不立刻重拉——首页种子吃同一份缓存，播放中重排会把正在看的视频顶掉
          void queryClient.invalidateQueries({
            queryKey: keys.watchHistory,
            refetchType: 'none',
          });
          void queryClient.invalidateQueries({
            queryKey: keys.localHistory,
            refetchType: 'none',
          });
        })
        .catch(() => {});
    },
    [seriesId, vidIndex, currentSeries, queryClient],
  );

  /** 历史上报心跳：timeupdate 播放位置驱动（暂停即停、窗口最小化不被后台节流）；节拍判定见 HistoryCadence。 */
  const report = useCallback(
    (time: number) => {
      // 流不是这一集的（信息流切剧、play 在途旧流还在播）：秒数不能串到新剧头上
      if (srcKeyRef.current !== episodeKey) return;
      if (!seriesId || !vidIndex || !currentVid) return;
      const video = videoRef.current;
      // seek 途中 timeupdate 会在连跳的位置上连发：等 seek 落定再报，否则一次拖动打出十几条上报
      if (video?.seeking) return;
      const positionMs = Math.round(time * 1000);
      if (!cadence.current!.due({ episodeKey, seriesId, positionMs })) return;
      const total = video && Number.isFinite(video.duration) ? video.duration : 0;
      reportHistory(time, total, currentVid);
    },
    [seriesId, vidIndex, episodeKey, currentVid, reportHistory, srcKeyRef, videoRef],
  );

  /**
   * 记录播放位置。force 绕过节流：暂停/播完/离开页面后不会再有 timeupdate，
   * 被节流挡掉等于这段进度永久丢失；force 同时补一条历史（本集最后一次心跳，收尾位置必须落上去）。
   */
  const persist = useCallback(
    (time: number, force = false) => {
      // 流不是这一集的（信息流切剧、play 在途旧流还在播）：秒数不能串到新剧头上
      if (srcKeyRef.current !== episodeKey) return;
      if (!seriesId || !vidIndex) return;
      const now = Date.now();
      if (!force && now - lastSaved.current < SAVE_INTERVAL) return;
      lastSaved.current = now;
      // 时长直接从元素读：不维护第二份免得两处对不上，后端靠它判断「接近片尾就别续播」
      const video = videoRef.current;
      const total = video && Number.isFinite(video.duration) ? video.duration : 0;
      lastKnownRef.current = { key: episodeKey, time, duration: total };
      savePosition({ seriesId, vidIndex, currentTime: time, duration: total });
      // 收尾上报仍守 5 秒建行门槛（快滑掠过不落噪点行），本集已报过就照报（同剧切集的立即翻页走这条）
      const positionMs = Math.round(time * 1000);
      const eligible = positionMs >= FIRST_REPORT_MS || cadence.current!.hasReported(episodeKey);
      if (force && currentVid && eligible) {
        cadence.current!.note({ episodeKey, seriesId, positionMs });
        reportHistory(time, total, currentVid);
      }
    },
    // ref 形参按仓库惯例写进依赖：ref 身份恒定，依赖变化永远由值参数驱动（同 incognito.ts）
    [
      seriesId,
      vidIndex,
      episodeKey,
      savePosition,
      currentVid,
      reportHistory,
      lastKnownRef,
      srcKeyRef,
      videoRef,
    ],
  );

  // 卸载 / 切集时补写最后一次：只靠 timeupdate 定时保存会丢最后一小段（看完直接退出，5 秒内攒的位置一次没落盘）；
  // 读 lastKnownRef 而不是 videoRef——cleanup 跑的时候 video 元素已被卸载
  useEffect(() => {
    if (!seriesId || !vidIndex) return;
    return () => {
      const { key, time, duration } = lastKnownRef.current;
      // 从头就没播过不写（否则给从未看过的集落 0 秒记录）；key 对不上 = 位置属于别的集，写进去就是串集
      if (key !== episodeKey || time <= 0) return;
      savePosition({ seriesId, vidIndex, currentTime: time, duration });
      // 最后一次也记历史，与心跳同门槛：<5 秒的掠过不进历史（否则首页快滑一路全是 1 秒的「未知剧集」行）
      const vid = currentSeries?.episodes.find((e) => e.vidIndex === vidIndex)?.vid ?? '';
      const positionMs = Math.round(time * 1000);
      const eligible = positionMs >= FIRST_REPORT_MS || cadence.current!.hasReported(episodeKey);
      if (vid && eligible) {
        cadence.current!.note({ episodeKey, seriesId, positionMs });
        reportHistory(time, duration, vid);
      }
    };
  }, [seriesId, vidIndex, episodeKey, savePosition, reportHistory, currentSeries, lastKnownRef]);

  return { persist, report };
}
