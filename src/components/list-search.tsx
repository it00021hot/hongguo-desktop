//! 「我的」系列表页右上角的搜索框（对齐参考端 v1.1.6 的历史/收藏/点赞/预约）。
//!
//! 纯客户端过滤：列表数据本来就全量在内存里，按标题/seriesId contains 即可，
//! 不牵动后端。四页共用同一形态（放大镜内嵌 + 细输入框）。

import { Search } from 'lucide-react';
import { Input } from '@/components/ui/input';

export function ListSearch({
  value,
  onChange,
  placeholder,
}: {
  value: string;
  onChange: (v: string) => void;
  placeholder: string;
}) {
  return (
    <div className="relative w-60 max-w-full">
      <Search
        className="text-muted-foreground pointer-events-none absolute top-1/2 left-2.5 size-4 -translate-y-1/2"
        aria-hidden
      />
      <Input
        value={value}
        onChange={(e) => onChange(e.target.value)}
        placeholder={placeholder}
        className="h-8 pl-8 text-sm"
      />
    </div>
  );
}
