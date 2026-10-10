---
name: hongguo-dev
description: hongguo-desktop（红果短剧 Tauri 桌面客户端）项目开发 skill——开发流程、前后端开发规范、IPC 契约、调试与测试、提交纪律。凡在本仓库做任何开发（新增功能、修 bug、写组件/hooks/查询/Rust 命令、对接官方 API、调试 WebView、跑测试、打包发版），都应先读本 skill。
---

# hongguo-desktop 开发 skill

Tauri 2 桌面应用（Windows/macOS）。前端 `src/`：React 19 + TS + Vite + TanStack Router/Query + Zustand + Tailwind v4 + shadcn/ui + zod。后端 `src-tauri/`：Rust，`commands(薄) → service(编排) → domain/api(外部接口) + store(持久化)` 分层。前后端只通过 IPC command、事件推送、自定义协议 `hongguo-local/stream/cover` 三条通道交互。

## 按任务读对应参考文档

| 你要做的事                                         | 先读                                                              |
| -------------------------------------------------- | ----------------------------------------------------------------- |
| 环境搭建 / 日常命令 / 调试 / 提交 / 发版           | `references/workflow.md`                                          |
| 新增（或改动）IPC 命令、事件、外部 API 端点        | `references/ipc.md`                                               |
| 写前端：组件、hooks、查询、store、样式、i18n、路由 | `references/frontend.md`                                          |
| 写后端：command、service、store、迁移、测试        | `references/backend.md`                                           |
| 对接官方 App 接口（抓包/端点/参数）                | `docs/hongguo-api-endpoints.md`（仓库文档，改 domain/api 前必读） |
| 动转码/平台硬编（media/platform）                  | `docs/platform-transcode-progress.md`（仓库文档，动前必读）       |

参考文档没覆盖的判断，回到两条元规则：**同一段逻辑只有一个明确归属**；**不为目录整齐制造转发文件**。

## 快速命令（详见 workflow.md）

```bash
make dev          # 启动开发模式（Vite + Tauri，端口 1420 固定、单实例）
make test         # 前端 vitest
make test-rust    # cargo test
make lint         # 质量闸门（一票否决）：ESLint + Prettier + cargo clippy×3（含 macOS 双目标交叉）
make release      # 打包发布版
```

## 硬红线（违反即返工）

1. **command 层禁止 `tokio::spawn`/`Handle::current`/`block_on`**——同步 command 跑在主线程 IPC 回调里没有 Tokio 上下文，会 panic→abort（2026-10-07 隐身闪退实录）。后台任务一律 `tauri::async_runtime::spawn`。有静态测试扫源码盯着。
2. **前端组件不直接 `invoke`**——统一走 `service/commands/`（按域拆分的封装）与 `service/tauri/invoke.ts` 的 `call<T>()`（zod 校验 + 错误 i18n 归一），否则后端契约变更在类型检查时不会暴露。
3. **事件名两端同步改**——Rust `download_service/events.rs` 的 `names` 与前端 `service/tauri/types.ts` 的 `EVENTS` 一一对应，只改一端订阅会静默失效。
4. **签名后不能再动 url/body/query**（`signer/ticket.rs` 产出 x-gorgon 等五件套后，连 query 顺序、URL 编码方式都会失配）。
5. **锁永远不跨 await**（parking_lot guard 非 Send，app_state.rs 模块注释是铁律）。
6. **Cargo release profile 绝不能加 `panic = "abort"`**——会让 wry objc2 catch 与 protocol 层 catch_unwind 两道防线静默失效。
7. **lib.rs 只做装配**；新模块在 lib.rs 保持私有（`mod xxx;` 无 pub）。
8. **启动顺序**：单实例插件必须第一个注册、数据库在 `.setup()` 里晚于它打开，否则二次启动撞库锁会被误报「数据损坏」。
9. **signer/ 是 JS 1:1 移植，禁改写法**；vendor/ 四个 patch 依赖有「上游修复后回收」约定。
10. **开发在 dev 分支**，conventional commits + 中文描述；纯移动/结构拆分与行为修改分开提交。
11. **新 UI 必须保持项目风格**：按钮一律 shadcn `Button` variant + 语义令牌，禁止硬编码色值（`bg-red-500` 等）——唯一例外是播放器控制栏/互动栏/评论面板的 hgplayer 红色皮肤；详见 `references/frontend.md` 样式节。
12. **对接官方接口禁止猜参数**——照 `docs/hongguo-api-endpoints.md` 抓包逐字段对齐（audit.py 出参数表），103008「无社区功能」这类报错八成是形态不对而不是权限。
13. **抓包对齐必须「请求 + 响应 + UI 效果」三点一线，只对齐请求等于没对齐**（2026-10-10 剧评回复返工实录）：① 响应 body 逐字段解剖——发送类接口（comment/add、reply/add）的响应是**完整对象回显**（头像/昵称/uid/时间/计数/评分），第三方把它整个插进列表当 UI 数据源，只取 code/id 丢弃对象 = 自己手拼简陋条目 = 缺 userId 删除按钮出不来、样式与第三方两样；② 计数类字段（digg_count 等）服务端有延迟，第三方 UI 是本地 ±1 乐观更新，不抄这层等于「点了没反应」；③ 对照第三方实操截图/逆向代码核对**渲染效果**（展开后长什么样、置顶不置顶、回显什么字段），「接口能通」不等于「效果对齐」。逆向第三方 bundle 时把它的 onSuccess 数据流（响应对象流向哪个缓存/列表）一并拆出来。
14. **禁止用 Python 脚本改项目代码**——没有状态管理，会静默丢写入、混入错字（本仓库多次事故）。改代码一律 Edit/Write 工具。Python 照常可以**写辅助工具**（如 `captures/addon.py` 抓包、`captures/audit.py` 参数审计，放工具目录正常提交）；只做一次性分析的临时脚本用完即删。提交前的门禁命令必须**裸跑**确认退出码——`pnpm x | tail && commit` 这类管道会吞掉失败，曾把坏状态带进提交。

## 目录归属（结构规范）

前端 `pages/ + features/{player,update} + service/{tauri,commands,queries,schema} + stores/locales/utils/hooks`，后端 `domain/api/<域>/` 目录化 + `utils/` 工具层。新代码按 `references/frontend.md` 的「目录归属判定」落位：同一逻辑只有一个明确归属，不堆巨石文件、不造转发层；大规模结构调整先出计划文档经确认再动代码。
