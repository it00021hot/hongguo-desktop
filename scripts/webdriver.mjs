// W3C WebDriver 自动化测试 CLI（零依赖，node >= 22 内建 fetch）。
//
// macOS 的 WKWebView 没有对外 CDP/远程调试协议（桌面目标被 Apple 私有
// entitlement 锁给 Safari），scripts/cdp.mjs 那套是 Windows WebView2 专用。
// 这份脚本是它的 WebDriver 对等物：app 的 dev 构建通过 tauri-plugin-webdriver
// 在 127.0.0.1:4445 内嵌完整 W3C remote end，**直连即可**（app 已在跑时不必
// 经 tauri-webdriver intermediary，绕开单实例约束）。
//
// 用法（与 cdp.mjs 同形，路由**不带前导斜杠**）：
//   node scripts/webdriver.mjs pages            列出窗口与当前路由
//   node scripts/webdriver.mjs nav browse       SPA 导航（带 HMR 赛跑重试）
//   node scripts/webdriver.mjs eval "<js>"      页面里求值（自动 await Promise）
//   node scripts/webdriver.mjs click "<css>"    真实元素点击（走插件桥，合成事件之外的正规路径）
//   node scripts/webdriver.mjs shot [route] [out]  导航 + 截图 PNG
//   node scripts/webdriver.mjs probe [route]    图健康体检，裂图退出码 1
//
// 与 CDP 版 probe 的口径差异：WKWebView 侧拿不到 Network.loadingFailed，
// 图片请求失败与解码失败都收敛为 complete && naturalWidth === 0 判裂图
// （两种坏在这个形状上同貌），非图片资源的网络失败不可见——需要全量网络
// 事件时用 Windows 侧的 cdp.mjs。
//
// 截图限制：WKWebView 的视频层不走页面合成，`shot` 里视频区域恒黑（画面
// 本身在渲染）。验证播放用 `eval` 读 currentTime/readyState，或
// requestVideoFrameCallback（帧被呈现才回调，是权威信号）。
//
// 端口：TAURI_WEBDRIVER_PORT 环境变量（与插件 init() 同一默认 4445）。
// 会话：id 存 /tmp/hg-wd-session.json 复用；失效自动重建。**不主动删会话**
// ——插件对 DELETE session 的行为（是否动窗口）未验证，宁可留着。

import { writeFileSync, readFileSync, existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const CDP_HTTP = `http://127.0.0.1:${process.env.TAURI_WEBDRIVER_PORT ?? 4445}`;
const VITE_ORIGIN = 'http://localhost:1420';
const SESSION_FILE = join(tmpdir(), 'hg-wd-session.json');

const [cmd, ...args] = process.argv.slice(2);

/** W3C 响应解包：{value} 取 value；错误响应抛带 WebDriver error code 的异常。 */
async function unwrap(res) {
  const body = await res.json().catch(() => ({}));
  const v = body?.value ?? body;
  if (v?.error) {
    throw new Error(`WebDriver ${v.error}: ${v.message ?? ''}`);
  }
  return v;
}

/** 已有会话就复用（探测一次 /url），没有就建新的并落盘。 */
async function session() {
  const create = async () => {
    const res = await fetch(`${CDP_HTTP}/session`, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ capabilities: { alwaysMatch: {} } }),
    });
    const s = await unwrap(res);
    const id = s.sessionId ?? s['session'];
    if (!id) throw new Error('建会话失败：响应里没有 sessionId');
    writeFileSync(SESSION_FILE, id);
    return id;
  };
  if (existsSync(SESSION_FILE)) {
    const id = readFileSync(SESSION_FILE, 'utf8').trim();
    try {
      await unwrap(await fetch(`${CDP_HTTP}/session/${id}/url`));
      return id;
    } catch {
      // 会话失效（app 重启过）——重建
    }
  }
  return create();
}

/** 页面里求值，Promise 自动等完（execute/async，末位 arguments 是完成回调）。 */
async function evaluate(id, expression) {
  const script = `
    const done = arguments[arguments.length - 1];
    Promise.resolve((() => { ${expression} })()).then(
      (v) => done({ ok: v }),
      (e) => done({ err: String(e && e.message ? e.message : e) }),
    );`;
  const r = await unwrap(
    await fetch(`${CDP_HTTP}/session/${id}/execute/async`, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ script, args: [] }),
    }),
  );
  if (r?.err) throw new Error(`页面里抛异常了: ${r.err}`);
  return r?.ok;
}

