import { useState } from 'react';
import { FolderOpen, Cpu, Zap, Copy, ExternalLink } from 'lucide-react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Switch } from '@/components/ui/switch';
import { Badge } from '@/components/ui/badge';
import { Separator } from '@/components/ui/separator';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import {
  useDecodeCapability,
  useRedetectCapability,
  useSaveSettings,
  useSettings,
  useStorageUsage,
  useTestProxy,
} from '@/service/queries';
import { app as appApi, transcode as transcodeApi } from '@/service/commands';
import { formatBytes } from '@/lib/format';
import { t, tf } from '@/i18n';
import type { DecodeCapability, Settings } from '@/service/schema';
import { UpdateCard } from '@/features/update/update-card';
import { AccountCard } from './account-card';

/** 设置页：目录 / 命名 / 并发 / 代理 / 播放 / 存储。 */
export function SettingsPage() {
  const { data: loaded, isPending, refetch: refetchSettings } = useSettings();
  const saveMutation = useSaveSettings();
  const { data: capability } = useDecodeCapability();
  const redetect = useRedetectCapability();
  const { data: usage } = useStorageUsage();
  const testProxyMutation = useTestProxy();

  // 草稿为 null 表示「未修改」，直接用服务端值派生，避免 effect 同步引发级联渲染
  const [draft, setDraft] = useState<Partial<Settings> | null>(null);
  const [proxyUrl, setProxyUrl] = useState<string | null>(null);

  if (isPending || !loaded) {
    return <div className="text-muted-foreground p-6 text-sm">{t('common.loading')}</div>;
  }

  // 合并草稿与服务端值
  const current: Settings = { ...loaded, ...(draft ?? {}) };
  const effectiveProxyUrl = proxyUrl ?? current.proxy.url;

  const patch = (next: Partial<Settings>) => setDraft((prev) => ({ ...(prev ?? {}), ...next }));
  const patchProxy = (next: Partial<Settings['proxy']>) =>
    setDraft((prev) => ({ ...(prev ?? {}), proxy: { ...current.proxy, ...next } }));

  const chooseDir = async () => {
    const picked = await appApi.selectFolder().catch(() => null);
    if (picked) patch({ downloadDir: picked });
  };

  const submit = () => {
    saveMutation.mutate(
      { ...current, proxy: { ...current.proxy, url: effectiveProxyUrl } },
      {
        onSuccess: () => {
          toast.success(t('settings.saved'));
          // 保存成功后丢弃草稿，回到「跟随服务端」状态
          setDraft(null);
          setProxyUrl(null);
        },
        onError: (e: Error) => toast.error(e.message),
      },
    );
  };

  return (
    <div className="mx-auto flex max-w-3xl flex-col gap-4 p-6">
      {/* 账户（短信登录） */}
      <AccountCard account={current.account ?? null} onChanged={() => void refetchSettings()} />

      {/* 目录 */}
      <Card>
        <CardHeader>
          <CardTitle className="text-base">{t('settings.downloadDir')}</CardTitle>
        </CardHeader>
        <CardContent className="flex gap-2">
          <Input
            value={current.downloadDir}
            onChange={(e) => patch({ downloadDir: e.target.value })}
            className="font-mono text-xs"
          />
          <Button variant="outline" onClick={() => void chooseDir()}>
            <FolderOpen className="size-4" />
            {t('settings.choose')}
          </Button>
        </CardContent>
      </Card>

      {/* 命名与并发 */}
      <Card>
        <CardHeader>
          <CardTitle className="text-base">{t('settings.naming')}</CardTitle>
        </CardHeader>
        <CardContent className="flex flex-col gap-4">
          <div className="flex flex-col gap-2">
            <Label>{t('settings.naming')}</Label>
            <Select
              value={current.naming}
              onValueChange={(v) => patch({ naming: v as Settings['naming'] })}
            >
              <SelectTrigger className="w-full">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="titleIndex">{t('settings.namingTitleIndex')}</SelectItem>
                <SelectItem value="titleIndexEpisode">
                  {t('settings.namingTitleIndexEpisode')}
                </SelectItem>
                <SelectItem value="onlyTitle">{t('settings.namingOnlyTitle')}</SelectItem>
              </SelectContent>
            </Select>
          </div>

          <div className="flex flex-col gap-2">
            <Label htmlFor="concurrency">
              {t('settings.concurrency')} — {current.maxConcurrency}
            </Label>
            <Input
              id="concurrency"
              type="range"
              min={1}
              max={10}
              value={current.maxConcurrency}
              onChange={(e) => patch({ maxConcurrency: Number(e.target.value) })}
            />
            <p className="text-muted-foreground text-xs">{t('settings.concurrencyDesc')}</p>
          </div>
        </CardContent>
      </Card>

      {/* 代理 */}
      <Card>
        <CardHeader>
          <CardTitle className="text-base">{t('settings.proxy')}</CardTitle>
        </CardHeader>
        <CardContent className="flex flex-col gap-3">
          <Select
            value={current.proxy.mode}
            onValueChange={(v) => patchProxy({ mode: v as Settings['proxy']['mode'] })}
          >
            <SelectTrigger className="w-full">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="system">{t('settings.proxySystem')}</SelectItem>
              <SelectItem value="manual">{t('settings.proxyManual')}</SelectItem>
              <SelectItem value="direct">{t('settings.proxyDirect')}</SelectItem>
            </SelectContent>
          </Select>

          {current.proxy.mode === 'manual' && (
            <Input
              value={effectiveProxyUrl}
              onChange={(e) => setProxyUrl(e.target.value)}
              placeholder="http://127.0.0.1:7890"
              className="font-mono text-xs"
            />
          )}

          <div className="flex items-center gap-2">
            <Button
              size="sm"
              variant="secondary"
              disabled={testProxyMutation.isPending}
              onClick={() =>
                testProxyMutation.mutate(
                  { ...current.proxy, url: effectiveProxyUrl },
                  {
                    onSuccess: (r) => {
                      if (r.ok) toast.success(tf('settings.testOk', { ms: r.elapsedMs }));
                      else toast.error(tf('settings.testFail', { message: r.message }));
                    },
                  },
                )
              }
            >
              <Zap className="size-4" />
              {testProxyMutation.isPending ? t('settings.testing') : t('settings.testProxy')}
            </Button>
            {testProxyMutation.data && (
              <Badge variant={testProxyMutation.data.ok ? 'success' : 'destructive'}>
                {testProxyMutation.data.elapsedMs} ms
              </Badge>
            )}
          </div>
        </CardContent>
      </Card>

      <Separator />

      {/* 播放 */}
      <Card>
        <CardHeader>
          <CardTitle className="text-base">{t('settings.playback')}</CardTitle>
        </CardHeader>
        <CardContent className="flex flex-col gap-3">
          <ToggleRow
            id="auto-next"
            label={t('settings.autoNext')}
            checked={current.autoNextEpisode}
            onChange={(v) => patch({ autoNextEpisode: v })}
          />
          <ToggleRow
            id="auto-delete"
            label={t('settings.autoDelete')}
            checked={current.autoDeleteAfterPlay}
            onChange={(v) => patch({ autoDeleteAfterPlay: v })}
          />

          <div className="bg-muted/50 flex flex-wrap items-center gap-2 rounded-md p-3 text-sm">
            <Cpu className="text-muted-foreground size-4" />
            <span>{t('settings.transcodeBackend')}</span>
            <Badge
              variant={
                capability?.platformHwEncoder || capability?.h264HwEncoder
                  ? 'success'
                  : capability?.platformEncoder || capability?.hasFfmpeg
                    ? 'warning'
                    : 'secondary'
              }
              className="ml-auto"
            >
              {backendLabel(capability)}
            </Badge>
            {/* 装完 ffmpeg 不重启应用是看不到变化的：进程环境变量不会更新，
                探测结果也被缓存着。给一条「重新检测」比让人去重启合理。 */}
            <Button
              size="sm"
              variant="ghost"
              disabled={redetect.isPending}
              onClick={() => redetect.mutate()}
            >
              {redetect.isPending ? t('common.loading') : t('settings.redetectBackend')}
            </Button>
          </div>
          <p className="text-muted-foreground text-xs">{t('settings.transcodeBackendHint')}</p>
          {redetect.isSuccess && (
            <p className="text-muted-foreground text-xs">
              {tf('settings.redetectDone', { backend: backendLabel(capability) })}
            </p>
          )}
          {/* 没装 ffmpeg 时给一条可照抄的安装路径：说清楚「不装也能用，只是慢」，
              再把命令和下载页都递到手边 */}
          {capability && !capability.hasFfmpeg && (
            <div className="bg-muted/50 flex flex-col gap-2 rounded-md p-3 text-xs">
              <p>{t('settings.ffmpegInstallHint')}</p>
              <div className="flex flex-wrap items-center gap-2">
                <code className="bg-background rounded px-2 py-1 font-mono">
                  winget install Gyan.FFmpeg
                </code>
                <Button
                  size="sm"
                  variant="ghost"
                  onClick={() => {
                    void navigator.clipboard
                      .writeText('winget install Gyan.FFmpeg')
                      .then(() => toast.success(t('settings.commandCopied')))
                      .catch(() => toast.error(t('settings.copyFailed')));
                  }}
                >
                  <Copy className="size-4" />
                  {t('settings.copyCommand')}
                </Button>
                <Button
                  size="sm"
                  variant="ghost"
                  onClick={() =>
                    void appApi
                      .openExternalPage('https://www.gyan.dev/ffmpeg/builds/')
                      .catch((e: Error) => toast.error(e.message))
                  }
                >
                  <ExternalLink className="size-4" />
                  {t('settings.ffmpegDownloadPage')}
                </Button>
              </div>
              <p className="text-muted-foreground">{t('settings.ffmpegInstallNote')}</p>
            </div>
          )}
        </CardContent>
      </Card>

      {/* 存储 */}
      <Card>
        <CardHeader>
          <CardTitle className="text-base">{t('settings.storage')}</CardTitle>
        </CardHeader>
        <CardContent className="flex flex-col gap-3">
          <p className="text-sm tabular-nums">
            {usage
              ? tf('settings.storageUsage', { size: formatBytes(usage.bytes), files: usage.files })
              : t('common.loading')}
          </p>
          {/* 磁盘清理只在存储页提供，这里只读占用与缓存清理。
              点击必须有回音：成功报清理条数，失败弹错误——此前是纯
              fire-and-forget，用户眼里就是「点了没反应」。 */}
          <div className="flex gap-2">
            <Button
              size="sm"
              variant="outline"
              onClick={() =>
                transcodeApi.clearOnlineCache().then(
                  (n) => toast.success(tf('settings.clearedCache', { count: n })),
                  (e) => toast.error(e.message),
                )
              }
            >
              {t('settings.clearOnlineCache')}
            </Button>
            <Button
              size="sm"
              variant="outline"
              onClick={() =>
                transcodeApi.clearCompatCache().then(
                  (n) => toast.success(tf('settings.clearedCache', { count: n })),
                  (e) => toast.error(e.message),
                )
              }
            >
              {t('settings.clearCompatCache')}
            </Button>
          </div>
        </CardContent>
      </Card>

      {/* 更新 */}
      <UpdateCard />

      <div className="flex justify-end">
        <Button onClick={submit} disabled={saveMutation.isPending}>
          {t('settings.save')}
        </Button>
      </div>
    </div>
  );
}

