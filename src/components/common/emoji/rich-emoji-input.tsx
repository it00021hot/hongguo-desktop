import { useCallback, useEffect, useImperativeHandle, useRef } from 'react';
import { parseEmojiSegments } from '@/utils/danmaku-emoji';
import { isImeComposing } from '@/utils/ime';
import { cn } from '@/lib/utils';
import { CARET_ANCHOR, renderValue, serialize } from './rich-emoji-dom';

/**
 * 富文本输入框：`[名字]` 表情代码即时渲染成内嵌图片（contenteditable 实现），value 仍是含代码的纯文本。
 * DOM 不归 React 管（受控重渲染会和光标打架）：onInput 序列化上报；仅外部 value 与 DOM 不一致时才重建；
 * 选择器插入走 ref.insertEmoji()——优先插在已记光标处（靠 selectionchange 持续记录），没有记录就追加末尾。
 */

export interface RichEmojiInputHandle {
  insertEmoji: (name: string) => void;
  /** 在光标处插入 @ 提及段（吞掉触发词后插入 `@名字\uFFFC `） */
  insertMention: (name: string) => void;
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
  /** AI bot 用户名：提及 chip 对它追加官方同款渐变「Ai」后缀（纯展示） */
  mentionAiName?: string;
  ref?: React.Ref<RichEmojiInputHandle>;
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
  mentionAiName,
  ref,
}: Props) {
  const elRef = useRef<HTMLDivElement | null>(null);
  /** 输入框最后持有的光标（失焦后仍可用于插入） */
  const savedRangeRef = useRef<Range | null>(null);
  /** 输入法组合中（拼音未上屏）：期间不做任何 DOM 接管，避免搅乱组合 */
  const composingRef = useRef(false);
  /** 带 Ai 名的渲染（提及 chip 展示用，见 rich-emoji-dom） */
  const render = useCallback(
    (v: string) => renderValue(elRef.current!, v, { aiName: mentionAiName }),
    [mentionAiName],
  );

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
  // onChange 回流是相等的，不动 DOM，光标才稳。重建期间字段若持焦
  // （如选中 @ 候选后的整体替换），把光标放回末尾——开头落点会让
  // 用户接续输入插到句子中间。⚠️ IME 组合期间绝不接管 DOM（重建会
  // 打断拼音组合），组合结束后 syncFromDom 的 onChange 自然对账。
  useEffect(() => {
    const el = elRef.current;
    if (!el || composingRef.current) return;
    if (serialize(el) !== value) {
      render(value);
      savedRangeRef.current = null;
      if (document.activeElement === el) {
        const r = document.createRange();
        const last = el.lastChild;
        if (last instanceof Text) {
          // 光标落进**最后一个文本节点内部**——parent/childIndex 形态的
          // 选区会与编辑光标脱节：后续输入替换末尾字符而非追加（实测
          // 提及段的尾随空格被第一个正文替换掉）
          r.setStart(last, last.data.length);
        } else {
          r.selectNodeContents(el);
        }
        r.collapse(true);
        const sel = window.getSelection();
        sel?.removeAllRanges();
        sel?.addRange(r);
      }
    }
  }, [value, render]);

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
          n.textContent === CARET_ANCHOR &&
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
        render(next);
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
      // 垫一个零宽锚点作游标落点（serialize 时剥掉，不进 value）
      const anchor = document.createTextNode(CARET_ANCHOR);
      img.after(anchor);
      range.setStart(anchor, 1);
      range.collapse(true);
      sel?.removeAllRanges();
      sel?.addRange(range);
      el.focus();
      savedRangeRef.current = range.cloneRange();

      let next = serialize(el);
      if (maxLength !== undefined && next.length > maxLength) next = next.slice(0, maxLength);
      if (serialize(el) !== next) render(next);
      onChange(next);
    },
    insertMention: (name: string) => {
      const el = elRef.current;
      if (!el) return;
      const span = document.createElement('span');
      // chip 展示与序列化同源（data-mention 还原 `@名字\uFFFC`）
      span.dataset.mention = name;
      span.contentEditable = 'false';
      span.className = 'text-sky-400';
      span.append(`@${name}`);
      if (name === mentionAiName) {
        const ai = document.createElement('span');
        ai.className =
          'bg-gradient-to-r from-amber-400 to-pink-500 bg-clip-text font-semibold text-transparent';
        ai.textContent = 'Ai';
        span.append(' ', ai);
      }

      const sel = window.getSelection();
      // 光标来源优先级与 insertEmoji 相同：实时选区 > 记录的旧光标 > 末尾
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
      // 向光标前方吞掉本次 @ 触发词（`@` + 连续非空白/非@/非占位段）
      const start = range.startContainer;
      if (start.nodeType === Node.TEXT_NODE) {
        const data = (start as Text).data;
        let i = Math.min(range.startOffset, data.length);
        while (i > 0 && !/[\s@￼]/.test(data[i - 1]!)) i -= 1;
        if (i > 0 && data[i - 1] === '@') {
          range.setStart(start, i - 1);
        }
      }
      range.deleteContents();
      range.insertNode(span);
      // chip 后带一个可见空格（官方同款：提及与正文的间隔空隙），再垫
      // 零宽锚点稳住光标（WebKit 紧贴不可编辑单元时后续输入会插到前面）；
      // serialize 剥掉锚点、保留空格 → value 为 `@名字\uFFFC `
      const anchor = document.createTextNode(` ${CARET_ANCHOR}`);
      span.after(anchor);
      range.setStart(anchor, 1);
      range.collapse(true);
      sel?.removeAllRanges();
      sel?.addRange(range);
      el.focus();
      savedRangeRef.current = range.cloneRange();

      let next = serialize(el);
      if (maxLength !== undefined && next.length > maxLength) next = next.slice(0, maxLength);
      if (serialize(el) !== next) render(next);
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
        if (isImeComposing(e.nativeEvent)) return;

        const el = elRef.current;
        const sel = window.getSelection();
        if (!el || !sel || !sel.isCollapsed || sel.rangeCount === 0) return;
        const range = sel.getRangeAt(0);
        if (!el.contains(range.startContainer)) return;
        const { startContainer: c, startOffset: o } = range;
        /** 不可编辑的原子内联单元：表情 img / 提及 chip，连同其后的
         *  零宽锚点视作一个单位参与光标与删除 */
        const atomicAt = (node: Node | null): HTMLElement | null =>
          node instanceof HTMLImageElement ||
          (node instanceof HTMLElement && node.dataset.mention !== undefined)
            ? node
            : null;
        const isAnchor = (s: string | undefined) => s === CARET_ANCHOR;
        const setCaret = (container: Node, offset: number) => {
          e.preventDefault();
          const r = document.createRange();
          r.setStart(container, Math.max(0, offset));
          r.collapse(true);
          sel.removeAllRanges();
          sel.addRange(r);
          savedRangeRef.current = r.cloneRange();
        };

        // ←→ 方向键：把光标位展平成有序槽位——文本逐字符一位；表情
        // img + 其后零宽锚点合起来只给前/后两个边界位（锚点内部不放
        // 停靠位，原生移动会卡在不可见的位置上），方向键只做「取相邻
        // 槽位」，不再依赖 WebKit 的原生移动
        if (e.key === 'ArrowLeft' || e.key === 'ArrowRight') {
          const dir = e.key === 'ArrowRight' ? 1 : -1;
          const slots: { container: Node; offset: number }[] = [];
          for (const node of el.childNodes) {
            if (node instanceof Text) {
              if (node.data.length === 0) continue;
              for (let i = 0; i <= node.data.length; i++) {
                // 紧贴零宽锚点之后的位置不可见，不放停靠位
                if (i > 0 && i < node.data.length && node.data.charAt(i - 1) === CARET_ANCHOR) {
                  continue;
                }
                slots.push({ container: node, offset: i });
              }
            } else if (atomicAt(node)) {
              const ns = node.nextSibling;
              if (!(ns instanceof Text && ns.data.charAt(0) === CARET_ANCHOR)) {
                const idx = [...el.childNodes].indexOf(node);
                slots.push({ container: el, offset: idx });
                slots.push({ container: el, offset: idx + 1 });
              }
              // 有锚点：前/后边界位由锚点文本节点的循环给出
            }
          }
          const cmp = (s: { container: Node; offset: number }) => {
            const r = document.createRange();
            r.setStart(s.container, s.offset);
            r.collapse(true);
            return r.compareBoundaryPoints(Range.START_TO_START, range);
          };
          let target: { container: Node; offset: number } | undefined;
          const exact = slots.findIndex(
            (s) => s.container === range.startContainer && s.offset === range.startOffset,
          );
          if (exact >= 0) {
            target = slots[exact + dir];
          } else if (dir === 1) {
            // 光标停在槽位之间的不可见位置：取右方第一个槽位
            target = slots.find((s) => cmp(s) > 0);
          } else {
            const reversed = [...slots].reverse();
            target = reversed.find((s) => cmp(s) < 0);
          }
          // 端点或无槽位：光标原地（原生也到不了更远）
          if (target) setCaret(target.container, target.offset);
          else e.preventDefault();
          return;
        }

        // Backspace/Delete：原子单元（img/提及 chip）连同锚点零宽视作
        // 一个可见单位整块删，光标落在删减后的正确位置；普通字符与扩选
        // 删除走浏览器默认
        if (e.key === 'Backspace' || e.key === 'Delete') {
          const forward = e.key === 'Delete';
          let atomic: HTMLElement | null = null;
          /** 需要摘掉的零宽字符（节点 + 下标） */
          let anchor: { node: Text; index: number } | null = null;
          /** 删完后光标落位；null = 原地 */
          let caret: { container: Node; offset: number } | null = null;

          if (c.nodeType === Node.TEXT_NODE) {
            const t = c as Text;
            if (!forward) {
              if (o > 0 && isAnchor(t.data[o - 1])) {
                // 紧前是锚点零宽：原子单元在节点首字符时的 previousSibling
                if (o - 1 === 0) atomic = atomicAt(t.previousSibling);
                if (atomic) {
                  anchor = { node: t, index: o - 1 };
                  const before = atomic.previousSibling;
                  caret =
                    before && before.nodeType === Node.TEXT_NODE
                      ? { container: before, offset: (before as Text).data.length }
                      : { container: el, offset: [...el.childNodes].indexOf(atomic) };
                }
              } else if (o === 0 && atomicAt(t.previousSibling)) {
                atomic = t.previousSibling as HTMLElement;
                const ps = atomic.previousSibling;
                anchor =
                  ps instanceof Text && ps.data === CARET_ANCHOR ? { node: ps, index: 0 } : null;
                caret = anchor
                  ? { container: el, offset: [...el.childNodes].indexOf(anchor.node) }
                  : { container: el, offset: [...el.childNodes].indexOf(atomic) };
              }
            } else {
              if (o < t.data.length && isAnchor(t.data[o])) {
                // 向前删先吃掉看不见的零宽：接管掉这次「无操作」
                anchor = { node: t, index: o };
                caret = { container: t, offset: o };
              } else if (o === t.data.length && atomicAt(t.nextSibling)) {
                atomic = t.nextSibling as HTMLElement;
                const ns = atomic.nextSibling;
                anchor =
                  ns instanceof Text && ns.data === CARET_ANCHOR ? { node: ns, index: 0 } : null;
                caret = { container: t, offset: o };
              }
            }
          } else if (c === el) {
            if (forward) {
              atomic = atomicAt(el.childNodes[o] ?? null);
              if (atomic) {
                const ns = atomic.nextSibling;
                anchor =
                  ns instanceof Text && ns.data === CARET_ANCHOR ? { node: ns, index: 0 } : null;
                caret = { container: el, offset: o };
              }
            } else {
              atomic = atomicAt(el.childNodes[o - 1] ?? null);
              if (atomic) {
                const ps = atomic.previousSibling;
                anchor =
                  ps instanceof Text && ps.data === CARET_ANCHOR ? { node: ps, index: 0 } : null;
                caret = anchor
                  ? { container: el, offset: [...el.childNodes].indexOf(anchor.node) }
                  : { container: el, offset: [...el.childNodes].indexOf(atomic) };
              }
            }
          }

          if (atomic || anchor) {
            e.preventDefault();
            atomic?.remove();
            if (anchor) {
              anchor.node.deleteData(anchor.index, 1);
              if (anchor.node.data === '' && anchor.node.parentNode === el) anchor.node.remove();
            }
            if (caret) setCaret(caret.container, caret.offset);
            const next = serialize(el);
            if (next === '' && el.childNodes.length > 0) el.replaceChildren();
            onChange(next);
          }
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
