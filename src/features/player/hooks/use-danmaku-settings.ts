import { useCallback, useState } from 'react';
import {
  readDanmakuDisplay,
  readDanmakuEnabled,
  writeDanmakuDisplay,
  writeDanmakuEnabled,
  type DanmakuDisplaySettings,
} from '@/utils/playback-prefs';

/**
 * 弹幕设置读写：总开关与显示参数（透明度/字号/速度/显示区域）。
 *
 * 偏好落在 localStorage（playback-prefs），state 是渲染镜像——
 * 写入即时生效，重启后由 useState 初始化器读回。
 */
export function useDanmakuSettings() {
  const [danmakuOn, setDanmakuOn] = useState(() => readDanmakuEnabled());
  const [danmakuDisplay, setDanmakuDisplay] = useState<DanmakuDisplaySettings>(() =>
    readDanmakuDisplay(),
  );
  const updateDanmakuDisplay = useCallback((patch: Partial<DanmakuDisplaySettings>) => {
    setDanmakuDisplay((prev) => {
      const next = { ...prev, ...patch };
      writeDanmakuDisplay(next);
      return next;
    });
  }, []);
  const toggleDanmaku = useCallback(() => {
    setDanmakuOn((on) => {
      writeDanmakuEnabled(!on);
      return !on;
    });
  }, []);
  return { danmakuOn, danmakuDisplay, updateDanmakuDisplay, toggleDanmaku };
}
