import { DANMAKU_EMOJI_FILES } from './danmaku-emoji-files.generated';

/**
 * 弹幕表情（对齐 hgplayer 1.1.6）：上游文本里表情以 `[名字]` 代码出现，
 * 渲染时按映射表换成打包进应用的 webp 图片——评论/弹幕同源，评论区
 * 以后要渲染表情也走这里。
 */

// 文件名 → 打包后的资源 URL（构建期哈希，可直接当 img src 用）
const URLS = import.meta.glob('../assets/emoji/*.webp', {
  eager: true,
  import: 'default',
}) as Record<string, string>;

/** `[名字]` 代码：1–8 个非空白字符（hgplayer 同一款正则）。 */
const EMOJI_RE = /\[[^\][\s]{1,8}\]/g;

/** 名字 → 图片 URL。 */
const EMOJI_BY_NAME = new Map(
  DANMAKU_EMOJI_FILES.map(([name, file]) => [name, URLS[`../assets/emoji/${file}`] ?? '']),
);

/** 一段解析结果：纯文本或一张表情图。 */
export type EmojiSegment =
  { kind: 'text'; value: string } | { kind: 'emoji'; value: string; url: string };

/** 文本 → 段序列：认识的 `[名字]` 换表情，不认识的原样保留为文本。 */
export function parseEmojiSegments(text: string): EmojiSegment[] {
  const segments: EmojiSegment[] = [];
  let cursor = 0;
  for (const match of text.matchAll(EMOJI_RE)) {
    const url = EMOJI_BY_NAME.get(match[0]);
    if (!url) continue;
    if (match.index > cursor) {
      segments.push({ kind: 'text', value: text.slice(cursor, match.index) });
    }
    segments.push({ kind: 'emoji', value: match[0], url });
    cursor = match.index + match[0].length;
  }
  if (cursor < text.length) {
    segments.push({ kind: 'text', value: text.slice(cursor) });
  }
  return segments;
}

/** 表情选择器的条目（名字 + URL），按 hgplayer 同序。 */
export const DANMAKU_EMOJI_LIST = DANMAKU_EMOJI_FILES.map(([name, file]) => ({
  name,
  url: URLS[`../assets/emoji/${file}`] ?? '',
})).filter((e) => e.url !== '');

/**
 * 受控输入框的光标处插入 `[名字]` 代码（hgplayer insertAtCursor 同款）：
 * value 先更新，rAF 等受控重渲染完成后再把焦点与光标落回插入点。
 */
export function insertEmojiCode(
  el: HTMLInputElement | HTMLTextAreaElement | null,
  current: string,
  name: string,
  setValue: (next: string) => void,
): void {
  const code = `[${name}]`;
  const start = el?.selectionStart ?? current.length;
  const end = el?.selectionEnd ?? current.length;
  setValue(current.slice(0, start) + code + current.slice(end));
  requestAnimationFrame(() => {
    if (!el) return;
    el.focus();
    const pos = start + code.length;
    el.setSelectionRange(pos, pos);
  });
}
