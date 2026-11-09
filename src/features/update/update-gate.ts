import { offlineQa } from '@/utils/offline-qa';

/** 更新插件的唯一查询入口。启动、手动和失败后的再次检查都从这里过。 */
export async function queryUpdate<T>(check: () => Promise<T>): Promise<T | null> {
  if (offlineQa()) return null;
  return check();
}
