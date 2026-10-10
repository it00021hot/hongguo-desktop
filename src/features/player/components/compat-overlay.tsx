/** 兜底转码进度浮层：黑场盖层 + 阶段文案 + 进度条（纯 props→JSX）。 */
import { Progress } from '@/components/ui/progress';
import { t, tf } from '@/locales';
import type { CompatProgress } from '@/service/schema';

export function CompatOverlay({
  compat,
}: {
  // 形状与 use-transcode-fallback 的 compatProgress 状态一致（phase 那边是
  // 宽 string，不用 schema 的 CompatProgress 收窄它）
  compat: CompatProgress;
}) {
  return (
    <div className="absolute inset-0 z-30 grid place-items-center bg-black/85 p-6 text-center text-sm text-neutral-200">
      <div className="flex w-full max-w-sm flex-col items-center gap-3">
        <p>
          {compat.phase === 'downloading'
            ? t('player.compatFetching')
            : tf('player.compatTranscoding', { percent: Math.floor(compat.percent) })}
        </p>
        <Progress value={compat.phase === 'downloading' ? 0 : compat.percent} className="h-1.5" />
        <span className="text-xs text-neutral-400">{t('player.compatHint')}</span>
      </div>
    </div>
  );
}
