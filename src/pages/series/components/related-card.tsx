/** 相关作品卡片：封面（角标 + 评分）+ 两行剧名 + 集数/播放量。 */
import { SeriesCover } from '@/components/series-cover';
import { formatPlayCount } from '@/utils/format';
import { t, tf } from '@/locales';
import type { RelatedItem } from '@/service/schema';
import { ReserveButton } from './reserve-button';

export function RelatedCard({ item, onOpen }: { item: RelatedItem; onOpen: (id: string) => void }) {
  const isUpcoming = item.episodeCnt === 0 || item.tag === '即将上线';
  return (
    // 整张卡可点。不用 <button> 包：一是内容模型只允许 phrasing content
    // （卡里有 <p>，里面还可能嵌预约按钮），二是网格默认 stretch 拉齐行高时
    // button 会把内容**垂直居中**——标题折行数不同的卡内容高不同、下移量
    // 不同，整排一高一低。article + role="button" 与全站卡片网格同一做法，
    // flex-col 保证内容永远从顶上排。
    <article
      role="button"
      tabIndex={0}
      aria-label={item.title}
      onClick={() => onOpen(item.seriesId)}
      onKeyDown={(e) => {
        if (e.key !== 'Enter' && e.key !== ' ') return;
        // 空格默认会滚动页面，按钮不该有滚动副作用
        e.preventDefault();
        onOpen(item.seriesId);
      }}
      className="group flex w-full cursor-pointer flex-col text-left"
      title={item.videoDesc || item.title}
    >
      {/* 封面盒：宽度随格子、高度锁 3:4，图 object-cover 裁切——
          封面原始比例五花八门，绝不能让它撑盒子（一上一下就是这么来的） */}
      <div className="bg-muted relative aspect-[3/4] w-full overflow-hidden rounded-lg">
        {/* plan 接口的封面现已是 fqnovelpic HEIC 签名 URL（旧注释里的
            byteimg JPEG 不会再出现），直挂必裂，统一走 SeriesCover */}
        <SeriesCover cover={item.cover} alt={item.title} />
        {item.tag && (
          <span className="absolute top-1 left-1 rounded bg-black/50 px-1 py-0.5 text-[10px] leading-none text-white/95 backdrop-blur-[2px]">
            {item.tag}
          </span>
        )}
        {item.score > 0 && (
          <span className="absolute right-1 bottom-1 rounded bg-black/60 px-1 py-0.5 text-[10px] leading-none text-amber-300">
            {item.score.toFixed(1)}分
          </span>
        )}
      </div>
      <p className="mt-1.5 line-clamp-2 text-xs leading-snug">{item.title}</p>
      <p className="text-muted-foreground mt-0.5 truncate text-[11px]">
        {item.episodeCnt > 0
          ? tf('detail.episodesCount', { count: item.episodeCnt })
          : item.tag === '即将上线'
            ? item.tag
            : ''}
        {item.playCnt > 0 && ` · ${formatPlayCount(item.playCnt)}${t('detail.plays')}`}
      </p>
      {isUpcoming && <ReserveButton seriesId={item.seriesId} />}
    </article>
  );
}
