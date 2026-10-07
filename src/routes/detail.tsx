import { createFileRoute } from '@tanstack/react-router';
import { SeriesDetailPage } from '@/features/series/components/series-detail-page';

/**
 * 剧集详情页。seriesId 走 search 参数（可收藏/可分享的深链）：
 * 播放器左下 @剧名 与相关推荐卡片都跳到这里。
 */
export const Route = createFileRoute('/detail')({
  validateSearch: (search: Record<string, unknown>) => ({
    // Router 默认 search 序列化会把纯数字 JSON.parse 成 number：应用内跳转
    // 靠 stringify 的引号往返侥幸无事，分享/手输的裸数字深链会落到这——
    // 统一收成 string，深链才真的可分享
    seriesId:
      typeof search.seriesId === 'string'
        ? search.seriesId
        : typeof search.seriesId === 'number'
          ? String(search.seriesId)
          : '',
  }),
  component: DetailRoute,
});

function DetailRoute() {
  const { seriesId } = Route.useSearch();
  return <SeriesDetailPage seriesId={seriesId} />;
}
