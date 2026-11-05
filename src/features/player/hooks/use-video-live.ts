import { useState } from 'react';

/**
 * 当前流是否已出画 / 正在卡顿：都带流指纹（streamKey），
 * 信息流 PlayerView 复用不随切剧重建，读时校验换流即失效。
 */
export function useVideoLive(streamKey: string) {
  const [live, setLive] = useState<{ key: string; on: boolean }>({ key: '', on: false });
  const [stalled, setStalled] = useState<{ key: string; on: boolean }>({ key: '', on: false });
  const videoLive = live.key === streamKey && live.on;
  /** waiting 后时间轴恢复推进（timeupdate）或重新出画即视为不卡 */
  const videoStalled = stalled.key === streamKey && stalled.on;

  const markLive = () => setLive({ key: streamKey, on: true });
  const markStalled = (on: boolean) => setStalled({ key: streamKey, on });

  return { videoLive, videoStalled, markLive, markStalled };
}
