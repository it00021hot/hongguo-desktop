import { useCallback, useRef, useState } from 'react';
import { Link } from '@tanstack/react-router';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { offlineQa } from '@/utils/offline-qa';
import { transcode } from '@/service/commands';
import { useEvent } from '@/service/tauri/events';
import { EVENTS } from '@/service/tauri/types';
import type { CompatProgress } from '@/service/schema';
import { useGrowingCompat } from '@/features/player/hooks/use-growing-compat';
import {
  flushPendingProducerCancel,
  wholeFileSrc,
} from '@/features/player/hooks/growing-fallback';

/** 与后端 qa_offline_local_play 固定缓存键一致 */
export const QA_OFFLINE_LOCAL_SERIES = 'qa-offline-local';
export const QA_OFFLINE_LOCAL_VID = 1;
const EPISODE_KEY = `${QA_OFFLINE_LOCAL_SERIES}:${QA_OFFLINE_LOCAL_VID}`;

/**
 * 仅 HONGGUO_OFFLINE_QA=1 可见：本地明文样本 → 兼容转码首段 + MSE。
 * 不进 BottomNav；默认模式调用方不得渲染本组件。
 */
export function OfflineQaLocalPlayPanel() {
  if (!offlineQa()) return null;
  return <OfflineQaLocalPlayPanelInner />;
}

function OfflineQaLocalPlayPanelInner() {
  const videoRef = useRef<HTMLVideoElement | null>(null);
  const [filePath, setFilePath] = useState(
    '/Users/milo/qa-hongguo-offline-data/inputs/hg-bytevc2-plain.mp4',
  );
  const [busy, setBusy] = useState(false);
  const [url, setUrl] = useState<string | null>(null);
  const [progressive, setProgressive] = useState(false);
  const [status, setStatus] = useState('待命');
  const [compatProgress, setCompatProgress] = useState<CompatProgress | null>(null);
  const [waitingForWhole, setWaitingForWhole] = useState(false);
  const [firstFrameAt, setFirstFrameAt] = useState<string | null>(null);

  useEvent(
    EVENTS.compatPlayProgress,
    useCallback((p: CompatProgress) => {
      if (p.key !== EPISODE_KEY) return;
      setCompatProgress(p);
      if (p.phase === 'completed') {
        setProgressive(false);
        setStatus(`转码完成 ${p.percent.toFixed(0)}%`);
      } else if (p.phase === 'failed') {
        setStatus('转码失败');
        setUrl(null);
        setProgressive(false);
      } else {
        setStatus(`${p.phase} ${p.percent.toFixed(0)}%`);
      }
    }, []),
  );

  const noteGrowingUnavailable = useCallback(() => {
    setWaitingForWhole(true);
    setStatus('MSE 不可用/失败，等待整片');
  }, []);

  const producer =
    compatProgress?.phase === 'failed'
      ? ('failed' as const)
      : compatProgress?.phase === 'completed'
        ? ('completed' as const)
        : progressive
          ? ('running' as const)
          : ('completed' as const);
  const wholeSrc = wholeFileSrc({
    episodeKey: EPISODE_KEY,
    eventKey: compatProgress?.key ?? (url ? EPISODE_KEY : null),
    producer,
    completeUrl: url,
    waiting: waitingForWhole || compatProgress?.phase === 'completed',
  });
  const playSrc = url;
  const showCompleteSrc = wholeSrc != null || (!progressive && !waitingForWhole);

  useGrowingCompat(
    videoRef,
    progressive ? playSrc : null,
    QA_OFFLINE_LOCAL_SERIES,
    QA_OFFLINE_LOCAL_VID,
    noteGrowingUnavailable,
  );

  const onFirstFrame = useCallback(() => {
    setFirstFrameAt((prev) => prev ?? new Date().toISOString());
  }, []);

  const start = async () => {
    setBusy(true);
    setFirstFrameAt(null);
    setWaitingForWhole(false);
    setCompatProgress(null);
    setUrl(null);
    setProgressive(false);
    setStatus('启动兼容转码…');
    try {
      // 丢掉挂起的 remount 推迟 cancel，再显式停旧任务；稍等让 grow 槽位落到 Cancelled
      flushPendingProducerCancel(QA_OFFLINE_LOCAL_SERIES, QA_OFFLINE_LOCAL_VID);
      await Promise.resolve(
        transcode.cancelCompat({
          seriesId: QA_OFFLINE_LOCAL_SERIES,
          vidIndex: QA_OFFLINE_LOCAL_VID,
        }),
      );
      await new Promise((r) => setTimeout(r, 120));
      const result = await transcode.qaOfflineLocalPlay(filePath.trim());
      // 同 path 的 hongguo-local URL 不变时仍要换 key，强制 video/MSE 重建
      const bust = result.progressive ? `${result.url}${result.url.includes('?') ? '&' : '?'}r=${Date.now()}` : result.url;
      setUrl(bust);
      setProgressive(Boolean(result.progressive));
      setStatus(
        result.cached
          ? `缓存命中 (${result.backend})`
          : `首段就绪 progressive=${result.progressive} backend=${result.backend} ${result.elapsedMs}ms`,
      );
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      setStatus(`失败: ${msg}`);
      toast.error(msg);
    } finally {
      setBusy(false);
    }
  };

  return (
    <Card data-testid="qa-offline-local-play">
      <CardHeader>
        <CardTitle className="text-base">离线 QA：本地样本首帧 / MSE</CardTitle>
      </CardHeader>
      <CardContent className="flex flex-col gap-3">
        <p className="text-muted-foreground text-xs">
          仅 HONGGUO_OFFLINE_QA=1 可见。走兼容转码首段 + hongguo-local Range / MSE，不改
          BottomNav。空路径默认 DATA_DIR/inputs/hg-bytevc2-plain.mp4。
        </p>
        <div className="flex flex-col gap-1.5">
          <Label htmlFor="qa-offline-local-path">本地明文路径</Label>
          <Input
            id="qa-offline-local-path"
            value={filePath}
            onChange={(e) => setFilePath(e.target.value)}
            placeholder="空=DATA_DIR/inputs/hg-bytevc2-plain.mp4"
          />
        </div>
        <div className="flex flex-wrap gap-2">
          <Button type="button" onClick={() => void start()} disabled={busy}>
            {busy ? '转码中…' : '播本地样本'}
          </Button>
          <Button type="button" variant="outline" asChild>
            <Link to="/qa-offline-local">打开专用页</Link>
          </Button>
        </div>
        <p className="text-muted-foreground text-xs" data-testid="qa-offline-status">
          状态：{status}
          {firstFrameAt ? ` · 首帧 ${firstFrameAt}` : ''}
        </p>
        <div className="relative aspect-video overflow-hidden rounded-md bg-black">
          {playSrc ? (
            <video
              ref={videoRef}
              key={playSrc}
              src={showCompleteSrc ? playSrc : undefined}
              className="absolute inset-0 size-full object-contain"
              controls
              autoPlay
              playsInline
              onPlaying={onFirstFrame}
              onLoadedData={onFirstFrame}
              onTimeUpdate={(e) => {
                if (e.currentTarget.currentTime > 0.05) onFirstFrame();
              }}
            />
          ) : (
            <div className="text-muted-foreground absolute inset-0 grid place-items-center text-sm">
              未起播
            </div>
          )}
        </div>
      </CardContent>
    </Card>
  );
}
