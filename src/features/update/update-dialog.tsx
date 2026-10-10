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

/** 全局更新弹层（对齐 hgplayer UpdateLayer）：下载在后台走，关掉弹层不中断，
 *  重开入口是设置卡的「查看新版本」。 */
export function UpdateDialog() {
  const {
    available,
    phase,
    progress,
    dialogOpen,
    setDialogOpen,
    startDownload,
    restartToUpdate,
    dismiss,
  } = useUpdate();
  if (!available) return null;

  // 主按钮状态机；downloading 原地禁用（下载已由 startDownload 持有，
  // 不需要点击做任何事）
  let actionLabel: string;
  let onAction: () => void;
  let actionDisabled = false;
  switch (phase) {
    case 'downloading':
      actionLabel = tf('update.downloading', { progress });
      actionDisabled = true;
      onAction = () => {};
      break;
    case 'ready':
      actionLabel = t('update.ready');
      onAction = restartToUpdate;
      break;
    case 'error':
      actionLabel = t('update.retry');
      onAction = startDownload;
      break;
    default:
      actionLabel = t('update.downloadAndInstall');
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
        {(phase === 'downloading' || phase === 'ready') && (
          <p className="text-muted-foreground text-sm">
            {phase === 'downloading' ? t('update.downloadingNote') : t('update.readyNote')}
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
