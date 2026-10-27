/** 猜你喜欢区块：plan 接口第二格的响应式封面网格。 */
import { useNavigate } from '@tanstack/react-router';
import { t } from '@/locales';
import type { RelatedItem } from '@/service/schema';
import { RelatedCard, relatedDetailSearch } from './related-card';

/**
 * 猜你喜欢（官方 plan 接口第二格，官方客户端详情页同款）：响应式封面网格，
 * 卡片与相关作品同一套（角标 + 评分 + 剧名 + 集数/播放量）。
 */
export function GuessYouLike({ items }: { items: RelatedItem[] }) {
  const navigate = useNavigate();
  // 带 prefill：未上线剧靠档案快照渲染「即将上线」降级视图，缺了会整页报错
  const open = (item: RelatedItem) =>
    void navigate({ to: '/detail', search: relatedDetailSearch(item) });
  return (
    <div className="grid gap-3">
      <h3 className="text-sm font-semibold">{t('detail.guessYouLike')}</h3>
      <div className="grid grid-cols-[repeat(auto-fill,minmax(128px,1fr))] gap-3">
        {items.map((w) => (
          <RelatedCard key={w.seriesId} item={w} onOpen={open} />
        ))}
      </div>
    </div>
  );
}
