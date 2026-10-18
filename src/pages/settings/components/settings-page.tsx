import { useState } from 'react';
import { FolderOpen, Cpu, Zap, Copy, ExternalLink } from 'lucide-react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
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
  useCacheClear,
  useCacheUsage,
  useDecodeCapability,
  useDefaultCacheDir,
  useRedetectCapability,
  useSaveSettings,
  useSettings,
  useStorageUsage,
  useTestProxy,
} from '@/service/queries';
import { app as appApi } from '@/service/commands';
import { formatBytes } from '@/utils/format';
import { isMac } from '@/utils/platform';
import { useShortcutsStore } from '@/stores/shortcuts';
import { t, tf } from '@/locales';
import type { DecodeCapability, Settings } from '@/service/schema';
import { UpdateCard } from '@/features/update/update-card';
import { AccountCard } from './account-card';
import { ShortcutCard } from './shortcut-card';
import { BossKeyCard, resolveBossKeyConflicts } from './boss-key-card';
import { ToggleRow } from './toggle-row';

/** ffmpeg 安装路径随平台：Windows 走 winget + gyan 构建，macOS 走
 *  Homebrew + 官方下载页（官方页列有各 mac 构建源）。 */
const FFMPEG_INSTALL = isMac()
  ? { cmd: 'brew install ffmpeg', page: 'https://ffmpeg.org/download.html' }
  : { cmd: 'winget install Gyan.FFmpeg', page: 'https://www.gyan.dev/ffmpeg/builds/' };

