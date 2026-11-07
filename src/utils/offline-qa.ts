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

/** authority 中最后一个 `@` 之后才是 host。userinfo 里的 localhost 不能当成主机。 */
export function requestHost(url: string): string | null {
  const afterScheme = url.split('://')[1];
  if (!afterScheme) return null;
  const authority = afterScheme.split(/[/?#]/, 1)[0] ?? '';
  if (!authority) return null;
  const at = authority.lastIndexOf('@');
  const hostport = at >= 0 ? authority.slice(at + 1) : authority;
  if (!hostport) return null;
  if (hostport.startsWith('[')) {
    const end = hostport.indexOf(']');
    return end > 1 ? hostport.slice(1, end).toLowerCase() : null;
  }
  const host = hostport.split(':', 1)[0] ?? '';
  return host ? host.toLowerCase() : null;
}

export function isLoopbackUrl(url: string): boolean {
  const host = requestHost(url);
  return host === 'localhost' || host === '127.0.0.1' || host === '::1';
}

/** 离线时，http(s) 且真实 host 不是 loopback 的地址不能挂进 img/src。 */
export function externalPageUrl(url: string): boolean {
  return offlineQa() && /^https?:\/\//i.test(url) && !isLoopbackUrl(url);
}

/** 离线时把外部图片地址收成空串，组件因此不发请求。 */
export function pageImageSrc(url: string): string {
  if (!url || externalPageUrl(url)) return '';
  return url;
}
