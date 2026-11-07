/** 与 Rust `HONGGUO_OFFLINE_QA` 精确等于 `1` 同一条规则。未设置、空串、`0` 都是正式联网。 */
function runtimeOfflineFlag(): string | undefined {
  const bag = globalThis as unknown as { __HONGGUO_OFFLINE_QA?: string };
  const flag = bag.__HONGGUO_OFFLINE_QA;
  return typeof flag === 'string' ? flag : undefined;
}

export function offlineQa(): boolean {
  const runtime = runtimeOfflineFlag();
  if (runtime !== undefined) return runtime === '1';
  return import.meta.env.VITE_HONGGUO_OFFLINE_QA === '1';
}

/** 页面解析相对地址时用的基址。只用于判断 host，不发起请求。 */
const PAGE_BASE = 'http://127.0.0.1/';

/** WHATWG URL，与浏览器给 img/src 和 fetch 用的是同一解析器。失败返回 null。 */
export function parsePageUrl(input: string): URL | null {
  try {
    return new URL(input, PAGE_BASE);
  } catch {
    return null;
  }
}

export function requestHost(input: string): string | null {
  return parsePageUrl(input)?.hostname ?? null;
}

function loopbackHost(hostname: string): boolean {
  const host = hostname.replaceAll('[', '').replaceAll(']', '');
  return host === 'localhost' || host === '127.0.0.1' || host === '::1';
}

/** 离线时，解析后会变成非 loopback HTTP(S) 的地址不能挂进 img/src。解析失败也拒绝。 */
export function externalPageUrl(input: string): boolean {
  if (!offlineQa()) return false;
  const parsed = parsePageUrl(input);
  if (!parsed) return true;
  if (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') return false;
  return !loopbackHost(parsed.hostname);
}

/** 离线时把外部图片地址收成空串，组件因此不发请求。 */
export function pageImageSrc(url: string): string {
  if (!url || externalPageUrl(url)) return '';
  return url;
}
