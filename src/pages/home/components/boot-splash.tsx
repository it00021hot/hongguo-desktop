import { cn } from '@/lib/utils';

/** 冷启动加载态：feed 首批到位前的整页占位（居中 app 图标 + 呼吸光环，读作「启动中」而不是黑屏坏了）。 */
export function BootSplash({ className }: { className?: string }) {
  return (
    <div className={cn('flex h-full min-h-0 flex-col items-center justify-center', className)}>
      <div className="relative flex items-center justify-center">
        <div className="bg-primary/10 absolute size-28 animate-pulse rounded-[2rem]" />
        <img
          src="/app-icon.png"
          alt=""
          draggable={false}
          className="relative size-20 rounded-2xl shadow-lg select-none"
        />
      </div>
    </div>
  );
}
