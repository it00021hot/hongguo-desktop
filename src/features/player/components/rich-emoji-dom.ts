/** 富文本输入框的 DOM 辅助：表情代码 ↔ 纯文本 的渲染/序列化纯函数。 */
import { parseEmojiSegments } from '@/utils/danmaku-emoji';

/** 把含代码的纯文本渲染进容器（文本节点 + 表情 img，杜绝 innerHTML 注入）。 */
export function renderValue(el: HTMLElement, value: string) {
  el.replaceChildren();
  for (const seg of parseEmojiSegments(value)) {
    if (seg.kind === 'text') {
      el.appendChild(document.createTextNode(seg.value));
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

/** DOM → 纯文本：表情 img 还原成 [名字]，其余元素递归取文本。 */
export function serialize(el: HTMLElement): string {
  let out = '';
  for (const node of el.childNodes) {
    if (node.nodeType === Node.TEXT_NODE) {
      // \u200B 是表情后的游标锚点（WebKit 对着不可编辑 img 放光标的垫片），
      // 不属于用户内容
      out += (node.textContent ?? '').replace(/\u200B/g, '');
    } else if (node instanceof HTMLImageElement) {
      out += node.alt;
    } else if (node instanceof HTMLBRElement) {
      // 清空时浏览器可能残留 <br>，序列化为空并顺手清掉
      out += '';
    } else {
      out += node.textContent ?? '';
    }
  }
  return out;
}
