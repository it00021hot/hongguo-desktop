import { createContext, useContext } from 'react';
import type { Update } from '@tauri-apps/plugin-updater';

/** 状态机：available（发现新版，等用户决定）→ downloading（后台下载，界面
 *  可继续用）→ downloaded（下载完成，等用户点「立即更新」才安装）→ install
 *  重启进新版本；error 停在原地，重试会重新下载。 */
export type UpdatePhase =
  'idle' | 'checking' | 'available' | 'downloading' | 'downloaded' | 'error';

export interface UpdateContextValue {
  phase: UpdatePhase;
  /** 当前应用版本（getVersion()），加载完成前为空串 */
  version: string;
  /** 启动或手动检查之后是否真的查过一轮——驱动卡片「已是最新版本」副行 */
  checked: boolean;
  available: Update | null;
  /** 下载进度 0-100 */
  progress: number;
  dialogOpen: boolean;
  setDialogOpen: (open: boolean) => void;
  /** manual = 设置卡手动点按钮：无新版/失败要 toast；启动检查静默 */
  checkForUpdate: (manual: boolean) => Promise<void>;
  /** 用户点「下载更新」才触发：只下载不安装 */
  startDownload: () => void;
  /** 「稍后」：记下版本号，同版本的启动检查不再自动弹 */
  dismiss: () => void;
  /** downloaded 后「立即更新」：运行安装包并重启进新版本 */
  installUpdate: () => void;
}

export const UpdateContext = createContext<UpdateContextValue | null>(null);

export function useUpdate() {
  const ctx = useContext(UpdateContext);
  if (!ctx) throw new Error('useUpdate 必须在 UpdateProvider 内使用');
  return ctx;
}
