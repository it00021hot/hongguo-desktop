/**
 * 表情发送输入框（弹幕 / 单集评论 / 回复 / 剧评共用的统一封装）。
 *
 * 封装三处原本各自手写的接线：富文本输入（`[名字]` 表情即时渲染）+
 * 表情选择器开合 + ref 插入 + Enter 提交 + 发送按钮 pending 态。
 * 皮肤差异（控制栏胶囊 / 深色面板 / 浅色剧评）全部经 className 注入，
 * 组件本身不含任何业务（发送 mutation / 登录门槛归调用方）。
 */
import { useImperativeHandle, useRef, useState } from 'react';
import { ImageIcon } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { t } from '@/locales';
import { cn } from '@/lib/utils';
import { emoticon } from '@/service/commands/emoticon';
import type { Emoticon } from '@/service/schema';
import { localFromPicked, type LocalImageAttachment } from '@/utils/image-attachment';
import { EmojiPickerButton } from './emoji-picker';
import { RichEmojiInput, type RichEmojiInputHandle } from './rich-emoji-input';

/** 透传给调用方的输入框操作句柄（表情 / @ 提及插入） */
export interface EmojiSendBoxHandle {
  insertEmoji: (name: string) => void;
  insertMention: (name: string) => void;
}

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
  /**
   * 表情面板锚点（见 `EmojiPickerButton`）：`send-area` 时面板贴在发送区
   * 顶边，发送区自己的行（附件预览、回复目标）不会被面板盖住。
   */
  pickerAnchorTo?: 'trigger' | 'send-area';
  /** 容器（flex 行）附加类 */
  className?: string;
  /** 富文本输入附加类（各场景皮肤） */
  inputClassName?: string;
  /** 发送按钮附加类（覆盖默认红底胶囊皮肤） */
  sendClassName?: string;
  /** AI bot 用户名：提及 chip 对它追加官方同款渐变「Ai」后缀 */
  mentionAiName?: string;
  /** 选中表情图片（第三方 GIF / 自定义表情 / 搜索结果），带场景值 */
  onPickImage?: (item: Emoticon, sceneType: number) => void;
  /** 选本地图片（评论附图；官方输入框左侧的「图片」入口）。 */
  onPickLocalImage?: (local: LocalImageAttachment) => void;
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
  pickerAnchorTo = 'trigger',
  className,
  inputClassName,
  sendClassName,
  mentionAiName,
  onPickImage,
  onPickLocalImage,
  ref,
}: Props & { ref?: React.Ref<EmojiSendBoxHandle> }) {
  const [emojiOpen, setEmojiOpen] = useState(false);
  const richRef = useRef<RichEmojiInputHandle | null>(null);
  useImperativeHandle(
    ref,
    () => ({
      insertEmoji: (name: string) => richRef.current?.insertEmoji(name),
      insertMention: (name: string) => richRef.current?.insertMention(name),
    }),
    [],
  );
  const enabled = canSend ?? value.trim() !== '';
  return (
    <div className={cn('flex items-center gap-2', className)}>
      {onPickLocalImage && (
        <button
          type="button"
          title={t('player.emoticon.image')}
          aria-label={t('player.emoticon.image')}
          onMouseDown={(e) => e.preventDefault()}
          // 选图走 Rust 原生文件面板：WKWebView 对 `<input type=file>`
          // 不弹选择面板（真实点击到达 input 也不调 runOpenPanel，实测
          // 2026-10-26，WebView2 无此问题）。
          onClick={() => {
            void emoticon.pickImage().then((picked) => {
              const local = localFromPicked(picked);
              if (local) onPickLocalImage(local);
            });
          }}
          className="grid shrink-0 cursor-pointer place-items-center rounded-full px-1 text-white/60 transition-colors hover:text-white"
        >
          <ImageIcon className="size-4" aria-hidden />
        </button>
      )}
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
        mentionAiName={mentionAiName}
      />
      <EmojiPickerButton
        open={emojiOpen}
        onToggle={() => setEmojiOpen((o) => !o)}
        align={pickerAlign}
        anchorTo={pickerAnchorTo}
        onPick={(name) => richRef.current?.insertEmoji(name)}
        onPickImage={onPickImage}
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
