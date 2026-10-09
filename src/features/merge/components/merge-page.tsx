import { useState } from 'react';
import { Combine, Zap, Gauge, Trash2, FolderOpen } from 'lucide-react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Badge } from '@/components/ui/badge';
import { Progress } from '@/components/ui/progress';
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert';
import { Tabs, TabsList, TabsTrigger, TabsContent } from '@/components/ui/tabs';
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
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import {
  useMergeActions,
  useMergeCandidates,
  useMergeEvents,
  useMergePreflight,
  useMergeTasks,
} from '@/lib/queries';
import { formatBytes } from '@/lib/format';
import { t, tf } from '@/i18n';
import type { MergeMode } from '@/service/schema';

/** 一键合并：快速合并（流复制）与兼容合并（转码）。 */
export function MergePage() {
  // 候选项按「有已下载分集」聚合，不用剧集档案：档案被移除记录软删除后
  // 本地文件仍在，那部剧照样合得起来。
  const { data: candidates, isPending: candidatesPending } = useMergeCandidates();
  const [seriesId, setSeriesId] = useState('');
  const [outputName, setOutputName] = useState('');
  const [mode, setMode] = useState<MergeMode>('quick');

  const { data: preflight } = useMergePreflight(seriesId || null);
  const { data: tasks } = useMergeTasks();
  const { start, remove, openOutput } = useMergeActions();
  useMergeEvents();

  /** 待删除的合并任务 id：null 表示确认框没打开 */
  const [pendingDelete, setPendingDelete] = useState<string | null>(null);

  const current = candidates?.find((s) => s.seriesId === seriesId);
  const output = outputName || current?.seriesTitle || '';

  // 已有任务在跑同一个输出名时禁用按钮。后端也会拦（见 merge_service::guard），
  // 但那是最后一道防线：真按下去只会得到一个报错 toast，界面却已经跳了两三次。
  const busy = (tasks ?? []).some((t) => t.status === 'running' && t.outputName === output);

  // 一部都没下过时直接说清楚：下拉是空的，placeholder 只会让人以为是加载卡住了
  const noCandidates = !candidatesPending && (candidates?.length ?? 0) === 0;

  // 快速合并是整文件字节级顺序拼接，编码不一致时产出的文件连索引都过不去
  const quickUnavailable =
    preflight != null && preflight.episodeCount > 0 && !preflight.codecConsistent;

  // 换剧会让 quick 变禁用，但 mode 可能还停在 quick。mode 记的是用户的模式偏好，
  // effectiveMode 才是这一次真正能用的模式：quick 被禁用就从偏好派生 compat，
  // Tab 选中项与提交参数同源，不会出现「Tab 停在 quick、点开始却提交别的模式」。
  const effectiveMode: MergeMode = quickUnavailable ? 'compat' : mode;

  const handleStart = () => {
    if (!seriesId || !output) return;
    start.mutate(
      { seriesId, outputName: output, mode: effectiveMode },
      {
        // 后端派发到后台线程后立刻返回 running 任务，真正的进度与成败
        // 走合并事件流。所以这里只确认「已启动」，别去判断最终状态。
        onSuccess: () => toast.success(t('merge.started')),
        onError: (e) => toast.error(e.message),
      },
    );
  };

  const confirmDelete = () => {
    if (!pendingDelete) return;
    remove.mutate(pendingDelete, {
      onSuccess: () => setPendingDelete(null),
      onError: (e) => toast.error(e.message),
    });
  };

  return (
    <div className="mx-auto flex max-w-3xl flex-col gap-4 p-6">
      <Card>
        <CardHeader>
          <CardTitle className="text-base">{t('merge.title')}</CardTitle>
        </CardHeader>
        <CardContent className="flex flex-col gap-4">
          {/* 选剧 */}
          <div className="flex flex-col gap-2">
            <Label htmlFor="merge-series">{t('merge.selectSeries')}</Label>
            <Select
              value={seriesId || undefined}
              onValueChange={setSeriesId}
              disabled={noCandidates || candidatesPending}
            >
              <SelectTrigger id="merge-series" className="w-full">
                <SelectValue placeholder={t('merge.pickSeries')} />
              </SelectTrigger>
              <SelectContent>
                {(candidates ?? []).map((s) => (
                  <SelectItem key={s.seriesId} value={s.seriesId}>
                    {/* 带上可合并集数：一部剧下了几集是这里最该先看到的信息 */}
                    {s.seriesTitle} · {tf('common.episodeCount', { count: s.episodeCount })}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            {noCandidates && (
              <p className="text-muted-foreground text-sm">{t('merge.noCandidates')}</p>
            )}
          </div>

          <div className="flex flex-col gap-2">
            <Label htmlFor="merge-output">{t('merge.outputName')}</Label>
            <Input
              id="merge-output"
              value={output}
              onChange={(e) => setOutputName(e.target.value)}
              placeholder={current?.seriesTitle ?? ''}
            />
          </div>

          {/* 模式 */}
          <Tabs value={effectiveMode} onValueChange={(v) => setMode(v as MergeMode)}>
            <TabsList className="w-full">
              <TabsTrigger value="quick" className="flex-1" disabled={quickUnavailable}>
                <Zap className="size-4" />
                {t('merge.quick')}
              </TabsTrigger>
              <TabsTrigger value="compat" className="flex-1">
                <Gauge className="size-4" />
                {t('merge.compat')}
              </TabsTrigger>
            </TabsList>
            <TabsContent value="quick" className="text-muted-foreground pt-2 text-sm">
              {quickUnavailable ? t('merge.quickUnavailable') : t('merge.quickDesc')}
            </TabsContent>
            <TabsContent value="compat" className="text-muted-foreground pt-2 text-sm">
              {t('merge.compatDesc')}
            </TabsContent>
          </Tabs>

          {/* 合并前校验。warnings 传的是 i18n key，这里带变量池逐条翻成当前语言。 */}
          {preflight && (
            <Alert variant={preflight.ok ? 'default' : 'warning'}>
              {/* 0 集时标题栏整块不渲染：没有集数也没有体积可汇总，
                  而「没有已下载的分集」已经由下面的 warnings 说了，
                  再写一遍就是同一句话在同一个 Alert 里出现两次。 */}
              {preflight.episodeCount > 0 && (
                <AlertTitle>
                  {tf('common.episodeCount', { count: preflight.episodeCount })} ·{' '}
                  {formatBytes(preflight.estimatedSize)}
                  {/* freeSpace 为 null 是「查不到」，显示成 0 B 是在骗人，所以整段不出现 */}
                  {preflight.freeSpace !== null &&
                    ` · ${tf('merge.freeSpace', { size: formatBytes(preflight.freeSpace) })}`}
                </AlertTitle>
              )}
              {preflight.warnings.length > 0 && (
                <AlertDescription>
                  {preflight.warnings
                    .map((key) => tf(key, { episode: preflight.codecMismatchEpisode ?? 0 }))
                    .join('；')}
                </AlertDescription>
              )}
            </Alert>
          )}

          <Button
            onClick={handleStart}
            disabled={busy || !seriesId || !output || !preflight?.episodeCount}
          >
            <Combine className="size-4" />
            {busy ? t('merge.running') : t('merge.start')}
          </Button>
        </CardContent>
      </Card>

      {/* 合并任务记录 */}
      {tasks && tasks.length > 0 && (
        <Card>
          <CardHeader>
            <CardTitle className="text-base">{t('merge.taskList')}</CardTitle>
          </CardHeader>
          <CardContent className="flex flex-col gap-2">
            {tasks.map((task) => (
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
                {task.status === 'running' && (
                  <div className="flex w-28 shrink-0 items-center gap-2">
                    <Progress value={task.percent} />
                    <span className="text-muted-foreground w-9 text-right font-mono text-xs tabular-nums">
                      {Math.round(task.percent)}%
                    </span>
                  </div>
                )}
                {/* 合并完最顺手的下一步就是拿走成品：一键定位到产物文件 */}
                {task.status === 'completed' && task.outputPath && (
                  <Button
                    size="icon"
                    variant="ghost"
                    disabled={openOutput.isPending}
                    onClick={() => openOutput.mutate(task.id)}
                    aria-label={t('merge.actions.openOutput')}
                    title={task.outputPath}
                  >
                    <FolderOpen className="size-4" />
                  </Button>
                )}
                <Button
                  size="icon"
                  variant="ghost"
                  onClick={() => setPendingDelete(task.id)}
                  aria-label={t('merge.actions.remove')}
                >
                  <Trash2 className="size-4" />
                </Button>
              </div>
            ))}
          </CardContent>
        </Card>
      )}

      <AlertDialog open={pendingDelete !== null} onOpenChange={(o) => !o && setPendingDelete(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t('merge.removeConfirm')}</AlertDialogTitle>
            <AlertDialogDescription>{t('merge.removeDesc')}</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>{t('common.cancel')}</AlertDialogCancel>
            <AlertDialogAction onClick={confirmDelete}>{t('common.delete')}</AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}
