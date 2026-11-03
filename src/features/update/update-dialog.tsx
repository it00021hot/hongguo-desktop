import {
  AlertDialog,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog';
import { Button } from '@/components/ui/button';
import { Progress } from '@/components/ui/progress';
import { t, tf } from '@/locales';
import { useUpdate } from './update-context';

/** 全局更新弹层：发现新版先亮更新内容，点「下载更新」开始下载（关掉弹层不中断），下载完「立即更新」安装重启。
 *  主按钮用普通 Button 而非 AlertDialogAction——Radix Action 一点就关弹层，进度条会跟着消失。 */
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

  const downloading = phase === 'downloading';

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
        {downloading && (
          <div className="flex flex-col gap-2">
            <Progress value={progress} />
            <p className="text-muted-foreground text-sm">{t('update.downloadingNote')}</p>
          </div>
        )}
        {phase === 'downloaded' && (
          <p className="text-muted-foreground text-sm">{t('update.downloadedNote')}</p>
        )}
        <AlertDialogFooter>
          <AlertDialogCancel>{t('update.later')}</AlertDialogCancel>
          <Button onClick={onAction} disabled={actionDisabled}>
            {actionLabel}
          </Button>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
