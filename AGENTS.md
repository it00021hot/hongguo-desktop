# AGENTS.md — hongguo-desktop

红果短剧（Hongguo）非官方桌面客户端：Tauri 2 + 纯 Rust 业务内核 + React 19 界面层。Windows / macOS。仅供学习研究（PolyForm Noncommercial），不存储、不分发任何影视内容。

## 开始任何开发前

**先读 `.agents/skills/hongguo-dev/SKILL.md`**（完整开发规范），再按任务读对应参考文档：

| 你要做的事                                               | 先读                                                |
| -------------------------------------------------------- | --------------------------------------------------- |
| 环境搭建 / 日常命令 / 调试 / 提交 / 发版                 | `.agents/skills/hongguo-dev/references/workflow.md` |
| 新增（或改动）IPC 命令、事件、外部 API 端点              | `.agents/skills/hongguo-dev/references/ipc.md`      |
| 写前端：组件 / hooks / 查询 / store / 样式 / i18n / 路由 | `.agents/skills/hongguo-dev/references/frontend.md` |
| 写后端：command / service / store / 迁移 / 测试          | `.agents/skills/hongguo-dev/references/backend.md`  |
| 对接官方 App 接口（抓包 / 端点 / 参数）                  | `docs/hongguo-api-endpoints.md`                     |
| 动转码 / 平台硬编（media / platform）                    | `docs/platform-transcode-progress.md`               |
| WebView 调试 / CDP 自动化                                | `docs/webview-cdp-testing.md`                       |

## 目录结构

- `src/` 前端：React 19 + TS + Vite + TanStack Router/Query + Zustand + Tailwind v4 + shadcn/ui + zod。`pages/` + `features/` + `service/` + `stores/` + `components/`
- `src-tauri/` 后端：Rust，分层 `commands(薄) → service(编排) → domain/api(外部接口) + store(持久化)`
- `captures/` 抓包工具与数据（`addon.py` 抓包 / `audit.py` 参数审计），`docs/` 仓库文档，`scripts/` 构建辅助脚本

前后端只通过三条通道交互：IPC command、事件推送、自定义协议 `hongguo-local/stream/cover`。

## 常用命令

```bash
make dev        # 开发模式（Vite + Tauri，端口 1420 固定、单实例）
make test       # 前端 vitest（= pnpm test）
make test-rust  # cargo test
make lint       # 质量闸门一票否决：ESLint + Prettier + cargo clippy×3（Windows 宿主 + macOS 双目标交叉）
make typecheck  # tsc 两个 tsconfig --noEmit
make fmt        # cargo fmt
make release    # tauri build 打包
```

包管理用 pnpm。门禁命令必须**裸跑**确认退出码——管道（如 `pnpm x | tail`）会吞掉失败。

## 硬红线（违反即返工，完整清单与背景见 skill）

1. command 层禁止 `tokio::spawn` / `Handle::current` / `block_on`——同步 command 跑在主线程 IPC 回调里没有 Tokio 上下文，会 panic→abort；后台任务一律 `tauri::async_runtime::spawn`（有静态测试扫源码盯着）。
2. 前端组件不直接 `invoke`——统一走 `service/commands/` 与 `service/tauri/invoke.ts` 的 `call<T>()`（zod 校验 + 错误 i18n 归一）。
3. 事件名两端同步改：Rust `download_service/events.rs` 的 `names` ↔ 前端 `service/tauri/types.ts` 的 `EVENTS`，只改一端会静默失效。
4. 签名（`signer/ticket.rs` 产出 x-gorgon 等五件套）之后不能再动 url/body/query——连 query 顺序、URL 编码方式都会失配；`signer/` 是 JS 1:1 移植，禁改写法。
5. 锁永远不跨 await（parking_lot guard 非 Send）。
6. Cargo release profile 绝不能加 `panic = "abort"`；clippy 带 `-D warnings -D clippy::allow_attributes`，确需豁免用 `#[expect(reason)]` 而非 `#[allow]`。
7. `lib.rs` 只做装配，新模块保持私有（`mod xxx;` 无 pub）。
8. 启动顺序：单实例插件必须第一个注册，数据库在 `.setup()` 里晚于它打开。
9. 对接官方接口禁止猜参数——照 `docs/hongguo-api-endpoints.md` 抓包逐字段对齐。
10. 抓包对齐必须「请求 + 响应 + UI 效果」三点一线，只对齐请求等于没对齐：响应 body 逐字段解剖（发送类接口的响应是完整对象回显，丢弃=UI 缺字段）；计数类字段第三方做本地乐观 ±1；对照第三方实操效果核对渲染。
11. UI 用 shadcn `Button` variant + 语义令牌，禁止硬编码色值（`bg-red-500` 等）；唯一例外是播放器控制栏/互动栏/评论面板的 hgplayer 红色皮肤。
12. 禁止用 Python 脚本改项目代码——改代码只准 Edit/Write 工具；Python 可以写辅助工具（放工具目录正常提交），一次性分析脚本用完即删。
13. 开发在 dev 分支，conventional commits + 中文描述；纯结构移动/拆分与行为修改分开提交。
14. 大规模结构调整先出计划文档，经确认再动代码。

## 其他 gotcha

- git hooks 需克隆后手动启用：`git config core.hooksPath .githooks`（pre-commit 跑三目标 clippy + cargo test + eslint）。
- 用户可见文案改动必须同步 `src/locales/zh-CN.json` 与 `en-US.json` 两份。
- 改名 / 换 logo 后重跑 `make assets`（离线，产物生成后提交）；升级 Tauri 后重跑 `make nsis`。
