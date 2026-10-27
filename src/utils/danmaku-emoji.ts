import { DANMAKU_EMOJI_FILES } from './danmaku-emoji-files.generated';

/**
 * 弹幕表情：上游文本里表情以 `[名字]` 代码出现，
 * 渲染时按映射表换成打包进应用的 webp 图片——评论/弹幕同源，评论区
 * 以后要渲染表情也走这里。
 */

// 文件名 → 打包后的资源 URL（构建期哈希，可直接当 img src 用）
const URLS = import.meta.glob<string>('../assets/emoji/*.webp', {
  eager: true,
  import: 'default',
});

/** `[名字]` 代码：1–8 个非空白字符。 */
const EMOJI_RE = /\[[^\][\s]{1,8}\]/g;

/** 名字 → 图片 URL。 */
const EMOJI_BY_NAME = new Map(
  DANMAKU_EMOJI_FILES.map(([name, file]) => [name, URLS[`../assets/emoji/${file}`] ?? '']),
);

/**
 * 一段解析结果：纯文本、一张表情图，或一个 @ 提及段。
 * 提及段原文形如 `@名字\uFFFC`（U+FFFC 是提及段占位符，服务端契约），
 * `value` 保留原文供序列化还原，`name` 是纯名字供渲染。
 */
export type EmojiSegment =
  | { kind: 'text'; value: string }
  | { kind: 'emoji'; value: string; url: string }
  | { kind: 'mention'; value: string; name: string };

/** 文本 → 段序列：`@名字\uFFFC` 提及段、认识的 `[名字]` 换表情，其余原样保留为文本。 */
export function parseEmojiSegments(text: string): EmojiSegment[] {
  const segments: EmojiSegment[] = [];
  let cursor = 0;
  for (const match of text.matchAll(EMOJI_RE)) {
    const url = EMOJI_BY_NAME.get(match[0]);
    if (!url) continue;
    if (match.index > cursor) {
      segments.push(...parseTextWithMentions(text.slice(cursor, match.index)));
    }
    segments.push({ kind: 'emoji', value: match[0], url });
    cursor = match.index + match[0].length;
  }
  if (cursor < text.length) {
    segments.push(...parseTextWithMentions(text.slice(cursor)));
  }
  return segments;
}

/** 纯文本里的提及段拆分（`@名字\uFFFC`；U+FFFC 结尾，名字不含空白）。 */
function parseTextWithMentions(text: string): EmojiSegment[] {
  const segments: EmojiSegment[] = [];
  let cursor = 0;
  for (const match of text.matchAll(/@[^\s\uFFFC]+\uFFFC/g)) {
    if (match.index > cursor) {
      segments.push({ kind: 'text', value: text.slice(cursor, match.index) });
    }
    segments.push({ kind: 'mention', value: match[0], name: match[0].slice(1, -1) });
    cursor = match.index + match[0].length;
  }
  if (cursor < text.length) {
    segments.push({ kind: 'text', value: text.slice(cursor) });
  }
  return segments;
}

/** 表情选择器的条目（名字 + URL），与映射表同序。 */
export const DANMAKU_EMOJI_LIST = DANMAKU_EMOJI_FILES.map(([name, file]) => ({
  name,
  url: URLS[`../assets/emoji/${file}`] ?? '',
})).filter((e) => e.url !== '');
