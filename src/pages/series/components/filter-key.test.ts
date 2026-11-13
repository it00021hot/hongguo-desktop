import { describe, expect, it } from 'vitest';
import { browseFiltersSchema, type BrowseFilters } from '@/service/schema';
import { filterKey } from './filter-key';

const FIELDS: [string, keyof BrowseFilters][] = [
  ['genre', 'genre'],
  ['category_dim_theme', 'theme'],
  ['category_dim_role', 'role'],
  ['category_dim_epoch', 'epoch'],
  ['sort', 'sort'],
  ['gender', 'gender'],
  ['online_time', 'onlineTime'],
  ['duration', 'duration'],
];

describe('browse selector fields', () => {
  it.each(FIELDS)('preserves %s in the actual browse request field %s', (row, field) => {
    const key = filterKey(row);
    expect(key).toBe(field);
    if (key === undefined) throw new Error(`Missing filter field for ${row}`);
    const selected = browseFiltersSchema.parse({
      ...browseFiltersSchema.parse({}),
      [key]: 'chosen',
    });
    expect(selected[field]).toBe('chosen');
    const cleared = browseFiltersSchema.parse({ ...selected, [key]: '' });
    expect(cleared[field]).toBe('');
  });

  it('does not invent a state or IPC field for an unknown server row', () => {
    expect(filterKey('future-unrecognized-row')).toBeUndefined();
  });
});
