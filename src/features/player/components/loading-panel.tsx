/** 无流（取流中/缓冲中/出错）时压在封面上的加载与重试面板（纯 props→JSX）。 */
import { Loader2 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { t, tf } from '@/locales';
import { formatBytes } from '@/utils/format';
import type { OnlineProgress } from '@/service/schema';

export function LoadingPanel({
  error,
  buffering,
  retryOnline,
}: {
  error: string | null;
  buffering: OnlineProgress | null;
  retryOnline: () => void;
}) {
  return (
    <div className="absolute inset-0 z-10 grid place-items-center p-6">
      {/* 缓冲时给的是「在动到哪了」，不是一个没头没尾的转圈；
        文案收进胶囊压在封面上。自动重试耗尽的错误态再给一个
        手动重试入口——用户不该只能眼看黑屏干着急。 */}
      <div className="flex flex-col items-center gap-3">
        <span className="flex items-center gap-2 rounded-full bg-black/60 px-4 py-1.5 text-xs text-white/85 backdrop-blur-sm">
          {!error && <Loader2 className="size-3.5 animate-spin" aria-hidden />}
          {error ??
            (buffering && buffering.phase !== 'ready'
              ? tf('player.buffering', {
                  percent: buffering.total > 0 ? Math.floor(buffering.percent) : 0,
                  size:
                    buffering.total > 0
                      ? `${formatBytes(buffering.received)} / ${formatBytes(buffering.total)}`
                      : formatBytes(buffering.received),
                })
              : t('common.loading'))}
        </span>
        {error && (
          <Button
            size="sm"
            variant="outline"
            data-wheel-block
            onClick={(e) => {
              e.stopPropagation();
              retryOnline();
            }}
          >
            {t('player.retry')}
          </Button>
        )}
      </div>
    </div>
  );
}
