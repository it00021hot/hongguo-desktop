import { useState } from 'react';
import { ListCheck, Trash2 } from 'lucide-react';
import { Button } from '@/components/ui/button';
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

/** 管理模式开关按钮（列表头部右侧用）。 */
export function ManageToggle({
  managing,
  disabled,
  onEnter,
  onExit,
}: {
  managing: boolean;
  disabled?: boolean;
  onEnter: () => void;
  onExit: () => void;
}) {
  return (
    <Button size="sm" variant="outline" disabled={disabled} onClick={managing ? onExit : onEnter}>
      {managing ? t('batch.exitManage') : t('batch.manage')}
    </Button>
  );
}

/** 批量操作条：全选 + 已选计数 + 删除（带确认框）。吸底随页面滚动。 */
export function BatchBar({
  count,
  total,
  deleting,
  deleteLabel,
  confirmTitle,
  onToggleAll,
  onDelete,
}: {
  count: number;
  total: number;
  deleting: boolean;
  /** 删除按钮文案（区分页面动作，如「删除所选历史」） */
  deleteLabel: string;
  confirmTitle: string;
  onToggleAll: () => void;
  onDelete: () => void;
}) {
  const [confirmOpen, setConfirmOpen] = useState(false);
  const allPicked = total > 0 && count >= total;
  return (
    <div className="bg-background/95 sticky bottom-0 z-10 -mx-4 flex items-center gap-3 border-t px-4 py-3">
      <Button variant="ghost" size="sm" disabled={total === 0} onClick={onToggleAll}>
        <ListCheck className="size-4" aria-hidden />
        {allPicked ? t('batch.deselectAll') : t('batch.selectAll')}
      </Button>
      <span className="text-muted-foreground text-xs tabular-nums">
        {tf('batch.selectedCount', { count })}
      </span>
      <Button
        variant="destructive"
        size="sm"
        className="ml-auto"
        disabled={count === 0 || deleting}
        onClick={() => setConfirmOpen(true)}
      >
        <Trash2 className="size-4" aria-hidden />
        {deleting ? t('batch.deleting') : deleteLabel}
      </Button>
      <AlertDialog open={confirmOpen} onOpenChange={setConfirmOpen}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{confirmTitle}</AlertDialogTitle>
            <AlertDialogDescription>{tf('batch.confirmBody', { count })}</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>{t('common.cancel')}</AlertDialogCancel>
            <AlertDialogAction
              onClick={() => {
                setConfirmOpen(false);
                onDelete();
              }}
            >
              {t('common.confirm')}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}

/** 管理模式下的选择圆钮（卡片左上/行首用）。 */
export function PickDot({ checked, onToggle }: { checked: boolean; onToggle: () => void }) {
  return (
    <button
      type="button"
      role="checkbox"
      aria-checked={checked}
      onClick={(e) => {
        e.stopPropagation();
        onToggle();
      }}
      className={
        checked
          ? 'bg-primary text-primary-foreground grid size-5 shrink-0 place-items-center rounded-full transition-colors'
          : 'border-muted-foreground/50 size-5 shrink-0 rounded-full border-2 transition-colors'
      }
    >
      {checked && <ListCheck className="size-3.5" aria-hidden />}
    </button>
  );
}
