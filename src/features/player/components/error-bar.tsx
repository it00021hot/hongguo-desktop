/** 页面级错误提示条：画面右上角的浮层胶囊（纯 props→JSX）。 */
import { cn } from '@/lib/utils';

/** 错误条压在画面右上角（左上是应用名、左下是剧名信息层，右上只在评论区打开时让位）；跟随悬浮层淡出。 */
export function ErrorBar({ chromeShown, error }: { chromeShown: boolean; error: string | null }) {
  return (
    <div
      data-wheel-block
      className={cn(
        'absolute top-3 right-3 z-20 flex items-center gap-2',
        'transition-opacity duration-300',
        chromeShown ? 'opacity-100' : 'pointer-events-none opacity-0',
      )}
    >
      {error && (
        <span className="rounded-full border border-red-500/30 bg-red-950/90 px-3 py-1 text-xs text-red-200">
          {error}
        </span>
      )}
    </div>
  );
}
