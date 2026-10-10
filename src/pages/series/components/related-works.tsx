/** 相关作品·系列区块：plan 接口第一格的展示与跳转。 */
import { useNavigate } from '@tanstack/react-router';
import { t } from '@/locales';
import type { RelatedItem } from '@/service/schema';
import { RelatedCard } from './related-card';

/**
 * 相关作品·系列（官方 plan 接口第一格）：同系列各季（第1季/第2季…）
 * 与同 IP 作品。没有相关作品就整块不渲染——它是增强项，不值得占一个
 * 错误位。
 */
export function RelatedWorks({ works }: { works: RelatedItem[] }) {
  const navigate = useNavigate();
  if (works.length === 0) return null;

  const open = (id: string) => {
    // 同一路由换 search 参数：整页数据随之换挡
    void navigate({ to: '/detail', search: { seriesId: id } });
  };

  return (
    <div className="grid gap-3 pb-4">
      <h3 className="text-sm font-semibold">{t('detail.relatedWorks')}</h3>
      {/* 自适应网格与猜你喜欢同一套：一行放不下自动换行，不出横向滚动条 */}
      <div className="grid grid-cols-[repeat(auto-fill,minmax(128px,1fr))] gap-3">
        {works.map((w) => (
          <RelatedCard key={w.seriesId} item={w} onOpen={open} />
        ))}
      </div>
    </div>
  );
}
