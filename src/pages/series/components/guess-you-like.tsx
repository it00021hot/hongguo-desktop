/** 猜你喜欢区块：plan 接口第二格的响应式封面网格。 */
import { useNavigate } from '@tanstack/react-router';
import { t } from '@/locales';
import type { RelatedItem } from '@/service/schema';
import { RelatedCard } from './related-card';

/**
 * 猜你喜欢（官方 plan 接口第二格，第三方详情页同款）：响应式封面网格，
 * 卡片与相关作品同一套（角标 + 评分 + 剧名 + 集数/播放量）。
 */
export function GuessYouLike({ items }: { items: RelatedItem[] }) {
  const navigate = useNavigate();
  const open = (id: string) => {
    void navigate({ to: '/detail', search: { seriesId: id } });
  };
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