/**
 * 告诉用户「兼容合并会跑多快」，而不是「用什么写的」。
 *
 * 探测结果里有 Rust / ffmpeg / 编码器这些实现细节，但用户只关心快慢和
 * 要不要额外装东西，所以标签一律按速度分档。
 */
function backendLabel(cap: DecodeCapability | undefined): string {
  if (!cap) return t('common.loading');
  // 真硬件（平台硬编 / ffmpeg 硬编）才进「硬件加速」档；macOS 无硬编时
  // VideoToolbox 走 Apple 软编会话——比 ffmpeg 快但编码仍在 CPU，归「软件加速」。
  if (cap.platformHwEncoder || cap.h264HwEncoder) return t('settings.backendFfmpegHw');
  if (cap.platformEncoder || cap.hasFfmpeg) return t('settings.backendFfmpegSw');
  return t('settings.backendRust');
}

function ToggleRow({
  id,
  label,
  checked,
  onChange,
}: {
  id: string;
  label: string;
  checked: boolean;
  onChange: (v: boolean) => void;
}) {
  return (
    <div className="flex items-center gap-3">
      <Switch id={id} checked={checked} onCheckedChange={onChange} />
      <Label htmlFor={id} className="cursor-pointer">
        {label}
      </Label>
    </div>
  );
}
