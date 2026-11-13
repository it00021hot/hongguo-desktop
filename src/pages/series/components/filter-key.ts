import type { BrowseFilters } from '@/service/schema';

const FILTER_KEYS: ReadonlyMap<string, keyof BrowseFilters> = new Map([
  ['genre', 'genre'],
  ['category_dim_theme', 'theme'],
  ['category_dim_role', 'role'],
  ['category_dim_epoch', 'epoch'],
  ['sort', 'sort'],
  ['gender', 'gender'],
  ['online_time', 'onlineTime'],
  ['duration', 'duration'],
]);

export function filterKey(rowType: string): keyof BrowseFilters | undefined {
  return FILTER_KEYS.get(rowType);
}
