import { useState } from 'react';

/** 列表页批量管理模式的选择状态（对齐 hgplayer v1.1.6 的「管理」：
 *  进入管理 → 多选 → 批量删除）。选中集合按条目 id 记。
 *  与 BatchBar/ManageToggle/PickDot（components/batch-manage.tsx）配套。 */
export function useBatchSelect() {
  const [managing, setManaging] = useState(false);
  const [selected, setSelected] = useState<ReadonlySet<string>>(new Set());

  const enter = () => setManaging(true);
  const exit = () => {
    setManaging(false);
    setSelected(new Set());
  };
  const toggle = (id: string) =>
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  const toggleAll = (ids: readonly string[]) =>
    setSelected((prev) => (prev.size >= ids.length ? new Set() : new Set(ids)));

  return { managing, selected, enter, exit, toggle, toggleAll };
}
