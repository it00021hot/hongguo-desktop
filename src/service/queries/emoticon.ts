import { useInfiniteQuery, useMutation, useQuery, useQueryClient } from '@tanstack/react-query';

import { emoticon as emoticonCmd } from '../commands';
import type { Emoticon, EmoticonCollectInput } from '../schema';
import { keys } from './common';

// ---------------------------------------------------------------- 表情（官方表情 / 第三方 GIF / 自定义表情）
//
// 官方三档来源：
// - 第三方表情库：GIF tab，`search_type=2`，一页 100 条，按 offset 翻页；
// - 我上传的自定义表情：♡ tab，`search_type=4&keyword=profile`；
// - 关键词搜索：`search_type=1`，官方逐字前缀匹配。
//
// 表情是全站用户生产的素材（官方标注「内容来自于第三方」），按社交态
// 数据处理：不缓存，挂载即拉。

/** 第三方表情库（GIF tab）：分页无限滚动。 */
export function useThirdPartyEmoticons(enabled: boolean) {
  return useInfiniteQuery({
    queryKey: keys.emoticonsThirdParty,
    queryFn: ({ pageParam }) => emoticonCmd.listThirdParty(100, pageParam),
    initialPageParam: 0,
    getNextPageParam: (last) => (last.hasMore ? last.nextOffset : undefined),
    enabled,
    staleTime: 0,
  });
}

/** 我上传的自定义表情（♡ tab）。 */
export function useOwnEmoticons(enabled: boolean) {
  return useQuery({
    queryKey: keys.emoticonsOwn,
    queryFn: () => emoticonCmd.listOwn(49, 0),
    enabled,
    staleTime: 0,
  });
}

/** 关键词搜索表情（关键词非空才发请求）。 */
export function useEmoticonSearch(keyword: string, enabled: boolean) {
  const trimmed = keyword.trim();
  return useQuery({
    queryKey: keys.emoticonSearch(trimmed),
    queryFn: () => emoticonCmd.search(trimmed, 15, 0),
    enabled: enabled && trimmed.length > 0,
    staleTime: 0,
  });
}

/**
 * 上传自定义表情：先传图（`/reading/reader/upload/picture/`），再收藏
 * （`/reading/ugc/comment/emoticon/collect/v`）——官方是这两步。
 *
 * 成功后失效「我的表情」列表，新表情立刻出现在 ♡ tab。
 */
export function useUploadCustomEmoticon() {
  const queryClient = useQueryClient();
  return useMutation({
    // 组件层有定制 toast（上传/收藏/删除各自的回执），全局错误面静默
    meta: { silentOnError: true },
    mutationFn: async (input: {
      bytes: number[];
      filename: string;
      width: number;
      height: number;
    }) => {
      const uploaded = await emoticonCmd.upload(input.bytes, input.filename);
      // 上传的图还没有服务端 id（官方实包这一档 image_id 恒空串）
      return emoticonCmd.collect({
        imageId: '',
        imageUrl: uploaded.url,
        width: input.width,
        height: input.height,
        imageType: 1,
      });
    },
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: keys.emoticonsOwn });
    },
  });
}

/**
 * 收藏一张已有表情到「我的表情」（长按表情 → 「添加到表情」）。
 *
 * 官方形态：`image_id` 填**源表情 id**，`image_url` 填该
 * 表情的静态 URL；服务端把它并进「我的表情」列表（与上传的自定义表情
 * 同一列表，同一档 `search_type=4&keyword=profile`）。
 */
export function useCollectEmoticon() {
  const queryClient = useQueryClient();
  return useMutation({
    // 组件层有定制 toast，全局错误面静默（重复收藏等由组件层提示）
    meta: { silentOnError: true },
    mutationFn: (input: EmoticonCollectInput) => emoticonCmd.collect(input),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: keys.emoticonsOwn });
    },
  });
}

/**
 * 从「我的表情」删除（长按自己的表情 → 「删除表情」）。
 *
 * 官方同款：同一端点、`collect_type=2`；服务端回
 * `image_data: null`，所以没有可回显的表情对象。
 */
export function useDeleteEmoticon() {
  const queryClient = useQueryClient();
  return useMutation({
    // 组件层有定制 toast，全局错误面静默
    meta: { silentOnError: true },
    mutationFn: (input: EmoticonCollectInput) => emoticonCmd.remove(input),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: keys.emoticonsOwn });
    },
  });
}

/** 表情图片的展示 URL：动图优先用 `dynamicUrl`（官方动图表情会动）。 */
export function emoticonPreviewUrl(item: Emoticon): string {
  return item.dynamicUrl && item.imageType === 2 ? item.dynamicUrl : item.webUri;
}
