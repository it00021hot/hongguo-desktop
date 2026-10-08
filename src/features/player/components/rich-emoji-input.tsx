import { useEffect, useImperativeHandle, useRef } from 'react';
import { parseEmojiSegments } from '@/lib/danmaku-emoji';
import { cn } from '@/lib/utils';

/**
 * 富文本输入框：`[名字]` 表情代码即时渲染成内嵌图片（<input> 装不下图，
 * 用 contenteditable 实现）。value 仍是含代码的纯文本——提交/校验逻辑
 * 与普通输入框完全一致，只是「看」的方式变了。
 *
 * DOM 不归 React 管（受控重渲染会和光标打架）：组件自己维护——
 * - onInput 时把 DOM 序列化回纯文本上报；
 * - 仅当外部 value 与 DOM 不一致时（清空/外部赋值）才重建 DOM；
 * - 选择器插入走 ref.insertEmoji()：优先插在已记的光标处（面板打开时
 *   焦点在菜单里，输入框失焦，光标位置靠 selectionchange 持续记录），
 *   没有记录就追加到末尾。
 */

export interface RichEmojiInputHandle {
  insertEmoji: (name: string) => void;
  focus: () => void;
}

interface Props {
  value: string;
  onChange: (next: string) => void;
  onEnter?: () => void;
  onEscape?: () => void;
  placeholder: string;
  maxLength?: number;
  autoFocus?: boolean;
  className?: string;
  ref?: React.Ref<RichEmojiInputHandle>;
}

