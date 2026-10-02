import { useState } from 'react';
import { Combine, Zap, Gauge } from 'lucide-react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Badge } from '@/components/ui/badge';
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert';
import { Tabs, TabsList, TabsTrigger, TabsContent } from '@/components/ui/tabs';
import {
  useMergeActions,
  useMergeEvents,
  useMergePreflight,
  useMergeTasks,
  useSeriesList,
} from '@/lib/queries';
import { formatBytes } from '@/lib/format';
import { t } from '@/i18n';
import type { MergeMode } from '@/lib/schema';

/** 一键合并：快速合并（流复制）与兼容合并（转码）。 */
export function MergePage() {
  const { data: seriesList } = useSeriesList();
  const [seriesId, setSeriesId] = useState('');
  const [outputName, setOutputName] = useState('');
  const [mode, setMode] = useState<MergeMode>('quick');

  const { data: preflight } = useMergePreflight(seriesId || null);
  const { data: tasks } = useMergeTasks();
  const { start } = useMergeActions();
  useMergeEvents();

  const current = seriesList?.find((s) => s.seriesId === seriesId);
  const output = outputName || current?.title || '';

  const handleStart = () => {
    if (!seriesId || !output) return;
    start.mutate(
      { seriesId, outputName: output, mode },
      {
        onSuccess: (task) => {
          if (task.status === 'completed') {
            toast.success(`${t('merge.title')} · ${task.episodeCount} 集`);
          }
        },
        onError: (e) => toast.error(e.message),
      },
    );
  };

  return (
    <div className="mx-auto flex max-w-3xl flex-col gap-4 p-6">
      <Card>
        <CardHeader>
          <CardTitle className="text-base">{t('merge.title')}</CardTitle>
          <CardDescription>{t('merge.subtitle')}</CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-4">
          {/* 选剧 */}
          <div className="flex flex-col gap-2">
            <Label htmlFor="merge-series">{t('merge.selectSeries')}</Label>
            <select
              id="merge-series"
              value={seriesId}
              onChange={(e) => setSeriesId(e.target.value)}
              className="border-input bg-background h-9 w-full rounded-md border px-3 text-sm"
            >
              <option value="">{t('merge.pickSeries')}</option>
              {(seriesList ?? []).map((s) => (
                <option key={s.seriesId} value={s.seriesId}>
                  {s.title}
                </option>
              ))}
            </select>
          </div>

          <div className="flex flex-col gap-2">
            <Label htmlFor="merge-output">{t('merge.outputName')}</Label>
            <Input
              id="merge-output"
              value={output}
              onChange={(e) => setOutputName(e.target.value)}
              placeholder={current?.title ?? ''}
            />
          </div>

          {/* 模式 */}
          <Tabs value={mode} onValueChange={(v) => setMode(v as MergeMode)}>
            <TabsList className="w-full">
              <TabsTrigger value="quick" className="flex-1">
                <Zap className="size-4" />
                {t('merge.quick')}
              </TabsTrigger>
              <TabsTrigger value="compat" className="flex-1">
                <Gauge className="size-4" />
                {t('merge.compat')}
              </TabsTrigger>
            </TabsList>
            <TabsContent value="quick" className="pt-2 text-sm text-muted-foreground">
              {t('merge.quickDesc')}
            </TabsContent>
            <TabsContent value="compat" className="pt-2 text-sm text-muted-foreground">
              {t('merge.compatDesc')}
            </TabsContent>
          </Tabs>

          {/* 合并前校验 */}
          {preflight && (
            <Alert variant={preflight.ok ? 'default' : 'warning'}>
              <AlertTitle>
                {preflight.episodeCount > 0
                  ? `${preflight.episodeCount} 集 · ${formatBytes(preflight.estimatedSize)}`
                  : t('merge.noDownloads')}
              </AlertTitle>
              {preflight.warnings.length > 0 && (
                <AlertDescription>{preflight.warnings.join('；')}</AlertDescription>
              )}
            </Alert>
          )}

          <Button
            onClick={handleStart}
            disabled={!seriesId || !output || !preflight?.episodeCount}
          >
            <Combine className="size-4" />
            {t('merge.start')}
          </Button>
        </CardContent>
      </Card>

      {/* 合并任务记录 */}
      {(tasks?.length ?? 0) > 0 && (
        <Card>
          <CardHeader>
            <CardTitle className="text-base">{t('merge.taskList')}</CardTitle>
          </CardHeader>
          <CardContent className="flex flex-col gap-2">
            {tasks!.map((task) => (
              <div
                key={task.id}
                className="flex items-center gap-2 rounded-md border px-3 py-2 text-sm"
              >
                <span className="min-w-0 flex-1 truncate">{task.outputName}</span>
                <Badge
                  variant={
                    task.status === 'completed'
                      ? 'success'
                      : task.status === 'failed'
                        ? 'destructive'
                        : 'secondary'
                  }
                >
                  {t(`merge.status.${task.status}`)}
                </Badge>
                {task.outputSize > 0 && (
                  <span className="text-muted-foreground font-mono text-xs">
                    {formatBytes(task.outputSize)}
                  </span>
                )}
                {task.status === 'running' && task.percent > 0 && (
                  <span className="text-muted-foreground font-mono text-xs">
                    {Math.round(task.percent)}%
                  </span>
                )}
              </div>
            ))}
          </CardContent>
        </Card>
      )}
    </div>
  );
}
