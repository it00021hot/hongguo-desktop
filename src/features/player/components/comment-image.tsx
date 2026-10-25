/**
 * 评论附图（图片评论 / 表情评论共用一张图）。
 *
 * 长按 → 「添加到表情」（官方同款：长按别人发的图片可把它收藏进「我的
 * 表情」；官方实包走 `emoticon/collect/v`，`image_id` 填该图片的 id）。
 */
import { useCallback } from 'react';

import { emoticonPreviewUrl } from '@/service/queries';
import type { Emoticon } from '@/service/schema';
import { useLongPress, type LongPressPoint } from '@/components/common/emoji/use-long-press';

export function CommentImage({
  image,
  onMenu,
}: {
  image: Emoticon;
  /** 打开操作菜单（长按或右键——官方是长按，桌面端右键是等价手势）。 */
  onMenu: (image: Emoticon, point: LongPressPoint) => void;
}) {
  const longPress = useLongPress(
    useCallback(
      (point: LongPressPoint) => {
        onMenu(image, point);
      },
      [image, onMenu],
    ),
  );

  return (
    <img
      src={emoticonPreviewUrl(image)}
      alt=""
      loading="lazy"
      onPointerDown={longPress.onPointerDown}
      onPointerUp={longPress.onPointerUp}
      onPointerCancel={longPress.onPointerCancel}
      onPointerLeave={longPress.onPointerLeave}
      onContextMenu={(e) => {
        e.preventDefault();
        onMenu(image, { x: e.clientX, y: e.clientY });
      }}
      className="max-h-40 rounded-md border border-white/10 object-cover"
    />
  );
}
