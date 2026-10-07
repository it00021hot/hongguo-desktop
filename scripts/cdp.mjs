// WebView2 CDP 自动化测试 CLI（零依赖，node >= 22 的内建 fetch + WebSocket）。
//
// 前提：app 以 CDP 启动——
//   WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="--remote-debugging-port=9222" pnpm tauri dev
// 环境变量必须在 app 进程启动**之前**设置；实例已在跑时注入不了，只能重启。
// 背景与实战案例见 docs/webview-cdp-testing.md。
//
// 用法（路由**不带前导斜杠**，原因见 normRoute 注释）：
//   node scripts/cdp.mjs pages               列出 CDP targets
//   node scripts/cdp.mjs nav browse          导航（带 HMR 赛跑重试），打印落地路由
//   node scripts/cdp.mjs eval "<js 表达式>"  页面里求值，打印结果
//   node scripts/cdp.mjs shot [route] [out]  导航 + 截图 PNG（默认落系统临时目录）
//   node scripts/cdp.mjs probe [route]       导航 + 图健康/网络失败体检，裂图退出码 1
//
// probe 的判定口径：`complete && src 非空 && naturalWidth === 0` 视为裂图
// （请求成功但解码失败——HEIC 类问题正是这个形状），懒加载未触发的图
// （complete=false）不算失败；Network.loadingFailed 的图片请求一律算失败。

import { writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const CDP_HTTP = 'http://127.0.0.1:9222';
const APP_ORIGIN = 'http://localhost:1420';

const [cmd, ...args] = process.argv.slice(2);

/**
 * 路由参数归一：约定**不带前导斜杠**传（`probe browse`）——Git Bash 的
 * MSYS 路径转换会把 `/browse` 改写成 `c/Program Files/Git/browse`，
 * 前导斜杠的参数活不到 node 手里。传 `.` 或空串表示根路由。
 */
function normRoute(arg) {
  const raw = arg ?? '.';
  const cleaned = raw === '.' || raw === '' ? '' : raw.replace(/^\/+/, '').replace(/^\.\/?/, '');
  return `/${cleaned}`;
}

/** 连接第一个 page target，返回 { send, events, close }。 */
async function connect() {
  let targets;
  try {
    targets = await (await fetch(`${CDP_HTTP}/json`)).json();
  } catch {
    console.error(
      `CDP 端口连不上（${CDP_HTTP}）。先用下面方式启动 app：\n` +
        `  WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="--remote-debugging-port=9222" pnpm tauri dev`,
    );
    process.exit(2);
  }
  const page = targets.find((t) => t.type === 'page');
  if (!page) {
    console.error('没有 page target（窗口还没起来？）');
    process.exit(2);
  }
  const ws = new WebSocket(page.webSocketDebuggerUrl);
  let seq = 0;
  const pending = new Map();
  const events = [];
  ws.onmessage = (ev) => {
    const m = JSON.parse(ev.data);
    if (m.id) {
      const p = pending.get(m.id);
      if (!p) return;
      pending.delete(m.id);
      m.error ? p.rej(new Error(m.error.message)) : p.res(m.result);
    } else {
      events.push(m);
    }
  };
  await new Promise((res, rej) => {
    ws.onopen = res;
    ws.onerror = () => rej(new Error('WebSocket 连接失败'));
  });
  const send = (method, params = {}) =>
    new Promise((res, rej) => {
      const id = ++seq;
      pending.set(id, { res, rej });
      ws.send(JSON.stringify({ id, method, params }));
    });
  return { send, events, close: () => ws.close() };
}

/** 页面里求值。返回值经 JSON 序列化取回（表达式须返回可序列化结果）。 */
async function evaluate(send, expression) {
  const r = await send('Runtime.evaluate', {
    expression,
    returnByValue: true,
    awaitPromise: true,
  });
  if (r.exceptionDetails) {
    throw new Error(r.exceptionDetails.exception?.description ?? '页面里抛异常了');
  }
  return r.result?.value;
}

/**
 * SPA 导航。vite HMR 全量重载会跟 location 赋值赛跑把路由抢回去，
 * 所以落点不对就再试——两次都失败才认输。落点用 pathname+search 整体比，
 * 带 query 参数的深链（detail?seriesId=…）才能判准。
 */
async function nav(send, route) {
  await send('Page.enable');
  for (let i = 0; i < 3; i++) {
    await evaluate(send, `location.href = ${JSON.stringify(APP_ORIGIN + route)}; 'nav'`);
    await new Promise((r) => setTimeout(r, 4000));
    if ((await evaluate(send, 'location.pathname + location.search')) === route) return route;
  }
  throw new Error(
    `导航到 ${route} 失败，落地在 ${await evaluate(send, 'location.pathname + location.search')}`,
  );
}

/** 图健康体检：返回 { imgs, broken, netFails }。 */
async function imgHealth(send, events) {
  const imgs = await evaluate(
    send,
    `JSON.stringify([...document.querySelectorAll('img')].map(i => ({
      src: i.src, nw: i.naturalWidth, complete: i.complete,
      shown: i.clientWidth + 'x' + i.clientHeight,
    })))`,
  ).then(JSON.parse);
  const netFails = events.filter((m) => m.method === 'Network.loadingFailed');
  const broken = imgs.filter((i) => i.complete && i.src && i.nw === 0);
  return { imgs, broken, netFails };
}

if (cmd === 'pages') {
  const targets = await (await fetch(`${CDP_HTTP}/json`)).json();
  for (const t of targets) console.log(`${t.type}  ${t.title}  ${t.url}`);
} else if (cmd === 'nav') {
  const c = await connect();
  await nav(c.send, normRoute(args[0]));
  console.log(await evaluate(c.send, 'location.pathname + location.search'));
  c.close();
} else if (cmd === 'eval') {
  const c = await connect();
  console.log(await evaluate(c.send, args[0]));
  c.close();
} else if (cmd === 'shot') {
  const c = await connect();
  if (args[0]) await nav(c.send, normRoute(args[0]));
  await new Promise((r) => setTimeout(r, 3000)); // 等懒加载图追上
  const r = await c.send('Page.captureScreenshot', { format: 'png' });
  const out = args[1] ?? join(tmpdir(), `hongguo-cdp-${Date.now()}.png`);
  writeFileSync(out, Buffer.from(r.data, 'base64'));
  console.log(out);
  c.close();
} else if (cmd === 'probe') {
  const c = await connect();
  await c.send('Network.enable');
  // events 在 connect 后才开收，Network.enable 之前的静默期可忽略
  await nav(c.send, normRoute(args[0]));
  await new Promise((r) => setTimeout(r, 8000)); // 等查询与懒加载稳定
  const { imgs, broken, netFails } = await imgHealth(c.send, c.events);
  console.log(`路由 ${await evaluate(c.send, 'location.pathname')}`);
  console.log(`img 共 ${imgs.length} 张，裂图 ${broken.length} 张，网络失败 ${netFails.length} 条`);
  for (const b of broken) console.log(`  裂图 nw=0 ${b.src.slice(0, 140)}`);
  for (const f of netFails)
    console.log(`  网络失败 ${f.params.errorText} (${f.params.type ?? ''})`);
  for (const i of imgs.filter((x) => x.shown === '0x0' && x.src).slice(0, 5))
    console.log(`  未布局(0x0) ${i.src.slice(0, 140)}`);
  c.close();
  if (broken.length > 0 || netFails.some((f) => f.params.type?.startsWith('Image'))) {
    console.error('FAIL');
    process.exit(1);
  }
  console.log('PASS');
} else {
  console.error(
    '未知子命令。可用：pages | nav <route> | eval <js> | shot [route] [out] | probe [route]',
  );
  process.exit(2);
}
