import {
  Empty,
  EmptyContent,
  EmptyDescription,
  EmptyHeader,
  EmptyTitle,
} from '@/components/ui/empty';
import { Button } from '@/components/ui/button';
import { SkeletonRows } from '@/components/ui/skeletons';
import { ErrorState } from '@/components/common/error-state';
import { t } from '@/locales';

/**
 * 路由级兜底：RouteError（路由组件抛错，壳保留）/ RouteNotFound（深链不存在）/
 * PendingFallback（lazy chunk 加载中）；RouterCrashed 是路由体系之外的顶层渲染异常，给整页兜底 + 整页刷新。
 */

/** 路由组件抛错：保留应用壳，就地展示错误与重试。 */
export function RouteError({ error, reset }: { error: unknown; reset?: () => void }) {
  return (
    <div className="p-6">
      <ErrorState
        message={t('error.routeCrashed')}
        detail={error instanceof Error ? error.message : undefined}
        onRetry={reset}
      />
    </div>
  );
}

/** 深链不存在的路由。 */
export function RouteNotFound() {
  return (
    <Empty className="py-16">
      <EmptyHeader>
        <EmptyTitle className="text-muted-foreground text-sm font-normal">
          {t('error.notFound')}
        </EmptyTitle>
        <EmptyDescription className="text-xs opacity-70">
          {t('error.notFoundBody')}
        </EmptyDescription>
      </EmptyHeader>
    </Empty>
  );
}

/** lazy 页面 chunk 加载期间的骨架（行形态对多数页面都成立）。 */
export function PendingFallback() {
  return (
    <div className="p-6">
      <SkeletonRows count={6} height="h-28 rounded-xl" />
    </div>
  );
}

/** 路由体系之外的顶层渲染异常：整页兜底。 */
export function RendererCrashed({ error }: { error: unknown }) {
  return (
    <div className="bg-background grid h-screen w-screen place-items-center p-6">
      <div className="w-full max-w-md text-center">
        <EmptyHeader>
          <EmptyTitle className="text-base font-semibold">{t('error.routeCrashed')}</EmptyTitle>
          <EmptyDescription className="text-muted-foreground text-sm">
            {t('error.routeCrashedBody')}
          </EmptyDescription>
        </EmptyHeader>
        <EmptyContent>
          <Button variant="outline" onClick={() => window.location.reload()}>
            {t('feed.retry')}
          </Button>
          {error instanceof Error && error.message !== '' && (
            <EmptyDescription className="text-destructive text-xs">
              {error.message}
            </EmptyDescription>
          )}
        </EmptyContent>
      </div>
    </div>
  );
}
