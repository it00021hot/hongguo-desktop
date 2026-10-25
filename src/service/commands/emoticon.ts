import { call } from '../tauri/invoke';
import type { EmoticonCollectInput } from '../schema';

// ---------------------------------------------------------------- 表情（官方表情 / 第三方 GIF / 自定义表情）

/**
 * 表情域命令封装（2026-10-25 官方实包锁定）。
 *
 * 档位来源对应官方面板的入口：GIF tab（第三方）、♡ tab（我的表情）、
 * 搜索页（关键词）。收藏与删除共用官方 collect 端点，只是
 * `collect_type` 分别为 1 / 2。
 */
export const emoticon = {
  /** 第三方表情库（GIF tab）：官方一页 100 条，按 offset 翻页。 */
  listThirdParty: (count?: number, offset?: number) =>
    call('emoticon_list', { source: 'thirdParty', count, offset }),
  /** 我上传的自定义表情（♡ tab）。 */
  listOwn: (count?: number, offset?: number) =>
    call('emoticon_list', { source: 'own', count, offset }),
  /** 关键词搜索表情（官方逐字前缀匹配，后端已带 `+` 前缀）。 */
  search: (keyword: string, count?: number, offset?: number) =>
    call('emoticon_search', { keyword, count, offset }),
  /** 上传本地图片（评论附图 / 自定义表情共用）。 */
  upload: (bytes: number[], filename: string) => call('emoticon_upload', { bytes, filename }),
  /** 把一张已有表情收藏进「我的表情」（长按 → 添加到表情）。 */
  collect: (input: EmoticonCollectInput) => call('emoticon_collect', input),
  /** 从「我的表情」删除（长按自己的表情 → 删除表情）。 */
  remove: (input: EmoticonCollectInput) => call('emoticon_delete', input),
};
