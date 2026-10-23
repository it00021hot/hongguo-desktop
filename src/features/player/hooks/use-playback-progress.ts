import { useCallback, useEffect, useRef } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { keys, useSavePosition } from '@/service/queries';
import { watchHistory } from '@/service/commands';
import type { Series } from '@/service/schema';
import { FIRST_REPORT_MS, HistoryCadence } from './history-cadence';

/** 进度保存间隔（毫秒）。太频繁会写爆磁盘，太稀疏丢进度。 */
const SAVE_INTERVAL = 5_000;

/**
 * 播放进度持久化：5 秒节流落盘 + 观看历史上报心跳 + 卸载补写。
 *
 * `srcKeyRef`/`lastKnownRef` 由取流侧（use-playback-source）持有、这里只读：
 * persist 落盘前要凭 srcKeyRef 确认「这条流还属于当前这一集」，卸载补写
 * 读的 lastKnownRef 是 cleanup 里唯一还能拿到的位置真值（届时 <video> 已卸载）。
 *
 * 上报与落盘是两条独立的节拍，别再把它们合成一条：
 * - [`persist`]：5 秒节流写播放档案，force 时顺带补一条历史（暂停/播完）；
 * - [`report`]：timeupdate 驱动的历史上报心跳，节拍判定全在 [HistoryCadence]
 *   ——挂在落盘节流上会把首报量化到 5–10 秒，还会让 60 秒节拍永远不成立。
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
          // 上报落定即把历史查询标记为过期（`refetchType: 'none'`）：
          // - 历史页下次挂载必拉新行（staleTime 挡不住，切页回来不再停在上一集）；
          // - 不立刻重拉——首页的「你的内容」种子吃同一份缓存，播放中
          //   重排种子会把游标指到别的剧，正在看的视频被顶掉。
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

  /**
   * 历史上报心跳：由 timeupdate 的播放位置驱动（暂停即停、不计时；窗口最小化
   * 时媒体事件照常，不会像定时器那样被后台节流）。
   *
   * 节拍判定见 [HistoryCadence]：新剧 5 秒首报 / 同剧切集立即翻页 / 每 60 秒
   * 一拍 / 回跳立即补。
   */
  const report = useCallback(
    (time: number) => {
      // 流不是这一集的（信息流切剧、play 在途旧流还在播）：秒数不能串到新剧头上
      if (srcKeyRef.current !== episodeKey) return;
      if (!seriesId || !vidIndex || !currentVid) return;
      const video = videoRef.current;
      // 拖动进度条/重取流的 seek 途中 timeupdate 会在连跳的位置上连发：
      // 等 seek 落定再报，否则一次拖动就能打出十几条上报
      if (video?.seeking) return;
      const positionMs = Math.round(time * 1000);
      if (!cadence.current!.due({ episodeKey, seriesId, positionMs })) return;
      const total = video && Number.isFinite(video.duration) ? video.duration : 0;
      reportHistory(time, total, currentVid);
    },
    [seriesId, vidIndex, episodeKey, currentVid, reportHistory, srcKeyRef, videoRef],
  );

  /**
   * 记录播放位置。
   *
   * `force` 绕过节流：暂停、播完、离开页面这三种时刻之后不会再有下一次
   * timeupdate，被节流挡掉就等于这一段进度永久丢失——「看了 3 秒就切走」
   * 正好落在这 5 秒窗口里，回来又是 0。force 同时补一条历史：这些时刻是
   * 本集最后一次心跳，收尾位置必须落上去。
   */
  const persist = useCallback(
    (time: number, force = false) => {
      // 流不是这一集的（信息流切剧、play 在途旧流还在播）：秒数不能串到新剧头上
      if (srcKeyRef.current !== episodeKey) return;
      if (!seriesId || !vidIndex) return;
      const now = Date.now();
      if (!force && now - lastSaved.current < SAVE_INTERVAL) return;
      lastSaved.current = now;
      // 时长直接从元素读：媒体状态归自绘控件管，这里不再维护第二份，
      // 免得两处对不上。后端靠它判断「接近片尾就别续播」。
      const video = videoRef.current;
      const total = video && Number.isFinite(video.duration) ? video.duration : 0;
      lastKnownRef.current = { key: episodeKey, time, duration: total };
      savePosition({ seriesId, vidIndex, currentTime: time, duration: total });
      // 收尾上报仍守 5 秒建行门槛（快滑掠过不落噪点行），但本集已经报过
      // 就照报——同剧切集的立即翻页走的就是这条。
      const positionMs = Math.round(time * 1000);
      const eligible = positionMs >= FIRST_REPORT_MS || cadence.current!.hasReported(episodeKey);
      if (force && currentVid && eligible) {
        cadence.current!.note({ episodeKey, seriesId, positionMs });
        reportHistory(time, total, currentVid);
      }
    },
    // ref 形参（videoRef/lastKnownRef/srcKeyRef）按仓库惯例写进依赖：
    // ref 身份恒定，依赖变化永远由值参数驱动（同 incognito.ts）
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

  // 卸载 / 切集时补写最后一次。
  //
  // 只靠 timeupdate 的定时保存会丢掉最后一小段：用户看完直接点侧边栏
  // 回列表，组件当场卸载，那 5 秒内攒下的位置一次都没落过盘。
  // 这里读的是 lastKnownRef 而不是 videoRef —— cleanup 跑的时候 video 元素已经被卸载了。
  useEffect(() => {
    if (!seriesId || !vidIndex) return;
    return () => {
      const { key, time, duration } = lastKnownRef.current;
      // 从头就没播过（加载失败、秒退）不写：否则会给从未看过的集
      // 落一条 0 秒记录，把「继续观看」里凭空多出一张卡。
      // key 对不上说明这份位置属于别的集，写进去就是串集。
      if (key !== episodeKey || time <= 0) return;
      savePosition({ seriesId, vidIndex, currentTime: time, duration });
      // 切集/退出的最后一次也记一条历史（本地必记 + 登录态顺路上报）。
      // 与心跳同门槛：<5 秒的掠过不进历史（进度照常落盘）——否则首页
      // 快滑一路，历史页全是 1 秒的「未知剧集」行。
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
