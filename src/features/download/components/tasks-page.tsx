import { useMemo, useState } from 'react';
import { Play, Pause, RotateCcw, FolderOpen, Trash2, X, ScanSearch } from 'lucide-react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import { Card } from '@/components/ui/card';
import { Progress } from '@/components/ui/progress';
import { Checkbox } from '@/components/ui/checkbox';
import { Label } from '@/components/ui/label';
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
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table';
import { useDownloadActions, useDownloadTasks, useQueueStatus } from '@/service/queries';
import { app as appApi } from '@/service/commands';
import { formatBytes } from '@/lib/format';
import { t, tf } from '@/i18n';
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

export function TasksPage() {
  const [selected, setSelected] = useState<Set<string>>(new Set());
  // 删除确认。`withFiles` 由弹窗里的勾选框决定，入口只负责给出要删哪些。
  const [confirmDelete, setConfirmDelete] = useState<null | { ids: string[]; withFiles: boolean }>(
    null,
  );

  const { data: tasks, isPending } = useDownloadTasks();
  const { data: status } = useQueueStatus();
  const actions = useDownloadActions();

  // 用 useMemo 稳定引用：`tasks ?? []` 每次渲染都是新数组，
  // 会让下游 useMemo 的依赖每次都变
  const all = useMemo(() => tasks ?? [], [tasks]);
  const failedIds = useMemo(
    () => all.filter((task) => task.status === 'failed').map((task) => task.id),
    [all],
  );
  const allIds = useMemo(() => all.map((task) => task.id), [all]);

  // 三态：0 个 / 全部 / 一部分。只按当前可见列表算，不去碰翻页之外的任务
  const selectedCount = allIds.filter((id) => selected.has(id)).length;
  const allChecked: boolean | 'indeterminate' =
    selectedCount === 0 ? false : selectedCount === allIds.length ? true : 'indeterminate';

  const toggleAll = () => setSelected(allChecked === true ? new Set() : new Set(allIds));

  /** 选中项里已下载完成的字节数：只有这些删文件才真的能释放空间。 */
  const freeableFor = useMemo(
    () =>
      (ids: string[]): number => {
        const set = new Set(ids);
        return all
          .filter((task) => set.has(task.id) && task.status === 'completed')
          .reduce((sum, task) => sum + task.total, 0);
      },
    [all],
  );

  const freeable = freeableFor([...selected]);

  /** 弹窗里要展示的两项：已下载完成的集数、可释放字节 */
  const confirmFreeable = confirmDelete ? freeableFor(confirmDelete.ids) : 0;
  const completedCount = useMemo(
    () =>
      confirmDelete
        ? all.filter((t) => confirmDelete.ids.includes(t.id) && t.status === 'completed').length
        : 0,
    [all, confirmDelete],
  );

  const toggle = (id: string) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const confirmAndDelete = () => {
    if (!confirmDelete) return;
    const { ids, withFiles } = confirmDelete;
    actions.remove.mutate(
      { ids, withFiles },
      {
        onSuccess: () => {
          setSelected(new Set());
          setConfirmDelete(null);
        },
      },
    );
  };
  if (isPending) {
    return <div className="text-muted-foreground p-6 text-sm">{t('common.loading')}</div>;
  }

  return (
    <div className="flex flex-col gap-4 p-6">
      <div className="flex flex-wrap items-center gap-2">
        <div className="text-muted-foreground text-sm tabular-nums">
          {status
            ? tf('tasks.queueStatus', {
                running: status.running,
                pending: status.pending,
                completed: status.completed,
                failed: status.failed,
              })
            : ''}
          {status &&
            ` · ${tf('tasks.concurrency', { active: status.active, limit: status.limit })}`}
        </div>

        <div className="ml-auto flex flex-wrap gap-2">
          <Button size="sm" variant="outline" onClick={() => actions.resumeAll.mutate()}>
            <Play className="size-4" />
            {t('tasks.startAll')}
          </Button>
          <Button size="sm" variant="outline" onClick={() => actions.pauseAll.mutate()}>
            <Pause className="size-4" />
            {t('tasks.pauseAll')}
          </Button>
          <Button
            size="sm"
            variant="outline"
            disabled={failedIds.length === 0}
            onClick={() => actions.retryMany.mutate(failedIds)}
          >
            <RotateCcw className="size-4" />
            {t('tasks.retryAll')}
          </Button>
          <Button size="sm" variant="outline" disabled={all.length === 0} onClick={toggleAll}>
            {allChecked === true ? t('tasks.deselectAll') : t('tasks.selectAll')}
          </Button>
          <Button
            size="sm"
            variant="outline"
            disabled={failedIds.length === 0}
            onClick={() => setSelected(new Set(failedIds))}
          >
            {t('tasks.selectFailed')}
          </Button>
          <Button
            size="sm"
            variant="outline"
            disabled={actions.rescan.isPending}
            onClick={() =>
              actions.rescan.mutate(undefined, {
                onSuccess: (count) =>
                  count > 0
                    ? toast.success(tf('tasks.rescanFound', { count }))
                    : toast.info(t('tasks.rescanNone')),
              })
            }
          >
            <ScanSearch className="size-4" />
            {actions.rescan.isPending ? t('common.loading') : t('tasks.rescan')}
          </Button>
        </div>
      </div>

      {all.length === 0 ? (
        <p className="text-muted-foreground py-16 text-center text-sm">{t('tasks.empty')}</p>
      ) : (
        <Card className="gap-0 overflow-hidden py-0">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead className="w-10">
                  <Checkbox
                    checked={allChecked}
                    onCheckedChange={toggleAll}
                    aria-label={allChecked === true ? t('tasks.deselectAll') : t('tasks.selectAll')}
                  />
                </TableHead>
                <TableHead>{t('nav.tasks.title')}</TableHead>
                <TableHead className="w-24">{t('tasks.columns.episode')}</TableHead>
                <TableHead className="w-28">{t('tasks.columns.status')}</TableHead>
                <TableHead className="w-32">{t('tasks.columns.size')}</TableHead>
                <TableHead className="w-20" />
              </TableRow>
            </TableHeader>
            <TableBody>
              {all.map((task) => (
                <TaskRow
                  key={task.id}
                  task={task}
                  checked={selected.has(task.id)}
                  onToggle={() => toggle(task.id)}
                  onStop={() => actions.stop.mutate(task.id)}
                  onRetry={() => actions.retry.mutate(task.id)}
                  onOpenFolder={() => void appApi.openFolder(task.seriesId)}
                  onDelete={(withFiles) => setConfirmDelete({ ids: [task.id], withFiles })}
                />
              ))}
            </TableBody>
          </Table>
        </Card>
      )}

      {selected.size > 0 && (
        <div className="bg-card sticky bottom-4 flex items-center gap-3 rounded-lg border px-4 py-2 shadow-lg">
          <span className="text-sm">{selected.size}</span>
          {freeable > 0 && (
            <span className="text-muted-foreground text-xs">
              {tf('tasks.freeable', { size: formatBytes(freeable) })}
            </span>
          )}
          <div className="ml-auto flex gap-2">
            <Button size="sm" variant="ghost" onClick={() => setSelected(new Set())}>
              <X className="size-4" />
            </Button>
            <Button
              size="sm"
              variant="destructive"
              onClick={() => setConfirmDelete({ ids: [...selected], withFiles: false })}
            >
              <Trash2 className="size-4" />
              {t('common.delete')}
            </Button>
          </div>
        </div>
      )}

      <AlertDialog open={confirmDelete !== null} onOpenChange={(o) => !o && setConfirmDelete(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>
              {confirmDelete && tf('tasks.removeConfirm', { count: confirmDelete.ids.length })}
            </AlertDialogTitle>
            <AlertDialogDescription asChild>
              <div className="grid gap-3">
                <span>
                  {confirmDelete &&
                    tf('tasks.removeSummary', {
                      count: confirmDelete.ids.length,
                      done: completedCount,
                      size: formatBytes(confirmFreeable),
                    })}
                </span>
                <Label className="flex cursor-pointer items-center gap-2 font-normal">
                  <Checkbox
                    checked={confirmDelete?.withFiles ?? false}
                    onCheckedChange={(v) =>
                      setConfirmDelete((prev) => (prev ? { ...prev, withFiles: v === true } : prev))
                    }
                  />
                  <span>
                    {confirmFreeable > 0
                      ? tf('tasks.removeFilesWithSize', { size: formatBytes(confirmFreeable) })
                      : t('tasks.removeFiles')}
                  </span>
                </Label>
                <span>{t('tasks.removeKeepHint')}</span>
              </div>
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>{t('common.cancel')}</AlertDialogCancel>
            <AlertDialogAction onClick={confirmAndDelete}>
              {confirmDelete?.withFiles ? t('tasks.removeAndFiles') : t('tasks.removeOnly')}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}

interface RowProps {
  task: DownloadTask;
  checked: boolean;
  onToggle: () => void;
  onStop: () => void;
  onRetry: () => void;
  onOpenFolder: () => void;
  onDelete: (withFiles: boolean) => void;
}

function TaskRow({ task, checked, onToggle, onStop, onRetry, onOpenFolder, onDelete }: RowProps) {
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
