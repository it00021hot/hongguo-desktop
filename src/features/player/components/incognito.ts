import { useCallback, useEffect, useRef, useState } from 'react';
import { app as appApi } from '@/lib/ipc/commands';
import { useEvent } from '@/lib/ipc/events';
import { EVENTS } from '@/lib/ipc/types';
import { readIncognito, writeIncognito } from '@/lib/playback-prefs';

/**
 * 隐身模式：开启后**鼠标脱离窗口 → 隐藏 + 暂停；鼠标回到窗口区域 →
 * 窗口原地重现 + 续播**。
 *
 * 判定在后端做系统级光标轮询（`set_incognito`）：隐藏的窗口收不到任何
 * 鼠标事件，「回来了」只能靠全局光标坐标对上窗口矩形来判断——这也是
 * 桌面端实现这套语义的唯一路径。前端只做两件事：
 * - 开关时通知后端起停轮询（关掉时后端同样补发 visible:true）；
 * - 收到 `incognito-visibility`：隐藏时暂停、重现时续播。
 *
 * 续播**只续被隐身暂停的那一次**：隐藏瞬间视频本来就在播才记「欠一次
 * 播放」，回来时补上；用户自己按停的（隐藏前就是暂停态）回来仍停着，
 * 不会自作主张出声。关掉隐身开关走同一条 visible:true 事件。
 *
 * 主播放器与小窗播放共用（偏好落在 localStorage；后端轮询只盯「活动的
 * 窗口」：有小窗是小窗，否则主窗）。
 */
export function useIncognitoMode(videoRef: React.RefObject<HTMLVideoElement | null>) {
  const [on, setOn] = useState(() => readIncognito());
  /** 隐身欠下的那次播放：回来时续播、手动暂停的不欠。 */
  const owedPlay = useRef(false);

  const toggle = useCallback(() => {
    setOn((v) => {
      writeIncognito(!v);
      return !v;
    });
  }, []);

  useEffect(() => {
    void appApi.setIncognito(on).catch(() => undefined);
  }, [on]);

  useEvent<{ visible: boolean }>(
    EVENTS.incognitoVisibility,
    useCallback(
      (p: { visible: boolean }) => {
        const video = videoRef.current;
        if (!video) return;
        if (!p.visible) {
          // 只在「正在播」时欠：已经暂停着说明是用户自己按停的
          owedPlay.current = !video.paused;
          if (owedPlay.current) video.pause();
        } else if (owedPlay.current) {
          owedPlay.current = false;
          void video.play().catch(() => undefined);
        }
      },
      [videoRef],
    ),
  );

  return { on, toggle };
}
