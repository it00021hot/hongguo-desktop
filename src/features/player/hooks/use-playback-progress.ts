import { useCallback, useEffect, useRef } from 'react';
import { useSavePosition } from '@/service/queries';
import { watchHistory } from '@/service/commands';
import type { Series } from '@/service/schema';

/** 进度保存间隔（毫秒）。太频繁会写爆磁盘，太稀疏丢进度。 */
const SAVE_INTERVAL = 5_000;

/**
 * 播放进度持久化：5 秒节流落盘 + 云端上报节拍 + 卸载补写。
 *
 * `srcKeyRef`/`lastKnownRef` 由取流侧（use-playback-source）持有、这里只读：
 * persist 落盘前要凭 srcKeyRef 确认「这条流还属于当前这一集」，卸载补写
 * 读的 lastKnownRef 是 cleanup 里唯一还能拿到的位置真值（届时 <video> 已卸载）。
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
  const lastSaved = useRef(0);
  /** 云端进度上报计数（配合 persist 的 5s 节流折算 ~1 分钟一次） */
  const cloudCounter = useRef(0);

  /**
   * 记录播放位置。
   *
   * `force` 绕过节流：暂停、播完、离开页面这三种时刻之后不会再有下一次
   * timeupdate，被节流挡掉就等于这一段进度永久丢失——「看了 3 秒就切走」
   * 正好落在这 5 秒窗口里，回来又是 0。
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
      // 云端进度上报（对齐参考端 v1.1.6 抓包 flows-20261009-v116-history.jsonl）：
      // 起播 5 秒首报（cloudCounter===1，本集第一次 persist 正好在 ~5s），
      // 之后每 ~60 秒一次（×12）；暂停/切集/卸载走 force。fire-and-forget
      // （后端匿名/失败都静默）。
      cloudCounter.current += 1;
      if (force || cloudCounter.current === 1 || cloudCounter.current % 12 === 0) {
        if (currentVid) {
          void watchHistory
            .reportProgress(seriesId, currentVid, vidIndex, Math.round(time * 1000))
            .catch(() => {});
        }
      }
    },
    // ref 形参（videoRef/lastKnownRef/srcKeyRef）按仓库惯例写进依赖：
    // ref 身份恒定，依赖变化永远由值参数驱动（同 incognito.ts）
    [seriesId, vidIndex, episodeKey, savePosition, currentVid, lastKnownRef, srcKeyRef, videoRef],
  );

  // 卸载 / 切集时补写最后一次。
  //
  // 只靠 timeupdate 的定时保存会丢掉最后一小段：用户看完直接点侧边栏
  // 回列表，组件当场卸载，那 5 秒内攒下的位置一次都没落过盘。
  // 这里读的是 lastKnownRef 而不是 videoRef —— cleanup 跑的时候 video 元素已经被卸载了。
  useEffect(() => {
    // 每集重置云端上报计数：5 秒首报的里程碑按集算，不清零的话
    // 切集后要等满 60 秒才有第一次上报（参考端切集后 5 秒即报）
    cloudCounter.current = 0;
    if (!seriesId || !vidIndex) return;
    return () => {
      const { key, time, duration } = lastKnownRef.current;
      // 从头就没播过（加载失败、秒退）不写：否则会给从未看过的集
      // 落一条 0 秒记录，把「继续观看」里凭空多出一张卡。
      // key 对不上说明这份位置属于别的集，写进去就是串集。
      if (key !== episodeKey || time <= 0) return;
      savePosition({ seriesId, vidIndex, currentTime: time, duration });
      // 切集/退出的最后一次位置也推一份云端（匿名/失败后端静默）
      const vid = currentSeries?.episodes.find((e) => e.vidIndex === vidIndex)?.vid ?? '';
      if (vid) {
        void watchHistory
          .reportProgress(seriesId, vid, vidIndex, Math.round(time * 1000))
          .catch(() => {});
      }
    };
  }, [seriesId, vidIndex, episodeKey, savePosition, currentSeries, lastKnownRef]);

  return { persist };
}
