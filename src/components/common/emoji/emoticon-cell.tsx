/**
 * 表情格（表情面板用）。
 *
 * 两种手势共存：点击 = 选中插入草稿；长按 = 「添加到表情 / 删除表情」
 * （官方同款 500ms 阈值）。长按触发后要吞掉紧随的那一下 click，否则会
 * 「收藏完又把表情插进草稿」——见 `use-long-press`。
 */
import { useCallback } from 'react';

import { emoticonPreviewUrl } from '@/service/queries';
import type { Emoticon } from '@/service/schema';
import { useLongPress, type LongPressPoint } from './use-long-press';

export function EmoticonCell({
  item,
  onPick,
  onMenu,
}: {
  item: Emoticon;
  onPick: (item: Emoticon) => void;
  /**
   * 打开操作菜单（长按或右键——官方是长按，桌面端右键是等价手势）。
   * 省略则该格不支持收藏（本地系统表情没有服务端 id，官方也不可收藏）。
   */
  onMenu?: (item: Emoticon, point: LongPressPoint) => void;
}) {
  const longPress = useLongPress(
    useCallback(
      (point: LongPressPoint) => {
        onMenu?.(item, point);
      },
      [item, onMenu],
    ),
  );

  return (
    <button
      type="button"
      onMouseDown={(e) => e.preventDefault()}
      onPointerDown={longPress.onPointerDown}
      onPointerUp={longPress.onPointerUp}
      onPointerCancel={longPress.onPointerCancel}
      onPointerLeave={longPress.onPointerLeave}
      onClickCapture={longPress.onClickCapture}
      onContextMenu={(e) => {
        e.preventDefault();
        onMenu?.(item, { x: e.clientX, y: e.clientY });
      }}
      onClick={() => onPick(item)}
      className="grid aspect-square cursor-pointer place-items-center rounded-md p-1 transition-colors hover:bg-white/15"
    >
      <img
        src={emoticonPreviewUrl(item)}
        alt=""
        draggable={false}
        loading="lazy"
        className="size-full object-contain"
      />
    </button>
  );
}
