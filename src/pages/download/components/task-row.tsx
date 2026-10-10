/** 下载任务表格行：状态徽标 / 进度 / 行内操作按钮（任务页表格用）。 */
import { Pause, RotateCcw, FolderOpen, Trash2 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import { Progress } from '@/components/ui/progress';
import { Checkbox } from '@/components/ui/checkbox';
import { TableRow, TableCell } from '@/components/ui/table';
import { formatBytes } from '@/utils/format';
import { t } from '@/locales';
import type { DownloadTask, TaskStatus } from '@/service/schema';

const STATUS_VARIANT: Record<
  TaskStatus,
  'secondary' | 'default' | 'success' | 'destructive' | 'warning'
> = {
  pending: 'secondary',
  running: 'default',
  completed: 'success',
  failed: 'destructive',
  stopped: 'warning',
};

interface RowProps {
  task: DownloadTask;
  checked: boolean;
  onToggle: () => void;
  onStop: () => void;
  onRetry: () => void;
  onOpenFolder: () => void;
  onDelete: (withFiles: boolean) => void;
}

export function TaskRow({
  task,
  checked,
  onToggle,
  onStop,
  onRetry,
  onOpenFolder,
  onDelete,
}: RowProps) {
  const percent = task.total > 0 ? (task.downloaded / task.total) * 100 : 0;

  return (
    <TableRow data-state={checked ? 'selected' : undefined}>
      <TableCell>
        <Checkbox checked={checked} onCheckedChange={onToggle} aria-label={task.id} />
      </TableCell>
      <TableCell className="max-w-64">
        <span className="block truncate" title={task.seriesTitle}>
          {task.seriesTitle}
        </span>
        {task.error && (
          // task.error 存的是 i18n key。老数据里存的是历史错误原文，
          // t() 查不到会回落显示原串，正好当降级用，不做数据迁移
          <span className="text-destructive block truncate text-xs" title={t(task.error)}>
            {t(task.error)}
          </span>
        )}
      </TableCell>
      <TableCell className="tabular-nums">{task.vidIndex}</TableCell>
      <TableCell>
        <Badge variant={STATUS_VARIANT[task.status]}>{t(`tasks.status.${task.status}`)}</Badge>
      </TableCell>
      <TableCell className="font-mono text-xs tabular-nums">
        {task.status === 'running' ? (
          <div className="flex flex-col gap-1">
            <Progress value={percent} />
            <span className="text-muted-foreground">
              {formatBytes(task.downloaded)} / {formatBytes(task.total)}
            </span>
          </div>
        ) : (
          formatBytes(task.total)
        )}
      </TableCell>
      <TableCell>
        <div className="flex gap-1">
          {task.status === 'running' && (
            <Button
              size="icon"
              variant="ghost"
              onClick={onStop}
              aria-label={t('tasks.actions.stop')}
            >
              <Pause className="size-4" />
            </Button>
          )}
          {(task.status === 'failed' || task.status === 'stopped') && (
            <Button
              size="icon"
              variant="ghost"
              onClick={onRetry}
              aria-label={t('tasks.actions.retry')}
            >
              <RotateCcw className="size-4" />
            </Button>
          )}
          {task.status === 'completed' && (
            <Button
              size="icon"
              variant="ghost"
              onClick={onOpenFolder}
              aria-label={t('tasks.actions.openFolder')}
            >
              <FolderOpen className="size-4" />
            </Button>
          )}
          <Button
            size="icon"
            variant="ghost"
            onClick={() => onDelete(false)}
            aria-label={t('tasks.actions.remove')}
          >
            <Trash2 className="size-4" />
          </Button>
        </div>
      </TableCell>
    </TableRow>
  );
}
