/** 找剧筛选面板：官方多维选项的渲染与单选交互。 */
import { Loader2 } from 'lucide-react';
import { t } from '@/locales';
import { cn } from '@/lib/utils';
import type { BrowseFilters } from '@/service/schema';

/** 面板行 type → 行头标签（未知类型回落服务端行名去掉「全部」前缀）。 */
const FILTER_LABEL_KEYS: Record<string, string> = {
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
        <Loader2 className="size-3.5 animate-spin" />
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

  const pill = (key: keyof BrowseFilters, id: string, label: string, active: boolean) => (
    <button
      key={id || '__all__'}
      type="button"
      onClick={() => onPick(key, id)}
      aria-pressed={active}
      className={cn(
        'cursor-pointer rounded-full border px-3 py-0.5 text-xs transition-colors',
        active
          ? 'border-primary text-primary bg-primary/10 font-medium'
          : 'text-muted-foreground hover:bg-accent hover:text-foreground border-border',
      )}
    >
      {label}
    </button>
  );

  return (
    <div className="grid gap-1.5">
      {rows.map((row) => {
        const key = row.rowType as keyof BrowseFilters;
        const current = rowValue(row.rowType);
        const fallbackLabel = FILTER_LABEL_KEYS[row.rowType] ?? row.rowName.replace(/^全部/, '');
        return (
          <div key={row.rowType} className="flex items-start gap-3 text-sm">
            <span className="text-muted-foreground w-10 shrink-0 pt-1 text-xs">
              {t(fallbackLabel)}
            </span>
            <div className="flex flex-wrap gap-x-1 gap-y-1.5">
              {pill(key, '', t('browse.all'), current === '')}
              {row.items.map((it) => pill(key, it.id, it.name, current === it.id))}
            </div>
          </div>
        );
      })}
      {/* 完结状态（hgplayer 1.1.8 同款客户端合成行，服务端 rows 不含）：
          选中值走请求体顶层 creation_status（creation_status_0 已完结 /
          creation_status_1 连载中） */}
      <div className="flex items-start gap-3 text-sm">
        <span className="text-muted-foreground w-10 shrink-0 pt-1 text-xs">
          {t('browse.fCreation')}
        </span>
        <div className="flex flex-wrap gap-x-1 gap-y-1.5">
          {pill('creationStatus', '', t('browse.all'), filters.creationStatus === '')}
          {pill(
            'creationStatus',
            'creation_status_0',
            t('browse.creationFinished'),
            filters.creationStatus === 'creation_status_0',
          )}
          {pill(
            'creationStatus',
            'creation_status_1',
            t('browse.creationOngoing'),
            filters.creationStatus === 'creation_status_1',
          )}
        </div>
      </div>
    </div>
  );
}
