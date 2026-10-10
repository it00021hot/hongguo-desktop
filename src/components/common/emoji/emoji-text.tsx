/** 表情文本渲染件：把 `[名字]` 表情代码渲染成内嵌表情图（评论区展示用）。 */
import { parseEmojiSegments } from '@/utils/danmaku-emoji';

/** 评论/回复文本：`[名字]` 表情代码渲染成图（hgplayer EmojiText 同款），
 *  其余文本原样保留。 */
export function EmojiText({ text }: { text: string }) {
  return (
    <>
      {parseEmojiSegments(text).map((seg, i) =>
        seg.kind === 'text' ? (
          <span key={i}>{seg.value}</span>
        ) : (
          <img
            key={i}
            src={seg.url}
            alt={seg.value}
            title={seg.value}
            draggable={false}
            className="mx-px inline-block size-[1.15em] object-contain align-[-0.18em]"
          />
        ),
      )}
    </>
  );
}