/** 当前页面 origin：dev 是 vite，打包后是 tauri://localhost——各用各的。 */
async function origin(id) {
  const url = await unwrap(await fetch(`${CDP_HTTP}/session/${id}/url`));
  try {
    return new URL(url).origin;
  } catch {
    return VITE_ORIGIN;
  }
}

/** 路由参数归一：与 cdp.mjs 同约定（`browse`、`.` 表示根）。 */
function normRoute(arg) {
  const raw = arg ?? '.';
  const cleaned = raw === '.' || raw === '' ? '' : raw.replace(/^\/+/, '').replace(/^\.\/?/, '');
  return `/${cleaned}`;
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/** SPA 导航：落点用 pathname+search 整体比，不对就再试（vite HMR 全量重载会抢路由）。 */
async function nav(id, route) {
  const base = await origin(id);
  for (let i = 0; i < 3; i++) {
    await unwrap(
      await fetch(`${CDP_HTTP}/session/${id}/url`, {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ url: base + route }),
      }),
    );
    await sleep(4000);
    if ((await evaluate(id, `return location.pathname + location.search;`)) === route) return route;
  }
  throw new Error(
    `导航到 ${route} 失败，落地在 ${await evaluate(id, `return location.pathname + location.search;`)}`,
  );
}

/** 图健康体检：返回 { imgs, broken }（WebKit 下请求失败与解码失败同貌）。 */
async function imgHealth(id) {
  const imgs = JSON.parse(
    await evaluate(
      id,
      `
      return JSON.stringify([...document.querySelectorAll('img')].map((i) => ({
        src: i.src, nw: i.naturalWidth, complete: i.complete,
        shown: i.clientWidth + 'x' + i.clientHeight,
      })));
    `,
    ),
  );
  return { imgs, broken: imgs.filter((i) => i.complete && i.src && i.nw === 0) };
}

if (cmd === 'pages') {
  const id = await session();
  const handles = await unwrap(await fetch(`${CDP_HTTP}/session/${id}/window/handles`));
  const current = await unwrap(await fetch(`${CDP_HTTP}/session/${id}/window`));
  const title = await unwrap(await fetch(`${CDP_HTTP}/session/${id}/title`));
  const url = await unwrap(await fetch(`${CDP_HTTP}/session/${id}/url`));
  console.log(`会话 ${id.slice(0, 8)}…，窗口 ${handles.length} 个（当前 ${current}）`);
  console.log(`  ${title}  ${url}`);
} else if (cmd === 'nav') {
  const id = await session();
  await nav(id, normRoute(args[0]));
  console.log(await evaluate(id, `return location.pathname + location.search;`));
} else if (cmd === 'eval') {
  const id = await session();
  console.log(await evaluate(id, args[0]));
} else if (cmd === 'click') {
  const id = await session();
  const found = await unwrap(
    await fetch(`${CDP_HTTP}/session/${id}/element`, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ using: 'css selector', value: args[0] }),
    }),
  );
  const el = found[Object.keys(found)[0]]; // element-6066-11e4-a52e-4f735466cecf
  await unwrap(await fetch(`${CDP_HTTP}/session/${id}/element/${el}/click`, { method: 'POST' }));
  await sleep(800);
  console.log(`clicked ${args[0]}`);
} else if (cmd === 'shot') {
  const id = await session();
  if (args[0]) await nav(id, normRoute(args[0]));
  await sleep(3000); // 等懒加载图追上
  const b64 = await unwrap(await fetch(`${CDP_HTTP}/session/${id}/screenshot`));
  const out = args[1] ?? join(tmpdir(), `hongguo-wd-${Date.now()}.png`);
  writeFileSync(out, Buffer.from(b64, 'base64'));
  console.log(out);
} else if (cmd === 'probe') {
  const id = await session();
  await nav(id, normRoute(args[0]));
  await sleep(8000); // 等查询与懒加载稳定
  const { imgs, broken } = await imgHealth(id);
  console.log(`路由 ${await evaluate(id, `return location.pathname;`)}`);
  console.log(`img 共 ${imgs.length} 张，裂图 ${broken.length} 张（WebDriver 口径：含网络失败）`);
  for (const b of broken) console.log(`  裂图 nw=0 ${b.src.slice(0, 140)}`);
  for (const i of imgs.filter((x) => x.shown === '0x0' && x.src).slice(0, 5))
    console.log(`  未布局(0x0) ${i.src.slice(0, 140)}`);
  if (broken.length > 0) {
    console.error('FAIL');
    process.exit(1);
  }
  console.log('PASS');
} else {
  console.error(
    '未知子命令。可用：pages | nav <route> | eval "<js>" | click "<css>" | shot [route] [out] | probe [route]',
  );
  process.exit(2);
}
