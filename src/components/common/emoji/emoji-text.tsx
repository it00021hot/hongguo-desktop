/** 表情文本渲染件：把 `[名字]` 表情代码渲染成内嵌表情图（评论区展示用）。 */
import { parseEmojiSegments } from '@/utils/danmaku-emoji';

/** 评论/回复文本：`[名字]` 表情代码渲染成图，
 *  其余文本原样保留。 */
export function EmojiText({ text }: { text: string }) {
  return (
    <>
      {parseEmojiSegments(text).map((seg, i) => {
        if (seg.kind === 'emoji') {
          return (
            <img
              key={i}
              src={seg.url}
              alt={seg.value}
              title={seg.value}
              draggable={false}
              className="mx-px inline-block size-[1.15em] object-contain align-[-0.18em]"
            />
          );
        }
        if (seg.kind === 'mention') {
          // 回复/正文里的提及段（@名字\uFFFC）：官方同款蓝色，不带 Ai 后缀
          // （是否 AI 由调用方按名字判断，这里拿不到候选数据）
          return (
            <span key={i} className="mr-1 text-sky-400">
              @{seg.name}
            </span>
          );
        }
        return <span key={i}>{seg.value}</span>;
      })}
    </>
  );
}
