/**
 * 表情发送输入框（弹幕 / 单集评论 / 回复 / 剧评共用的统一封装）。
 *
 * 封装三处原本各自手写的接线：富文本输入（`[名字]` 表情即时渲染）+
 * 表情选择器开合 + ref 插入 + Enter 提交 + 发送按钮 pending 态。
 * 皮肤差异（控制栏胶囊 / 深色面板 / 浅色剧评）全部经 className 注入，
 * 组件本身不含任何业务（发送 mutation / 登录门槛归调用方）。
 */
import { useRef, useState } from 'react';
import { Button } from '@/components/ui/button';
import { t } from '@/locales';
import { cn } from '@/lib/utils';
import { EmojiPickerButton } from './emoji-picker';
import { RichEmojiInput, type RichEmojiInputHandle } from './rich-emoji-input';

interface Props {
  value: string;
  onChange: (next: string) => void;
  /** Enter / 点发送按钮（业务提交逻辑归调用方） */
  onSubmit: () => void;
  /** Esc（回复行用：退出回复态） */
  onEscape?: () => void;
  placeholder: string;
  /** 发送按钮文案；缺省 = 「发送」 */
  sendLabel?: string;
  /** 提交进行中（禁发送钮） */
  pending?: boolean;
  /** 额外发送门槛（如剧评需先选星级）；缺省 = 非空文本即可发 */
  canSend?: boolean;
  maxLength?: number;
  autoFocus?: boolean;
  /** 表情面板对齐方向（控制栏左对齐 / 面板右侧对齐） */
  pickerAlign?: 'left' | 'right';
  /** 容器（flex 行）附加类 */
  className?: string;
  /** 富文本输入附加类（各场景皮肤） */
  inputClassName?: string;
  /** 发送按钮附加类（覆盖默认红底胶囊皮肤） */
  sendClassName?: string;
}

export function EmojiSendBox({
  value,
  onChange,
  onSubmit,
  onEscape,
  placeholder,
  sendLabel,
  pending = false,
  canSend,
  maxLength,
  autoFocus,
  pickerAlign = 'left',
  className,
  inputClassName,
  sendClassName,
}: Props) {
  const [emojiOpen, setEmojiOpen] = useState(false);
  const richRef = useRef<RichEmojiInputHandle | null>(null);
  const enabled = canSend ?? value.trim() !== '';
  return (
    <div className={cn('flex items-center gap-2', className)}>
      <RichEmojiInput
        ref={richRef}
        value={value}
        onChange={onChange}
        onEnter={onSubmit}
        onEscape={onEscape}
        placeholder={placeholder}
        maxLength={maxLength}
        autoFocus={autoFocus}
        className={inputClassName}
      />
      <EmojiPickerButton
        open={emojiOpen}
        onToggle={() => setEmojiOpen((o) => !o)}
        align={pickerAlign}
        onPick={(name) => richRef.current?.insertEmoji(name)}
      />
      <Button
        size="sm"
        className={cn('shrink-0', sendClassName)}
        disabled={!enabled || pending}
        onClick={onSubmit}
      >
        {sendLabel ?? t('player.interact.send')}
      </Button>
    </div>
  );
}
