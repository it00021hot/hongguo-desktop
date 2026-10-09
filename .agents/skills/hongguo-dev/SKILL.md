---
name: hongguo-dev
description: hongguo-desktop（红果短剧 Tauri 桌面客户端）项目开发 skill——开发流程、前后端开发规范、IPC 契约、调试与测试、提交纪律。凡在本仓库做任何开发（新增功能、修 bug、写组件/hooks/查询/Rust 命令、对接官方 API、调试 WebView、跑测试、打包发版）或执行 docs/refactor-plan.md 重构计划，都应先读本 skill。
---

# hongguo-desktop 开发 skill

Tauri 2 桌面应用（Windows/macOS）。前端 `src/`：React 19 + TS + Vite + TanStack Router/Query + Zustand + Tailwind v4 + shadcn/ui + zod。后端 `src-tauri/`：Rust，`commands(薄) → service(编排) → domain/api(外部接口) + store(持久化)` 分层。前后端只通过 IPC command、事件推送、自定义协议 `hongguo-local/stream/cover` 三条通道交互。

## 按任务读对应参考文档

| 你要做的事 | 先读 |
|---|---|
| 环境搭建 / 日常命令 / 调试 / 提交 / 发版 | `references/workflow.md` |
| 新增（或改动）IPC 命令、事件、外部 API 端点 | `references/ipc.md` |
| 写前端：组件、hooks、查询、store、样式、i18n、路由 | `references/frontend.md` |
| 写后端：command、service、store、迁移、测试 | `references/backend.md` |
| 对接官方 App 接口（抓包/端点/参数） | `docs/hongguo-api-endpoints.md`（仓库文档，改 domain/api 前必读） |
| 动转码/平台硬编（media/platform） | `docs/platform-transcode-progress.md`（仓库文档，动前必读） |
| 执行重构计划 | `docs/refactor-plan.md` + 见下文「重构执行」 |

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
2. **前端组件不直接 `invoke`**——统一走 `lib/ipc/commands.ts` 的 `call<T>()`（zod 校验 + 错误 i18n 归一），否则后端契约变更在类型检查时不会暴露。
3. **事件名两端同步改**——Rust `download_service/events.rs` 的 `names` 与前端 `lib/ipc/types.ts` 的 `EVENTS` 一一对应，只改一端订阅会静默失效。
4. **签名后不能再动 url/body/query**（`signer/ticket.rs` 产出 x-gorgon 等五件套后，连 query 顺序、URL 编码方式都会失配）。
5. **锁永远不跨 await**（parking_lot guard 非 Send，app_state.rs 模块注释是铁律）。
6. **Cargo release profile 绝不能加 `panic = "abort"`**——会让 wry objc2 catch 与 protocol 层 catch_unwind 两道防线静默失效。
7. **lib.rs 只做装配**；新模块在 lib.rs 保持私有（`mod xxx;` 无 pub）。
8. **启动顺序**：单实例插件必须第一个注册、数据库在 `.setup()` 里晚于它打开，否则二次启动撞库锁会被误报「数据损坏」。
9. **signer/ 是 JS 1:1 移植，禁改写法**；vendor/ 四个 patch 依赖有「上游修复后回收」约定。
10. **开发在 dev 分支**，conventional commits + 中文描述；纯移动/结构拆分与行为修改分开提交。

## 重构执行

仓库正按 `docs/refactor-plan.md` 从旧结构迁往 pages/features/service 目标结构。接到「继续重构/执行 P?」类任务：先读计划全文 → `git log --oneline` 对照进度（commit message 带阶段号，如 `refactor(service): P1-C1 …`）→ 严格按该步骤执行，**功能保持不变、纯移动不改逻辑** → 门禁通过后提交并标阶段号；发现 bug 单独 `fix:` 提交。动手前用目录探测当前处于旧结构（`src/routes/`、`src/lib/`）还是新结构（`src/pages/`、`src/service/`），新代码按所处阶段就近落位，别往旧巨石文件里继续堆。
