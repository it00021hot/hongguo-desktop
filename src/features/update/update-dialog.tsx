import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog';
import { t, tf } from '@/locales';
import { useUpdate } from './update-context';

/** 全局更新弹层：发现新版先亮出更新内容，用户点「下载更新」才开始下载
 *  （只下载不安装，关掉弹层不中断）；下载完成后点「立即更新」才安装重启。
 *  重开入口是设置卡的「查看新版本」。 */
export function UpdateDialog() {
  const {
    available,
    phase,
    progress,
    dialogOpen,
    setDialogOpen,
    startDownload,
    installUpdate,
    dismiss,
  } = useUpdate();
  if (!available) return null;

  // 主按钮状态机；downloading 原地禁用（下载已由 startDownload 持有，
  // 不需要点击做任何事）；error 的重试回到重新下载
  let actionLabel: string;
  let onAction: () => void;
  let actionDisabled = false;
  switch (phase) {
    case 'downloading':
      actionLabel = tf('update.downloading', { progress });
      actionDisabled = true;
      onAction = () => {};
      break;
    case 'downloaded':
      actionLabel = t('update.installNow');
      onAction = installUpdate;
      break;
    case 'error':
      actionLabel = t('update.retry');
      onAction = startDownload;
      break;
    default:
      actionLabel = t('update.download');
      onAction = startDownload;
  }

  return (
    <AlertDialog
      open={dialogOpen}
      onOpenChange={(open) => {
        if (open) setDialogOpen(true);
        else dismiss();
      }}
    >
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>
            {tf('update.dialogTitle', { version: available.version })}
          </AlertDialogTitle>
          <AlertDialogDescription asChild>
            <div className="max-h-48 overflow-y-auto whitespace-pre-wrap">
              {available.body?.trim() || t('update.noChangelog')}
            </div>
          </AlertDialogDescription>
        </AlertDialogHeader>
        {(phase === 'downloading' || phase === 'downloaded') && (
          <p className="text-muted-foreground text-sm">
            {phase === 'downloading' ? t('update.downloadingNote') : t('update.downloadedNote')}
          </p>
        )}
        <AlertDialogFooter>
          <AlertDialogCancel>{t('update.later')}</AlertDialogCancel>
          <AlertDialogAction onClick={onAction} disabled={actionDisabled}>
            {actionLabel}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
