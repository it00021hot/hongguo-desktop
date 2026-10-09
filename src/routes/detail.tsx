import { createFileRoute } from '@tanstack/react-router';
import { SeriesDetailPage } from '@/features/series/components/series-detail-page';

/**
 * 剧集详情页。seriesId 走 search 参数（可收藏/可分享的深链）：
 * 播放器左下 @剧名 与相关推荐卡片都跳到这里。
 *
 * prefill（title/cover/tags/desc）：来源页（榜单行等）自带的档案快照。
 * 未上线剧解析分集必然失败（平台无分集可给），带 prefill 时详情页用
 * 它渲染「即将上线」降级视图，而不是整页报错。
 */
export const Route = createFileRoute('/detail')({
  validateSearch: (search: Record<string, unknown>) => ({
    // seriesId 一律字符串（main.tsx 的 parseSearch 已按字符串保真，
    // 19 位纯数字 id 不会丢精度）；缺失给空串，由详情页指路
    seriesId: typeof search.seriesId === 'string' ? search.seriesId : '',
    title: typeof search.title === 'string' ? search.title : '',
    cover: typeof search.cover === 'string' ? search.cover : '',
    tags: typeof search.tags === 'string' ? search.tags : '',
    desc: typeof search.desc === 'string' ? search.desc : '',
  }),
  component: DetailRoute,
});

function DetailRoute() {
  const { seriesId, title, cover, tags, desc } = Route.useSearch();
  return (
    <SeriesDetailPage
      seriesId={seriesId}
      prefill={title !== '' || cover !== '' ? { title, cover, tags, desc } : undefined}
    />
  );
}
