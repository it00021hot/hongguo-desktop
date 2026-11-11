import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { toast } from 'sonner';
import { useDanmaku } from '@/service/queries';
import { tf } from '@/locales';

/**
 * 弹幕滚窗：首窗起点 = metadata 就绪采样的播放位置；后续窗在逼近已拉边界时预取（领先 10s）。
 */
export function useDanmakuWindow(params: {
  videoRef: React.RefObject<HTMLVideoElement | null>;
  currentVid: string;
  seriesId: string | null;
  /** 本集时长（秒），喂给滚窗协议 */
  duration: number;
  /** metadata 回调之外还需叠加的副作用（竖屏判定等） */
  onLoadedMetadata?: (autoplay?: boolean) => void;
  episodeKey: string;
}) {
  const { videoRef, currentVid, seriesId, duration, onLoadedMetadata, episodeKey } = params;

  // 首窗起点：metadata 就绪前 null；换集即重置（渲染期重置是 React 官方模式）
  const [danmakuStartVid, setDanmakuStartVid] = useState('');
  const [danmakuStartSecs, setDanmakuStartSecs] = useState<number | null>(null);
  if (danmakuStartVid !== currentVid) {
    setDanmakuStartVid(currentVid);
    setDanmakuStartSecs(null);
  }

  /** 片源竖屏态带集指纹：切集后未出 metadata 前不算竖屏 */
  const [portraitByKey, setPortraitByKey] = useState<{ key: string; on: boolean }>({
    key: '',
    on: false,
  });
  const videoPortrait = portraitByKey.key === episodeKey && portraitByKey.on;

  const noteVideoSize = useCallback(
    (width: number, height: number) => {
      if (width > 0 && height > 0 && episodeKey) {
        setPortraitByKey({ key: episodeKey, on: height >= width });
      }
    },
    [episodeKey],
  );

  const handleLoadedMetadataWithDanmaku = useCallback(
    (autoplay = true) => {
      onLoadedMetadata?.(autoplay);
      const video = videoRef.current;
      if (video) noteVideoSize(video.videoWidth, video.videoHeight);
      setDanmakuStartSecs(Math.floor(video?.currentTime ?? 0));
    },
    [onLoadedMetadata, videoRef, noteVideoSize],
  );

  const danmakuQuery = useDanmaku(
    currentVid ? `${currentVid}:${seriesId}` : '',
    duration,
    danmakuStartSecs,
  );
  const danmakuItems = useMemo(
    () => danmakuQuery.data?.pages.flatMap((p) => p.items) ?? [],
    [danmakuQuery.data],
  );

  // 滚窗推进：依赖只收原始值——query 对象每次渲染都是新身份
  const danmakuNextStartMs = danmakuQuery.data?.pages.at(-1)?.nextStartMs ?? 0;
  const danmakuHasNext = danmakuQuery.hasNextPage;
  const danmakuFetchingNext = danmakuQuery.isFetchingNextPage;
  const danmakuQueryRef = useRef(danmakuQuery);
  useEffect(() => {
    danmakuQueryRef.current = danmakuQuery;
  });
  useEffect(() => {
    if (!danmakuHasNext || danmakuFetchingNext || !danmakuNextStartMs) return;
    const timer = setInterval(() => {
      const t = videoRef.current?.currentTime;
      if (t != null && t * 1000 + 10_000 >= danmakuNextStartMs) {
        void danmakuQueryRef.current.fetchNextPage();
      }
    }, 1000);
    return () => clearInterval(timer);
  }, [danmakuHasNext, danmakuFetchingNext, danmakuNextStartMs, videoRef]);

  // 弹幕拉取失败不能静默：画面照常播，但用户该知道弹幕为什么没了
  useEffect(() => {
    if (danmakuQuery.isError) {
      toast.error(tf('player.danmakuFailed', { reason: String(danmakuQuery.error) }));
    }
  }, [danmakuQuery.isError, danmakuQuery.error]);

  return {
    danmakuItems,
    handleLoadedMetadataWithDanmaku,
    noteVideoSize,
    videoPortrait,
  };
}
