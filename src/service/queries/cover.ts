import { coverProxyUrl, isRenderableCover } from '@/utils/cover';

// ---------------------------------------------------------------- 封面增强

/**
 * 封面增强：能直接渲染的格式给 undefined（组件原样加载源图）；HEIC 等
 * WebView 解不了的源给 hongguo-cover 本地转码代理地址（ffmpeg 下载 HEIC
 * 转 JPEG，磁盘缓存）。
 *
 * 纯同步无查询。失败时组件自己的 onError 占位图兜底。
 */
export function useWebCover(sourceCover: string) {
  return {
    data:
      sourceCover !== '' && !isRenderableCover(sourceCover) ? coverProxyUrl(sourceCover) : undefined,
  };
}
