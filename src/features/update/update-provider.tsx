import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { check, type Update } from '@tauri-apps/plugin-updater';
import { getVersion } from '@tauri-apps/api/app';
import { relaunch } from '@tauri-apps/plugin-process';
import { toast } from 'sonner';
import { t, tf } from '@/locales';
import { UpdateContext, type UpdateContextValue, type UpdatePhase } from './update-context';

/** 「稍后」记到 localStorage：同版本启动检查不再自动弹（对齐 hgplayer） */
const DISMISSED_KEY = 'hongguo.update.dismissedVersion';

export function UpdateProvider({ children }: { children: React.ReactNode }) {
  const [phase, setPhase] = useState<UpdatePhase>('idle');
  const [version, setVersion] = useState('');
  const [checked, setChecked] = useState(false);
  const [available, setAvailable] = useState<Update | null>(null);
  const [progress, setProgress] = useState(0);
  const [dialogOpen, setDialogOpen] = useState(false);
  // 下载中组件可能重挂（关掉弹层再开），进度累计要跨渲染存活
  const receivedRef = useRef(0);
  const totalRef = useRef(0);

  /** 查询并落地共享状态。resolve 拿到的新 Update（null = 已是最新）；网络
   *  层错误原样抛给调用方。 */
  const runCheck = useCallback(async (): Promise<Update | null> => {
    const update = await check();
    setChecked(true);
    if (!update) {
      setAvailable(null);
      setPhase('idle');
      return null;
    }
    setAvailable(update);
    setPhase('available');
    return update;
  }, []);

  const checkForUpdate = useCallback(
    async (manual: boolean) => {
      setPhase('checking');
      try {
        const update = await runCheck();
        if (!update) {
          if (manual) toast.success(t('update.upToDate'));
          return;
        }
        // 手动检查总弹层；启动检查只对没被「稍后」过的版本弹
        const dismissed = localStorage.getItem(DISMISSED_KEY);
        if (manual || dismissed !== update.version) setDialogOpen(true);
      } catch (e) {
        setPhase('error');
        if (manual) toast.error(tf('update.checkFailed', { error: String(e) }));
      }
    },
    [runCheck],
  );

  // 只下载不安装：装不装、什么时候装，由用户在 downloaded 态点「立即更新」决定
  const startDownload = useCallback(() => {
    if (!available) return;
    setPhase('downloading');
    setProgress(0);
    receivedRef.current = 0;
    totalRef.current = 0;
    void available
      .download((event) => {
        switch (event.event) {
          case 'Started':
            totalRef.current = event.data.contentLength ?? 0;
            break;
          case 'Progress':
            receivedRef.current += event.data.chunkLength;
            if (totalRef.current > 0) {
              setProgress(
                Math.min(100, Math.round((receivedRef.current / totalRef.current) * 100)),
              );
            }
            break;
          case 'Finished':
            setProgress(100);
            break;
        }
      })
      .then(() => setPhase('downloaded'))
      .catch((e) => {
        setPhase('error');
        toast.error(tf('update.downloadFailed', { error: String(e) }));
      });
  }, [available]);

  const dismiss = useCallback(() => {
    if (available) localStorage.setItem(DISMISSED_KEY, available.version);
    setDialogOpen(false);
  }, [available]);

  // Windows 上 install() 拉起静默安装器后本进程随即退出，后面的 relaunch
  // 跑不到；macOS 换好二进制要靠 relaunch 重启——两端都兜住
  const installUpdate = useCallback(() => {
    if (!available) return;
    available
      .install()
      .then(() => relaunch())
      .catch((e) => {
        setPhase('error');
        toast.error(tf('update.installFailed', { error: String(e) }));
      });
  }, [available]);

  // 启动静默检查一次（失败保持安静——还没有 release 时 404 是常态）；
  // 版本号只用于卡片副行，取不到不挡功能。setState 都发生在网络回调里，
  // 不是 effect 同步连锁渲染，规则误报这里豁免。
  useEffect(() => {
    getVersion()
      .then((v) => setVersion(v))
      .catch(() => {});
    // eslint-disable-next-line react-hooks/set-state-in-effect -- 见上
    runCheck().catch(() => {});
  }, [runCheck]);

  const value = useMemo<UpdateContextValue>(
    () => ({
      phase,
      version,
      checked,
      available,
      progress,
      dialogOpen,
      setDialogOpen,
      checkForUpdate,
      startDownload,
      dismiss,
      installUpdate,
    }),
    [
      phase,
      version,
      checked,
      available,
      progress,
      dialogOpen,
      checkForUpdate,
      startDownload,
      dismiss,
      installUpdate,
    ],
  );

  return <UpdateContext.Provider value={value}>{children}</UpdateContext.Provider>;
}
