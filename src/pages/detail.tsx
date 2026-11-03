import { lazy, Suspense } from 'react';
import { createFileRoute } from '@tanstack/react-router';
import { PendingFallback } from '@/components/common/route-fallbacks';

/**
 * 剧集详情页（seriesId 走 search 深链）。prefill = 来源页自带的档案快照：
 * 未上线剧解析分集必然失败，带 prefill 时渲染「即将上线」降级视图而非整页报错。
 * 包装组件常驻路由文件（拿得到 Route 做 useSearch），页面本体 lazy 切出去；
 * routeTree.gen 是 as any 生成物会污染 useSearch 返回类型，形状以 validateSearch 为准 double cast。
 */
const SeriesDetailPage = lazy(() =>
  import('@/pages/series/components/series-detail-page').then((m) => ({
    default: m.SeriesDetailPage,
  })),
);

function DetailRoute() {
  const { seriesId, title, cover, tags, desc } = Route.useSearch() as unknown as {
    seriesId: string;
    title: string;
    cover: string;
    tags: string;
    desc: string;
  };
  return (
    <Suspense fallback={<PendingFallback />}>
      <SeriesDetailPage
        seriesId={seriesId}
        prefill={title !== '' || cover !== '' ? { title, cover, tags, desc } : undefined}
      />
    </Suspense>
  );
}

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
