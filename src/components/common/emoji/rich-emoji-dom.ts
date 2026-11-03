/** 富文本输入框的 DOM 辅助：表情代码/提及段 ↔ 纯文本 的渲染/序列化纯函数。 */
import { parseEmojiSegments } from '@/utils/danmaku-emoji';

/** 光标锚点字符：垫在不可编辑 img / 提及 chip 之后给光标一个文本落点。
 *  必须是 \uFEFF 而非 \u200B——后者会被 macOS 输入法的组合文本误纳入；serialize 时剥掉。 */
export const CARET_ANCHOR = '\uFEFF';

/** renderValue 的渲染选项：`aiName` 是 AI bot 的用户名——提及 chip 对它
 *  追加官方同款渐变「Ai」后缀（纯展示，不参与序列化）。 */
export interface RenderValueOptions {
  aiName?: string;
}

/** 提及 chip 的展示元素：蓝色 @名字 + AI bot 加渐变 Ai 后缀。
 *  ⚠️ 名字进 data-mention（序列化还原的依据），Ai 后缀纯装饰。 */
function buildMentionChip(name: string, aiName?: string): HTMLElement {
  const span = document.createElement('span');
  span.dataset.mention = name;
  span.contentEditable = 'false';
  span.className = 'text-sky-400';
  span.append(`@${name}`);
  if (name === aiName) {
    const ai = document.createElement('span');
    ai.className =
      'bg-gradient-to-r from-amber-400 to-pink-500 bg-clip-text font-semibold text-transparent';
    ai.textContent = 'Ai';
    span.append(' ', ai);
  }
  return span;
}

/** 把含代码/提及的纯文本渲染进容器（文本节点 + 表情 img + 提及 chip，
 *  杜绝 innerHTML 注入）。 */
export function renderValue(el: HTMLElement, value: string, options?: RenderValueOptions): void {
  el.replaceChildren();
  for (const seg of parseEmojiSegments(value)) {
    if (seg.kind === 'text') {
      el.appendChild(document.createTextNode(seg.value));
    } else if (seg.kind === 'mention') {
      el.appendChild(buildMentionChip(seg.name, options?.aiName));
    } else {
      const img = document.createElement('img');
      img.src = seg.url;
      img.alt = seg.value;
      img.title = seg.value;
      img.draggable = false;
      img.contentEditable = 'false';
      el.appendChild(img);
    }
  }
}

/** DOM → 纯文本：表情 img 还原成 [名字]，提及 chip 还原成 `@名字\uFFFC`，
 *  其余元素递归取文本。 */
export function serialize(el: HTMLElement): string {
  let out = '';
  for (const node of el.childNodes) {
    if (node.nodeType === Node.TEXT_NODE) {
      // 光标锚点（见 CARET_ANCHOR）是垫给编辑光标的，不属于用户内容
      out += (node.textContent ?? '').replaceAll(CARET_ANCHOR, '');
    } else if (node instanceof HTMLImageElement) {
      out += node.alt;
    } else if (node instanceof HTMLElement && node.dataset.mention !== undefined) {
      out += `@${node.dataset.mention}\uFFFC`;
    } else if (node instanceof HTMLBRElement) {
      // 清空时浏览器可能残留 <br>，序列化为空并顺手清掉
      out += '';
    } else {
      out += node.textContent ?? '';
    }
  }
  return out;
}
