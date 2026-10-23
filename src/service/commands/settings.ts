import { call } from '../tauri/invoke';
import type { ProxyConfig, Settings } from '../schema';

// ---------------------------------------------------------------- 设置

export const settings = {
  get: () => call('get_settings'),
  save: (next: Settings) => call('save_settings', { settings: next }),
  /** 缓存目录留空时的默认位置（进程内不变）。 */
  defaultCacheDir: () => call('default_cache_dir'),
  testProxy: (draft?: ProxyConfig) => call('test_proxy', { draft: draft ?? null }),
};
