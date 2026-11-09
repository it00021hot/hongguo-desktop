// 可重跑前提探针。只做 WHATWG 解析，不发请求。
// 手写 authority / 最后 @ / ^https?:// 已被这些输入证伪，产品守卫改为同一解析器。
const inputs = [
  'https://example.com\\@127.0.0.1/a.jpeg',
  ' https://example.com/a.jpeg',
  '//example.com/a.jpeg',
  'https:/example.com/a.jpeg',
  'http://localhost:80@example.com/a',
  'http://127.0.0.1:80@example.com/a',
  'http://user:pass@127.0.0.1/a',
  'http://[::1]:80/x',
];
const base = 'http://127.0.0.1/';
for (const input of inputs) {
  const url = new URL(input, base);
  const host = url.hostname.replaceAll('[', '').replaceAll(']', '');
  const loopback = host === 'localhost' || host === '127.0.0.1' || host === '::1';
  console.log(
    JSON.stringify({
      input,
      protocol: url.protocol,
      hostname: url.hostname,
      offlineWouldReject: url.protocol === 'http:' || url.protocol === 'https:' ? !loopback : false,
    }),
  );
}