/** 把含代码的纯文本渲染进容器（文本节点 + 表情 img，杜绝 innerHTML 注入）。 */
function renderValue(el: HTMLElement, value: string) {
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
function serialize(el: HTMLElement): string {
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

export function RichEmojiInput({
  value,
  onChange,
  onEnter,
  onEscape,
  placeholder,
  maxLength,
  autoFocus,
  className,
  ref,
}: Props) {
  const elRef = useRef<HTMLDivElement | null>(null);
  /** 输入框最后持有的光标（失焦后仍可用于插入） */
  const savedRangeRef = useRef<Range | null>(null);
  /** 输入法组合中（拼音未上屏）：期间不做任何 DOM 接管，避免搅乱组合 */
  const composingRef = useRef(false);

  // 持续记录光标：表情面板打开时焦点被菜单拿走，插入要用这里存的位置
  useEffect(() => {
    const el = elRef.current;
    if (!el) return;
    const onSelChange = () => {
      const sel = window.getSelection();
      if (sel?.rangeCount && el.contains(sel.anchorNode)) {
        savedRangeRef.current = sel.getRangeAt(0).cloneRange();
      }
    };
    document.addEventListener('selectionchange', onSelChange);
    return () => document.removeEventListener('selectionchange', onSelChange);
  }, []);

  // 回复行的 autoFocus：div 上不可靠，挂载后显式聚焦
  useEffect(() => {
    if (autoFocus) elRef.current?.focus();
    // eslint-disable-next-line react-hooks/exhaustive-deps -- 只在挂载时聚焦一次
  }, []);

  // 外部 value 与 DOM 不一致才重建（清空 / 外部赋值）；自己打字引发的
  // onChange 回流是相等的，不动 DOM，光标才稳
  useEffect(() => {
    const el = elRef.current;
    if (!el) return;
    if (serialize(el) !== value) {
      renderValue(el, value);
      savedRangeRef.current = null;
    }
  }, [value]);

  /** DOM → value 同步：孤儿锚点清理/截断/滚动归位。输入法组合期间
   *  跳过一切会动 DOM 的步骤（重建会打断拼音组合），只上报序列化。 */
  const syncFromDom = () => {
    const el = elRef.current;
    if (!el) return;
    if (!composingRef.current) {
      // 孤儿锚点清理：零宽锚点的存在意义是「垫在 img 后面」，img 被
      // 浏览器默认行为删掉后锚点就成了幽灵字符
      for (const n of [...el.childNodes]) {
        if (
          n.nodeType === Node.TEXT_NODE &&
          n.textContent === '\u200B' &&
          !(n.previousSibling instanceof HTMLImageElement)
        ) {
          n.remove();
        }
      }
    }
    let next = serialize(el);
    if (!composingRef.current) {
      // 清空后残留的 <br> 会让 :empty 失效——序列化为空就归零 DOM
      if (next === '' && el.childNodes.length > 0) el.replaceChildren();
      if (maxLength !== undefined && next.length > maxLength) {
        next = next.slice(0, maxLength);
        renderValue(el, next);
      }
      // 内容不溢出时把滚动归位：光标自动滚动可能把盒子停在「开头被
      // 裁掉」的静止态（占位符/短文本显示成中间截断）
      if (el.scrollWidth <= el.clientWidth) el.scrollLeft = 0;
    }
    onChange(next);
  };

  useImperativeHandle(ref, () => ({
    insertEmoji: (name: string) => {
      const el = elRef.current;
      if (!el) return;
      const img = document.createElement('img');
      const seg = parseEmojiSegments(`[${name}]`).find((s) => s.kind === 'emoji');
      if (!seg || seg.kind !== 'emoji') return;
      img.src = seg.url;
      img.alt = seg.value;
      img.title = seg.value;
      img.draggable = false;
      img.contentEditable = 'false';

      const sel = window.getSelection();
      // 光标来源优先级：实时选区（字段持焦，配合选择器的 mousedown
      // preventDefault 焦点纪律，这是常态）> 记录的旧光标 > 末尾
      let range: Range | null = null;
      if (sel?.rangeCount && document.activeElement === el && el.contains(sel.anchorNode)) {
        range = sel.getRangeAt(0);
      } else if (
        savedRangeRef.current &&
        el.contains(savedRangeRef.current.commonAncestorContainer)
      ) {
        range = savedRangeRef.current;
      }
      if (!range) {
        range = document.createRange();
        range.selectNodeContents(el);
        range.collapse(false);
      }
      // 清空后残留的 <br> 会让占位符失效，先摘掉
      for (const node of [...el.childNodes]) {
        if (node instanceof HTMLBRElement) node.remove();
      }
      range.deleteContents();
      range.insertNode(img);
      // WebKit 痛点：光标紧贴不可编辑 img 时，后续输入会插到 img 前面——
      // 垫一个零宽空格作游标锚点（serialize 时剥掉，不进 value）
      const anchor = document.createTextNode('\u200B');
      img.after(anchor);
      range.setStart(anchor, 1);
      range.collapse(true);
      sel?.removeAllRanges();
      sel?.addRange(range);
      el.focus();
      savedRangeRef.current = range.cloneRange();

      let next = serialize(el);
      if (maxLength !== undefined && next.length > maxLength) next = next.slice(0, maxLength);
      if (serialize(el) !== next) renderValue(el, next);
      onChange(next);
    },
    focus: () => elRef.current?.focus(),
  }));

  return (
    <div
      ref={elRef}
      role="textbox"
      aria-label={placeholder}
      tabIndex={0}
      contentEditable
      suppressContentEditableWarning
      data-placeholder={placeholder}
      className={cn('rich-emoji-input outline-none', className)}
      onCompositionStart={() => {
        composingRef.current = true;
      }}
      onCompositionEnd={() => {
        composingRef.current = false;
        // 上屏后补一次完整同步（组合期间的 onChange 跳过了截断/清理）
        syncFromDom();
      }}
      onInput={() => syncFromDom()}
      onKeyDown={(e) => {
        // 输入法组合中：Enter 是「字母上屏」不是提交，退格是删拼音——
        // 全部交还输入法/默认行为，不做任何接管（isComposing 标准位 +
        // keyCode 229 老式双保险，WebKit 两者都会给）
        if (e.nativeEvent.isComposing || e.nativeEvent.keyCode === 229) return;
        // 表情是不可编辑的内嵌 img，紧贴它的退格/删除在 WebKit 里行为
        // 怪异：先无声吃掉零宽锚点（按一下没反应），再误伤相邻文字。
        // 这里接管成「按可见单位删」：img 连同它的锚点一次删干净。
        if (e.key === 'Backspace' || e.key === 'Delete') {
          const el = elRef.current;
          const sel = window.getSelection();
          if (!el || !sel || !sel.isCollapsed || sel.rangeCount === 0) return;
          const range = sel.getRangeAt(0);
          if (!el.contains(range.startContainer)) return;
          const { startContainer: c, startOffset: o } = range;

          /** 光标某一侧的 img 单位（img + 其后零宽锚点） */
          const imgAt = (node: Node | null): HTMLImageElement | null =>
            node instanceof HTMLImageElement ? node : null;
          let img: HTMLImageElement | null = null;
          let anchor: Text | null = null;
          /** 删完后光标落位（text 末尾 / 字段某下标），null = 原地不动 */
          let caret: { container: Node; offset: number } | null = null;

          if (e.key === 'Backspace') {
            if (c.nodeType === Node.TEXT_NODE) {
              const t = c as Text;
              if (o > 0 && t.data[o - 1] === '\u200B') {
                img = imgAt(t.previousSibling);
                anchor = img ? t : null;
                if (img) {
                  const before = img.previousSibling;
                  caret =
                    before && before.nodeType === Node.TEXT_NODE
                      ? { container: before, offset: (before as Text).data.length }
                      : { container: el, offset: [...el.childNodes].indexOf(img) };
                }
              } else if (o === 0) {
                img = imgAt(t.previousSibling);
                if (img) caret = { container: el, offset: [...el.childNodes].indexOf(img) };
              }
            } else if (c === el && o > 0) {
              img = imgAt(el.childNodes[o - 1] ?? null);
              if (img) caret = { container: el, offset: [...el.childNodes].indexOf(img) };
            }
          } else {
            // Delete（向前删）：紧前零宽锚点视作无物，单位是光标之后的 img
            if (c.nodeType === Node.TEXT_NODE) {
              const t = c as Text;
              if (o < t.data.length && t.data[o] === '\u200B') {
                img = imgAt(t.nextSibling);
                anchor = img ? t : null;
              } else if (o === t.data.length) {
                img = imgAt(t.nextSibling);
                if (img) anchor = imgAt(img.nextSibling) ? (img.nextSibling as Text) : null;
              }
            } else if (c === el && o < el.childNodes.length) {
              img = imgAt(el.childNodes[o] ?? null);
              if (img) anchor = imgAt(img.nextSibling) ? (img.nextSibling as Text) : null;
            }
          }

          if (img) {
            e.preventDefault();
            img.remove();
            anchor?.remove();
            if (caret) {
              const r = document.createRange();
              r.setStart(caret.container, Math.max(0, caret.offset));
              r.collapse(true);
              sel.removeAllRanges();
              sel.addRange(r);
              savedRangeRef.current = r.cloneRange();
            }
            const next = serialize(el);
            if (next === '' && el.childNodes.length > 0) el.replaceChildren();
            onChange(next);
          }
          // 没接管的（普通字符/扩选）走浏览器默认删除
          return;
        }
        if (e.key === 'Enter') {
          e.preventDefault();
          onEnter?.();
        }
        if (e.key === 'Escape') onEscape?.();
      }}
      onPaste={(e) => {
        // 只收纯文本：富文本粘贴会带进不可控的节点结构
        e.preventDefault();
        const text = e.clipboardData.getData('text/plain');
        if (!text) return;
        document.execCommand('insertText', false, text);
      }}
    />
  );
}
