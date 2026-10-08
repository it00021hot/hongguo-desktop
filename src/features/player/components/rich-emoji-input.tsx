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
      out += node.textContent ?? '';
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
      const range =
        savedRangeRef.current && el.contains(savedRangeRef.current.startContainer)
          ? savedRangeRef.current
          : null;
      if (range && document.activeElement === el && sel) {
        range.deleteContents();
        range.insertNode(img);
        range.setStartAfter(img);
        range.collapse(true);
        sel.removeAllRanges();
        sel.addRange(range);
      } else {
        // 失焦态（焦点在表情面板里）：先清掉残留光标节点（<br>），追加到末尾
        if (serialize(el) === '') el.replaceChildren();
        el.appendChild(img);
      }
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
      onInput={() => {
        const el = elRef.current;
        if (!el) return;
        let next = serialize(el);
        // 清空后残留的 <br> 会让 :empty 失效——序列化为空就归零 DOM
        if (next === '' && el.childNodes.length > 0) el.replaceChildren();
        if (maxLength !== undefined && next.length > maxLength) {
          next = next.slice(0, maxLength);
          renderValue(el, next);
        }
        // 内容不溢出时把滚动归位：光标自动滚动可能把盒子停在「开头被
        // 裁掉」的静止态（占位符/短文本显示成中间截断）
        if (el.scrollWidth <= el.clientWidth) el.scrollLeft = 0;
        onChange(next);
      }}
      onKeyDown={(e) => {
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
