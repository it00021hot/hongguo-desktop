import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';

import { play, transcode } from '../commands';
import { keys } from './common';

/**
 * 在线起播返回完整档位列表，已下载的同集不会覆盖清晰度选择。
 * 未指定档位时按官方动态策略选档；响应里的 definition 是实际生效档。
 */
export function usePlay() {
  return useMutation({
    mutationFn: ({
      seriesId,
      vidIndex,
      definition,
      resetDefinition,
    }: {
      seriesId: string;
      vidIndex: number;
      definition?: number;
      resetDefinition?: boolean;
    }) => play.series(seriesId, vidIndex, '', true, definition, resetDefinition),
  });
}

export function useSavePosition() {
  return useMutation({
    // 播放中每 5 秒一次的后台保存，失败弹全局只会刷屏；换集/关闭仍有兜底
    meta: { silentOnError: true },
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

/** 重新探测 ffmpeg：装完 ffmpeg 当前进程环境变量不会更新，须丢弃后端缓存重探一次（免重启）。 */
export function useRedetectCapability() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: transcode.redetect,
    onSuccess: (data) => {
      qc.setQueryData(keys.capability, data);
    },
  });
}
