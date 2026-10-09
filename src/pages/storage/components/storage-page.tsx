import { useState } from 'react';
import { HardDrive, Trash2 } from 'lucide-react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { Badge } from '@/components/ui/badge';
import { Input } from '@/components/ui/input';
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
import { useStorageActions, useStorageSeries, useStorageUsage } from '@/service/queries';
import { formatBytes } from '@/utils/format';
import { t, tf } from '@/locales';

/** 磁盘占用与清理。列表即下载管理里真有文件落在磁盘上的剧。 */
export function StoragePage() {
  const { data: usage } = useStorageUsage();
  const { data: seriesList } = useStorageSeries();
  const { deleteSeries, deleteAll } = useStorageActions();
  const [filter, setFilter] = useState('');
  const [confirmAll, setConfirmAll] = useState(false);

  const visible = (seriesList ?? []).filter((s) => {
    if (!filter.trim()) return true;
    const kw = filter.trim().toLowerCase();
    return s.title.toLowerCase().includes(kw);
  });

  const handleDelete = (seriesId: string, title: string) => {
    deleteSeries.mutate(seriesId, {
      onSuccess: (n) => toast.success(`${title} · ${tf('storage.freedFiles', { count: n })}`),
      onError: (e) => toast.error(e.message),
    });
  };

  return (
    <div className="mx-auto flex max-w-3xl flex-col gap-4 p-6">
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2 text-base">
            <HardDrive className="size-4" />
            {t('settings.storage')}
          </CardTitle>
          <CardDescription>{t('storage.desc')}</CardDescription>
        </CardHeader>
        <CardContent className="flex items-center gap-3">
          {/* 加载中给占位而不是 0 B——「还没拿到数」和「真的清空了」不是一回事 */}
          <span className="text-2xl font-semibold tabular-nums">
            {usage ? formatBytes(usage.bytes) : t('common.loading')}
          </span>
          {usage && (
            <span className="text-muted-foreground text-sm">
              {usage.files} {t('storage.files')}
            </span>
          )}
          <Button
            variant="destructive"
            size="sm"
            className="ml-auto"
            onClick={() => setConfirmAll(true)}
          >
            <Trash2 className="size-4" />
            {t('settings.deleteAll')}
          </Button>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle className="text-base">{t('storage.bySeries')}</CardTitle>
        </CardHeader>
        <CardContent className="flex flex-col gap-3">
          <Input
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
            placeholder={t('player.seriesSearch')}
          />

          {visible.length === 0 ? (
            <p className="text-muted-foreground py-8 text-center text-sm">{t('common.empty')}</p>
          ) : (
            <div className="flex flex-col gap-2">
              {visible.map((s) => (
                <div
                  key={s.seriesId}
                  className="flex items-center gap-2 rounded-md border px-3 py-2"
                >
                  <span className="min-w-0 flex-1 truncate text-sm">{s.title}</span>
                  <Badge variant="secondary">
                    {tf('storage.downloadedEpisodes', { count: s.files })}
                  </Badge>
                  <span className="text-muted-foreground w-16 text-right text-xs tabular-nums">
                    {formatBytes(s.bytes)}
                  </span>
                  <Button
                    size="sm"
                    variant="ghost"
                    aria-label={tf('storage.deleteFilesOf', { title: s.title })}
                    title={t('storage.deleteFiles')}
                    onClick={() => handleDelete(s.seriesId, s.title)}
                  >
                    <Trash2 className="size-4" />
                  </Button>
                </div>
              ))}
            </div>
          )}
        </CardContent>
      </Card>

      <AlertDialog open={confirmAll} onOpenChange={setConfirmAll}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t('storage.deleteAllConfirm')}</AlertDialogTitle>
            <AlertDialogDescription>{t('storage.deleteAllDesc')}</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>{t('common.cancel')}</AlertDialogCancel>
            <AlertDialogAction
              onClick={() =>
                deleteAll.mutate(undefined, {
                  onSuccess: (n) => toast.success(tf('storage.freedFiles', { count: n })),
                  onError: (e) => toast.error(e.message),
                })
              }
            >
              {t('common.confirm')}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}