/** 设置页：目录 / 命名 / 并发 / 代理 / 播放 / 存储。 */
export function SettingsPage() {
  const { data: loaded, isPending, refetch: refetchSettings } = useSettings();
  const saveMutation = useSaveSettings();
  const { data: capability } = useDecodeCapability();
  const redetect = useRedetectCapability();
  const { data: usage } = useStorageUsage();
  const { data: cacheUsage } = useCacheUsage();
  const { data: defaultCacheDir } = useDefaultCacheDir();
  const clearCache = useCacheClear();
  const testProxyMutation = useTestProxy();
  // 播放器快捷键（老板键冲突检测的比对源；与播放器同一份 store）
  const shortcutKeys = useShortcutsStore((s) => s.keys);

  // 草稿为 null 表示「未修改」，直接用服务端值派生，避免 effect 同步引发级联渲染
  const [draft, setDraft] = useState<Partial<Settings> | null>(null);
  const [proxyUrl, setProxyUrl] = useState<string | null>(null);
  // 缓存目录输入框：null = 用户没动过。未配置时直接展示默认位置，
  // 空串 = 用户主动清空（仍保留「空 = 跟随数据目录」的存储语义）。
  const [cacheDirDraft, setCacheDirDraft] = useState<string | null>(null);

  if (isPending || !loaded) {
    return <div className="text-muted-foreground p-6 text-sm">{t('common.loading')}</div>;
  }

  // 合并草稿与服务端值
  const current: Settings = { ...loaded, ...(draft ?? {}) };
  const effectiveProxyUrl = proxyUrl ?? current.proxy.url;
  // 未配置缓存目录时，输入框直接填默认位置；用户清空后保留空串。
  const effectiveCacheDir = cacheDirDraft ?? (current.cacheDir || defaultCacheDir || '');
  // 老板键 vs 播放器快捷键冲突（全局组合在应用内按下会同时触发两者）
  const bossConflicts = resolveBossKeyConflicts(
    current.bossKey,
    shortcutKeys,
    t('settings.keySpace'),
  );

  const patch = (next: Partial<Settings>) => setDraft((prev) => ({ ...(prev ?? {}), ...next }));
  const patchProxy = (next: Partial<Settings['proxy']>) =>
    setDraft((prev) => ({ ...(prev ?? {}), proxy: { ...current.proxy, ...next } }));

  const chooseDir = async () => {
    const picked = await appApi.selectFolder().catch(() => null);
    if (picked) patch({ downloadDir: picked });
  };

  const chooseCacheDir = async () => {
    const picked = await appApi.selectFolder().catch(() => null);
    if (picked) {
      setCacheDirDraft(picked);
      patch({ cacheDir: picked });
    }
  };

  const submit = () => {
    if (bossConflicts.length > 0) {
      // 冲突不拦截其他设置的保存意图，但老板键这档必须先改：全局组合
      // 在应用内会同时触发播放器动作，存了就是坏状态
      toast.error(t('settings.bossKeyConflictBlock'));
      return;
    }
    saveMutation.mutate(
      {
        ...current,
        cacheDir: effectiveCacheDir,
        proxy: { ...current.proxy, url: effectiveProxyUrl },
      },
      {
        onSuccess: () => {
          toast.success(t('settings.saved'));
          // 保存成功后丢弃草稿，回到「跟随服务端」状态
          setDraft(null);
          setProxyUrl(null);
          setCacheDirDraft(null);
        },
        // 失败必须有回音：后端报错（老板键被占用注册失败这类）静默的
        // 话，用户眼里就是「点了没反应」（v0.5.0 保存无反应排查实录）
        onError: (e) => toast.error(String(e)),
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
          <ToggleRow
            id="pause-on-minimize"
            label={t('settings.pauseOnMinimize')}
            checked={current.pauseOnMinimize}
            onChange={(v) => patch({ pauseOnMinimize: v })}
          />

          {/* 连播预取集数（issue #6）：0=关闭；保存即生效于下一次起播 */}
          <div className="flex flex-col gap-2">
            <Label htmlFor="prefetch-episodes">
              {t('settings.prefetchEpisodes')} —{' '}
              {current.prefetchEpisodes === 0
                ? t('settings.prefetchOff')
                : current.prefetchEpisodes}
            </Label>
            <Input
              id="prefetch-episodes"
              type="range"
              min={0}
              max={3}
              step={1}
              value={current.prefetchEpisodes}
              onChange={(e) => patch({ prefetchEpisodes: Number(e.target.value) })}
            />
            <p className="text-muted-foreground text-xs">{t('settings.prefetchEpisodesDesc')}</p>
          </div>

          {/* 老板键已独立成卡（全局快捷键 ≠ 播放器快捷键），见 BossKeyCard */}

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
          {/* 平台编码层不可用（ffmpeg 是兜底决定者）且确实没装时，才给一条
              可照抄的安装路径。平台可用的机器分流不会走 ffmpeg——装了也不
              改道，文案「装上后转码自动改走它」在那档机器上是假话，指引
              随探测分层一并隐藏（探测严格按优先级，见 capability/probe.rs） */}
          {capability && !capability.platformEncoder && !capability.hasFfmpeg && (
            <div className="bg-muted/50 flex flex-col gap-2 rounded-md p-3 text-xs">
              <p>{t('settings.ffmpegInstallHint')}</p>
              <div className="flex flex-wrap items-center gap-2">
                <code className="bg-background rounded px-2 py-1 font-mono">
                  {FFMPEG_INSTALL.cmd}
                </code>
                <Button
                  size="sm"
                  variant="ghost"
                  onClick={() => {
                    void navigator.clipboard
                      .writeText(FFMPEG_INSTALL.cmd)
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
                      .openExternalPage(FFMPEG_INSTALL.page)
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

      {/* 快捷键（前端偏好，立即生效不走保存按钮） */}
      <ShortcutCard />

      {/* 老板键（全局快捷键，issue #4）：与播放器快捷键分卡展示并做冲突
          检测——全局组合在应用内按下会同时触发播放器动作 */}
      <BossKeyCard
        bossKey={current.bossKey}
        conflicts={resolveBossKeyConflicts(current.bossKey, shortcutKeys, t('settings.keySpace'))}
        onChange={(v) => patch({ bossKey: v })}
      />

      {/* 存储 */}
      <Card>
        <CardHeader>
          <CardTitle className="text-base">{t('settings.storage')}</CardTitle>
        </CardHeader>
        <CardContent className="flex flex-col gap-3">
          {/* 缓存目录：未配置时输入框直接填默认位置（空存储 = 跟随数据目录）。
              保存即时生效于新写入，旧目录残留不迁移——与下载目录同语义。 */}
          <div className="flex flex-col gap-2">
            <Label htmlFor="cache-dir">{t('settings.cacheDir')}</Label>
            <div className="flex gap-2">
              <Input
                id="cache-dir"
                value={effectiveCacheDir}
                placeholder={t('settings.cacheDirPlaceholder')}
                onChange={(e) => {
                  setCacheDirDraft(e.target.value);
                  patch({ cacheDir: e.target.value });
                }}
                className="font-mono text-xs"
              />
              <Button variant="outline" onClick={() => void chooseCacheDir()}>
                <FolderOpen className="size-4" />
                {t('settings.choose')}
              </Button>
            </div>
            <p className="text-muted-foreground text-xs">{t('settings.cacheDirDesc')}</p>
          </div>
          <p className="text-sm tabular-nums">
            {usage
              ? tf('settings.storageUsage', { size: formatBytes(usage.bytes), files: usage.files })
              : t('common.loading')}
          </p>
          {/* 磁盘清理只在存储页提供，这里只读占用与缓存清理。
              三处缓存（在线内存流 / 转码 / 封面）合成一个入口：此前两个按钮
              都不报大小，cover-cache 更是从未有清理入口——封面图片只进不出。
              点击必须有回音：成功报清理条数与释放量，失败弹错误。 */}
          <div className="flex flex-wrap items-center gap-2">
            <p className="text-sm tabular-nums">
              {cacheUsage
                ? tf('settings.cacheUsage', {
                    size: formatBytes(cacheUsage.compat.bytes + cacheUsage.cover.bytes),
                    compat: formatBytes(cacheUsage.compat.bytes),
                    cover: formatBytes(cacheUsage.cover.bytes),
                    online: cacheUsage.online.files,
                  })
                : t('common.loading')}
            </p>
            <Button
              size="sm"
              variant="outline"
              className="ml-auto"
              disabled={clearCache.isPending}
              onClick={() =>
                clearCache.mutate(undefined, {
                  onSuccess: (r) =>
                    toast.success(
                      tf('settings.cacheCleared', { count: r.items, size: formatBytes(r.bytes) }),
                    ),
                  onError: (e: Error) => toast.error(e.message),
                })
              }
            >
              {clearCache.isPending ? t('common.loading') : t('settings.clearCache')}
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
