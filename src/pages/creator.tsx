import { lazy, Suspense } from 'react';
import { createFileRoute } from '@tanstack/react-router';
import { PendingFallback } from '@/components/common/route-fallbacks';

/**
 * 创作者主页（主演/出品方点进来的个人主页）。
 * userId 走 search 参数（详情页创作者条目自带的 user_id）。
 */
const CreatorHomePage = lazy(() =>
  import('@/pages/creator/components/creator-home-page').then((m) => ({
    default: m.CreatorHomePage,
  })),
);

function CreatorRoute() {
  const { userId } = Route.useSearch() as unknown as { userId: string };
  return (
    <Suspense fallback={<PendingFallback />}>
      <CreatorHomePage userId={userId} />
    </Suspense>
  );
}

export const Route = createFileRoute('/creator')({
  validateSearch: (search: Record<string, unknown>) => ({
    userId: typeof search.userId === 'string' ? search.userId : '',
  }),
  component: CreatorRoute,
});
