import { useCallback, useEffect, useState } from 'react';
import { app as appApi } from '@/lib/ipc/commands';
import { useEvent } from '@/lib/ipc/events';
import { EVENTS } from '@/lib/ipc/types';
import { readIncognito, writeIncognito } from '@/lib/playback-prefs';

/**
 * 隐身模式：开启后**鼠标脱离窗口 → 隐藏 + 暂停；鼠标回到窗口区域 →
 * 窗口原地自动重现**。
 *
 * 判定在后端做系统级光标轮询（`set_incognito`）：隐藏的窗口收不到任何
 * 鼠标事件，「回来了」只能靠全局光标坐标对上窗口矩形来判断——这也是
 * 桌面端实现这套语义的唯一路径。前端只做两件事：
 * - 开关时通知后端起停轮询；
 * - 收到 `incognito-visibility`（visible=false）时暂停视频——音频不能
 *   在窗口消失后继续响。
 *
 * 窗口重现时**不自动续播**：隐身触发那一刻已暂停，鼠标扫回来就突然出声
 * 比「回来再按一下播放」糟得多——恢复播放交给用户的一次点击/空格。
 *
 * 主播放器与小窗播放共用（偏好落在 localStorage；后端轮询只盯「活动的
 * 窗口」：有小窗是小窗，否则主窗）。
 */
export function useIncognitoMode(videoRef: React.RefObject<HTMLVideoElement | null>) {
  const [on, setOn] = useState(() => readIncognito());

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
        if (!p.visible) videoRef.current?.pause();
      },
      [videoRef],
    ),
  );

  return { on, toggle };
}
