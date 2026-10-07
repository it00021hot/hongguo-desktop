# WebView2 CDP 调试与自动化测试

前端跑在 WebView2（Chromium 内核）里，所以**完整的 Chrome DevTools Protocol 都能用**：
在页面里求值、抓网络事件、截图、导航。排 UI 层的问题（图为什么裂、请求为什么挂、
路由为什么跳错）走这条路，比 UIA 树 + 截图猜坐标精准一个量级。

本文记录启动方式、仓库自带的工具 `scripts/cdp.mjs`，以及自动化测试的判定口径。

## 启动（唯一前提）

```bash
WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="--remote-debugging-port=9222" pnpm tauri dev
```

- 环境变量必须在 app 进程启动**之前**设置（WebView2 由 app 进程派生，env 继承）。
  实例已经在跑时注入不了，只能关掉重启。
- 起来之后 `http://127.0.0.1:9222/json` 列出 targets，`webSocketDebuggerUrl` 就是连接入口。
- **单实例约束不变**：端口 1420 + SQLite 锁，第二个 `tauri dev` 起不来。用户自己开着
  dev 会话时，要么请他关掉，要么退回 computer-use 路线（见文末分工）。

## 工具：`scripts/cdp.mjs`

零依赖（node ≥ 22 内建 fetch + WebSocket）。路由参数**不带前导斜杠**——Git Bash 的
MSYS 路径转换会把 `/browse` 改写成 `c/Program Files/Git/browse`，前导斜杠活不到 node。

```bash
node scripts/cdp.mjs pages          # 列出 CDP targets
node scripts/cdp.mjs nav browse     # SPA 导航（内建 HMR 赛跑重试），打印落地路由
node scripts/cdp.mjs eval "<js>"    # 页面里求值（awaitPromise，返回值 JSON 取回）
node scripts/cdp.mjs shot detail x.png   # 导航 + 截图
node scripts/cdp.mjs probe browse   # 图健康 + 网络失败体检，裂图退出码 1
```

`eval` 拿 DOM 事实的例子：

```bash
# 某个交互后页面里还有什么（断言用）
node scripts/cdp.mjs eval "document.querySelectorAll('article[role=button]').length"
# 从页面里直接调 Tauri 命令，验证后端返回
node scripts/cdp.mjs eval "window.__TAURI_INTERNALS__.invoke('related_series',{seriesId:'7690537440955616281'}).then(r=>JSON.stringify(r).slice(0,300))"
```

## 自动化测试口径

**`probe` 的 PASS/FAIL 规则**（退出码可直接接 CI / 收尾自测）：

- 裂图：`img.complete && src 非空 && naturalWidth === 0` → FAIL。
  请求成功但解码失败就是这个形状（HEIC 类问题），是最容易漏到线上的那种坏。
- 图片请求 `Network.loadingFailed` → FAIL。
- 懒加载没触发的图（`complete=false`）不算失败；`0x0` 未布局只提示不判死。

**改动后的回归惯例**：主要路由各跑一遍 `probe`，交互改动用 `eval` 断言关键 DOM，
视觉存疑用 `shot` 留档对比：

```bash
for r in . browse rank new history collections; do node scripts/cdp.mjs probe "$r"; done
```

## 实战案例：2026-10-07「Windows 封面全裂」

症状：找剧页与详情相关推荐在 Windows 无封面，macOS 正常。

探针结论：`<img>` 请求全部 HTTP 200，但 `naturalWidth=0` → **网络没问题，是解码失败**——
接口给的封面是 HEIC，WebView2 没装 HEVC 扩展解不了；macOS 的 WebKit 原生能解，
把 bug 挡了两个平台周期。修复是封面统一走 `SeriesCover` 增强链（webp → 本地转码）。
这正是 `probe` 的 FAIL 规则瞄准的形状：以后 UI 改动收尾跑一遍 probe 就能把同类问题
挡在提交前。

## 坑清单（每条都真踩过）

- **MSYS 路径转换**：Git Bash 里以 `/` 开头的命令行参数会被改写成本机路径。
  路由参数一律不带前导斜杠（`browse` 不是 `/browse`）；`eval` 的 JS 里没这问题。
- **HMR 赛跑**：vite 全量重载会把 `location` 赋值抢回去，导航后必须校验
  `location.pathname`，不对就再试（`nav` 内建了三次重试）。
- **Radix Tabs 合成激活**：程序化切 tab 要派发 `pointerdown`（`click` 无效）。
- **数字 search 参数**：TanStack Router 默认序列化会把纯数字 JSON.parse 成 number——
  深链 `?seriesId=<纯数字>` 会变空。`/detail` 的 `validateSearch` 已兜底，新路由照抄。
- **Network 事件时序**：`Network.enable` 之前发生的请求收不到；probe 是导航前开
  监听，冷启动首页的首屏请求会漏——以落地路由之后的事件为准。

## 与 computer-use 的分工

CDP 够用就 CDP（默认）：DOM/网络/截图/求值，可断言可自动化。
必须走 computer-use 的场景：CDP 没开的已运行实例、原生窗口装饰/系统对话框、
验证 UIA 可达性本身。
