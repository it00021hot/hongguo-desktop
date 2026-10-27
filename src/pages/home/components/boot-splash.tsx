import { cn } from '@/lib/utils';

/**
 * 冷启动加载态：feed 首批到位前的整页占位。
 *
 * 之前这里是「骨架黑砖」——暗色主题下一整块深灰 Skeleton 被感知成黑屏
 * （启动黑屏反馈）。换成居中 app 图标 + 呼吸光环的启动画面，
 * 同样是等待，读作「启动中」而不是「坏了」。语义令牌取色，无文案
 * （不进 i18n）。
 */
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
