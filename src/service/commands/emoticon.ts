import { call } from '../tauri/invoke';
import type { EmoticonCollectInput } from '../schema';

/**
 * 档位来源对应官方面板入口：GIF tab（第三方）、♡ tab（我的表情）、搜索页（关键词）；
 * 收藏与删除共用官方 collect 端点，`collect_type` 分别为 1 / 2。
 */
export const emoticon = {
  /** 第三方表情库（GIF tab）：官方一页 100 条，按 offset 翻页。 */
  listThirdParty: (count?: number, offset?: number) =>
    call('emoticon_list', { source: 'thirdParty', count, offset }),
  listOwn: (count?: number, offset?: number) =>
    call('emoticon_list', { source: 'own', count, offset }),
  /** 关键词搜索表情（官方逐字前缀匹配，后端已带 `+` 前缀）。 */
  search: (keyword: string, count?: number, offset?: number) =>
    call('emoticon_search', { keyword, count, offset }),
  /** 上传本地图片（评论附图 / 自定义表情共用）。 */
  upload: (bytes: number[], filename: string) => call('emoticon_upload', { bytes, filename }),
  /** 弹系统文件面板选图（取消时 `bytes` 为空数组）；不走 `<input type=file>`——WKWebView 对它不弹选择面板 */
  pickImage: () => call('emoticon_pick_image', {}),
  collect: (input: EmoticonCollectInput) => call('emoticon_collect', input),
  remove: (input: EmoticonCollectInput) => call('emoticon_delete', input),
};
