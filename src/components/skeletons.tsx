import { Skeleton } from '@/components/ui/skeleton';

/**
 * 统一骨架屏：各页的加载占位从手拼 Array.from 收敛到这里，
 * 形状只有两种——封面网格（信息流/新剧/找剧）与横向行（榜单/日历/预约）。
 */

/** 封面卡片网格骨架（aspect-[3/4] 封面 + 标题/副标题两行）。 */
export function SkeletonCardGrid({ count = 10 }: { count?: number }) {
  return (
    <div className="grid grid-cols-2 gap-4 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 2xl:grid-cols-8">
      {Array.from({ length: count }, (_, i) => (
        <div key={i} className="flex flex-col gap-2">
          <Skeleton className="aspect-[3/4] w-full rounded-xl" />
          <Skeleton className="h-4 w-3/4" />
          <Skeleton className="h-3 w-1/2" />
        </div>
      ))}
    </div>
  );
}

/** 横向行骨架（height 直接落在 Skeleton 上，如 "h-24 rounded-xl"）。 */
export function SkeletonRows({ count = 6, height }: { count?: number; height: string }) {
  return (
    <div className="flex flex-col gap-3">
      {Array.from({ length: count }, (_, i) => (
        <Skeleton key={i} className={height} />
      ))}
    </div>
  );
}
