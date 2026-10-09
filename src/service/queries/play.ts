import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';

import { play, transcode } from '../commands';
import { keys } from './common';

// ---------------------------------------------------------------- 播放与转码

/**
 * 起播。
 *
 * preferOnline 恒为 true：已下载的那一集后端仍优先走本地文件，
 * 没下载的走在线流——与原版一致，否则「点开没下过的集」永远播不了。
 *
 * `definition` 不传则取平台给的最高档；传了但平台没有这一档时后端自动回退，
 * 响应里的 `definition` 是**实际生效**的那档，菜单以它为准。
 */
export function usePlay() {
  return useMutation({
    mutationFn: ({
      seriesId,
      vidIndex,
      definition,
    }: {
      seriesId: string;
      vidIndex: number;
      definition?: number;
    }) => play.series(seriesId, vidIndex, '', true, definition),
  });
}

export function useSavePosition() {
  return useMutation({
    mutationFn: ({
      seriesId,
      vidIndex,
      currentTime,
      duration,
    }: {
      seriesId: string;
      vidIndex: number;
      currentTime: number;
      duration: number;
    }) => play.savePosition(seriesId, vidIndex, currentTime, duration),
  });
}

export function useDecodeCapability() {
  return useQuery({
    queryKey: keys.capability,
    queryFn: transcode.capability,
    // 能力要跑 ffmpeg 试编一帧才能定，不随窗口聚焦重取
    staleTime: Infinity,
  });
}

/**
 * 重新探测 ffmpeg。
 *
 * 装完 ffmpeg 之后**当前进程的环境变量不会更新**，不重探就一直报「未检测到」。
 * 这条命令丢弃后端缓存重新探一次，比让用户去重启应用合理。
 */
export function useRedetectCapability() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: transcode.redetect,
    onSuccess: (data) => {
      qc.setQueryData(keys.capability, data);
    },
  });
}

/**
 * 播放兼容兜底：把解不出来的一集转成 H.264。
 *
 * 只有在确认「有声音没画面」之后才调——提前转码等于白转。
 */
export function useCompatPlayback() {
  return useMutation({ mutationFn: transcode.transcodeForPlayback });
}
