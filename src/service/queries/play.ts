import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';

import { capability, play, transcode } from '../commands';
import { keys } from './common';

/**
 * 起播：preferOnline 恒为 true（已下载的那集后端仍优先走本地文件，没下载的走在线流）。
 * `definition` 不传取平台最高档，传了没有该档时后端自动回退——响应里的 definition 是实际生效档，菜单以它为准。
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

/**
 * 本机 WebView2 能否解 HEVC：后端是廉价的 MFT 枚举 + 进程级缓存，会话内结论不变；
 * 装完扩展走安装命令的返回值刷新缓存，不靠 staleTime 过期。
 */
export function usePlaybackCapability() {
  return useQuery({
    queryKey: keys.playbackCapability,
    queryFn: capability.playbackCapability,
    staleTime: Infinity,
  });
}

/**
 * 按需下载并安装 HEVC 视频扩展。
 *
 * 成功后用返回值直接改写能力缓存——装完不必重启即可让引导卡消失并重试播放。
 */
export function useInstallHevcExtension() {
  const qc = useQueryClient();
  return useMutation({
    // 组件层有带安装失败原因的定制 toast，全局错误 toast 静默
    meta: { silentOnError: true },
    mutationFn: capability.installHevcExtension,
    onSuccess: (playable) => {
      qc.setQueryData(keys.playbackCapability, playable);
    },
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

/** 播放兼容兜底：把解不出来的一集转成 H.264；只在确认「有声音没画面」之后才调，提前转码等于白转。 */
export function useCompatPlayback() {
  return useMutation({ mutationFn: transcode.transcodeForPlayback });
}
