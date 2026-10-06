import { createFileRoute } from '@tanstack/react-router';
import { SeriesDetailPage } from '@/features/series/components/series-detail-page';

/**
 * 剧集详情页。seriesId 走 search 参数（可收藏/可分享的深链）：
 * 播放器左下 @剧名 与相关推荐卡片都跳到这里。
 */
export const Route = createFileRoute('/detail')({
  validateSearch: (search: Record<string, unknown>) => ({
    seriesId: typeof search.seriesId === 'string' ? search.seriesId : '',
  }),
  component: DetailRoute,
});

function DetailRoute() {
  const { seriesId } = Route.useSearch();
  return <SeriesDetailPage seriesId={seriesId} />;
}
