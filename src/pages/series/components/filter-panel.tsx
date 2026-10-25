/** 找剧筛选面板：官方多维选项的渲染与单选交互。 */
import { Spinner } from '@/components/ui/spinner';
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group';
import { t, type MessageKey } from '@/locales';
import type { BrowseFilters } from '@/service/schema';

/** 面板行 type → 行头标签（未知类型回落服务端行名去掉「全部」前缀）。 */
const FILTER_LABEL_KEYS: Record<string, MessageKey> = {
  genre: 'browse.fGenre',
  category_dim_theme: 'browse.fTheme',
  category_dim_role: 'browse.fRole',
  category_dim_epoch: 'browse.fEpoch',
  sort: 'browse.fSort',
  gender: 'browse.fGender',
  online_time: 'browse.fOnlineTime',
  duration: 'browse.fDuration',
};

/**
 * 筛选面板：八行维度，每行「全部」+ 服务端选项，单选。
 * 行头标签按 type 映射 i18n（服务端 row_name 是中文，不适合多语言）。
 */
export function FilterPanel({
  rows,
  filters,
  loading,
  failed,
  onPick,
}: {
  rows: { rowType: string; rowName: string; items: { id: string; name: string }[] }[];
  filters: BrowseFilters;
  loading: boolean;
  failed: boolean;
  onPick: (key: keyof BrowseFilters, value: string) => void;
}) {
  if (loading) {
    return (
      <div className="text-muted-foreground flex items-center gap-2 py-2 text-sm">
        <Spinner className="size-3.5" />
        {t('common.loading')}
      </div>
    );
  }
  if (failed || rows.length === 0) return null;

  const rowValue = (key: string): string => {
    switch (key) {
      case 'genre':
        return filters.genre;
      case 'category_dim_theme':
        return filters.theme;
      case 'category_dim_role':
        return filters.role;
      case 'category_dim_epoch':
        return filters.epoch;
      case 'sort':
        return filters.sort;
      case 'gender':
        return filters.gender;
      case 'online_time':
        return filters.onlineTime;
      case 'duration':
        return filters.duration;
      default:
        return '';
    }
  };

  /** 行内单选胶囊（ToggleGroupItem；空值「全部」走哨兵 value，提交时还原空串） */
  const pillCls =
    'text-muted-foreground h-auto cursor-pointer rounded-full border border-border bg-transparent px-3 py-0.5 text-xs transition-colors hover:bg-accent hover:text-foreground data-[state=on]:border-primary data-[state=on]:bg-primary/10 data-[state=on]:font-medium data-[state=on]:text-primary';

  const rowGroup = (
    key: keyof BrowseFilters,
    current: string,
    items: { id: string; label: string }[],
  ) => (
    <ToggleGroup
      type="single"
      spacing={1}
      value={current === '' ? '__all__' : current}
      onValueChange={(v) => onPick(key, v === '__all__' ? '' : v)}
      className="flex flex-wrap gap-x-1 gap-y-1.5"
    >
      <ToggleGroupItem value="__all__" className={pillCls}>
        {t('browse.all')}
      </ToggleGroupItem>
      {items.map((it) => (
        <ToggleGroupItem key={it.id} value={it.id} className={pillCls}>
          {it.label}
        </ToggleGroupItem>
      ))}
    </ToggleGroup>
  );

  return (
    <div className="grid gap-1.5">
      {rows.map((row) => {
        const key = row.rowType as keyof BrowseFilters;
        const current = rowValue(row.rowType);
        // 已知行类型走 i18n key；未知类型回落服务端行名（纯展示串，
        // 查不到词条 t() 会回落原串）——服务端数据无法静态枚举才 cast
        const fallbackLabel =
          FILTER_LABEL_KEYS[row.rowType] ?? (row.rowName.replace(/^全部/, '') as MessageKey);
        return (
          <div key={row.rowType} className="flex items-start gap-3 text-sm">
            <span className="text-muted-foreground w-10 shrink-0 pt-1 text-xs">
              {t(fallbackLabel)}
            </span>
            {rowGroup(
              key,
              current,
              row.items.map((it) => ({ id: it.id, label: it.name })),
            )}
          </div>
        );
      })}
      {/* 完结状态（客户端合成行，服务端 rows 不含）：
          选中值走请求体顶层 creation_status（creation_status_0 已完结 /
          creation_status_1 连载中） */}
      <div className="flex items-start gap-3 text-sm">
        <span className="text-muted-foreground w-10 shrink-0 pt-1 text-xs">
          {t('browse.fCreation')}
        </span>
        {rowGroup('creationStatus', filters.creationStatus, [
          { id: 'creation_status_0', label: t('browse.creationFinished') },
          { id: 'creation_status_1', label: t('browse.creationOngoing') },
        ])}
      </div>
    </div>
  );
}
