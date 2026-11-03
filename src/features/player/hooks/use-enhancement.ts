import { useCallback, useState } from 'react';
import {
  readEnhancement,
  writeEnhancement,
  type EnhancementSettings,
} from '@/utils/playback-prefs';

/** 增强层运行状态：面板里的说明行与错误提示（文案已翻译）。 */
export interface EnhancementStatus {
  state: 'idle' | 'loading' | 'ready' | 'error';
  /** 空串 = 无说明；error 态对应「已退回原画」的理由。 */
  message: string;
}

/** 画质增强设置读写：偏好落 localStorage（playback-prefs），state 是渲染镜像（写入即时生效，重启由 useState 初始化器读回）。 */
export function useEnhancement() {
  const [settings, setSettings] = useState<EnhancementSettings>(() => readEnhancement());
  const [status, setStatus] = useState<EnhancementStatus>({ state: 'idle', message: '' });
  const update = useCallback((patch: Partial<EnhancementSettings>) => {
    setSettings((prev) => {
      const next = { ...prev, ...patch };
      writeEnhancement(next);
      return next;
    });
  }, []);
  return { settings, update, status, setStatus };
}
