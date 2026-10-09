import { call } from '../tauri/invoke';
import {
  proxyTestResultSchema,
  settingsSchema,
  type ProxyConfig,
  type ProxyTestResult,
  type Settings,
} from '../schema';

// ---------------------------------------------------------------- 设置

export const settings = {
  get: () => call<Settings>('get_settings', undefined, settingsSchema),
  save: (next: Settings) => call<Settings>('save_settings', { settings: next }, settingsSchema),
  testProxy: (draft?: ProxyConfig) =>
    call<ProxyTestResult>('test_proxy', { draft: draft ?? null }, proxyTestResultSchema),
};
