---
name: hongguo-dev
description: hongguo-desktop（红果短剧 Tauri 桌面客户端）项目开发规范与重构执行指南。凡在本仓库做任何开发——新增功能、修 bug、写组件/hooks/查询/Rust 命令、执行 docs/refactor-plan.md 重构计划的某个阶段、或不确定代码该放哪个目录——都应先读本 skill 再动手。
---

# hongguo-desktop 开发规范

## 项目速览

- Tauri 2 桌面应用（Windows/macOS）。前端 `src/`：React 19 + TS + Vite + TanStack Router/Query + Zustand + Tailwind v4 + shadcn/ui + zod。后端 `src-tauri/`：Rust，分层 `commands(薄) → service(编排) → domain/api(外部接口) + store(持久化)`。
- 前后端通道只有三条：Tauri IPC command（响应逐条过 zod 校验）、Rust→前端事件（`useEvent`）、自定义协议 `hongguo-local/stream/cover`（音视频数据面，绕 IPC）。
- 完整目标目录树与重构阶段定义见 [docs/refactor-plan.md](../../../docs/refactor-plan.md)，那是唯一事实源，本 skill 不复制它。

## 第一步：判断结构状态

本仓库正在从旧结构迁移到目标结构，动手前先探测（看目录即可）：

- 已迁移：存在 `src/pages/`、`src/service/` → 按下面「目录归属」的规则写。
- 未迁移/迁移中：仍是 `src/routes/`、`src/lib/ipc`、`src/lib/queries.ts`、`src/features/*` → 新代码按**现状就近**放置，不要往旧的巨石文件（`lib/queries.ts` 等）里继续堆；如果重构已进行到相关阶段，直接把新代码写进目标位置。

## 前端目录归属判定（新增代码放哪）

按顺序自问，命中即止：

1. **只服务于某个路由页面？** → `pages/<域>/` 就近放置；复杂页面才有 `components/`、`hooks/` 子目录，简单页面一个文件就够。
2. **复杂、会被多个页面复用的能力？** → `features/`。只有 player（播放引擎）、update 这类体量才配进；不按页面建 feature。
3. **和 Rust 交互 / 服务端数据？** → `service/`：IPC 封装在 `service/tauri/`，命令封装按域拆在 `service/commands/`，TanStack Query hooks 按域拆在 `service/queries/`（keys 随域走），zod 契约在 `service/schema/`。
4. **跨页面通用 hook？** → `hooks/`。**全局客户端状态？** → `stores/`（zustand）。**无业务语义的纯函数？** → `utils/`。
5. UI 基础件 `components/ui/`（shadcn，别手改生成风格）；布局壳 `components/layout/`；跨页展示组件 `components/common/`。
6. `cn()` 只活在 `lib/utils.ts`，别另起一份。

反面清单：不建"一个文件装所有域"的巨石；不造只有一行转发、没有实际价值的包装层；同一业务规则不许有两份实现（发现重复就收敛到一处）。

## 后端分层规则

- `commands/`：薄。只做参数校验和转调 service，禁止 `tokio::spawn`/`block_on`（有静态检查测试盯着）。
- `service/`：业务编排，一个服务一个目录。
- `domain/api/`：每域一目录 = `mod.rs`（端点函数+内嵌 tests）+ `model.rs`（struct/serde）+ `parse.rs`（复杂 JSON 解析才有，不强制）。
- `utils/`：只收无业务语义的纯函数（json/time/hex/url 这类）。准入标准：不依赖 AppState、不依赖领域类型；域内细节（如 mp4 的字节读取）留在域内不上提。
- 新增 IPC 命令：后端 `lib.rs` 的 `generate_handler!` 挂载 + 前端 `service/commands/`（旧结构为 `lib/ipc/commands.ts`）同步加封装，两端事件名常量保持一一对应。

## 提交与分支纪律

- 开发一律在 `dev` 分支；`main` 只进验收过的内容。
- Conventional commits + 中文描述：`feat(player): …`、`fix(feed): …`、`refactor(service): …`、`docs: …`。
- 纯移动/结构拆分与行为修改**分开提交**；bug 修复独立 `fix:` commit 并写明根因。
- 每 commit 必须编译绿：前端 `pnpm typecheck && pnpm test && pnpm lint`；后端 `cargo check && cargo test`。

## 验证门禁

- 前端：`pnpm typecheck` / `pnpm test`（vitest）/ `pnpm lint` / `pnpm knip`（死代码）；阶段收尾跑 `pnpm build`。
- 后端：`cargo check` / `cargo test` / `cargo clippy`。存量告警有基线记录，区分"存量"与"新增"，不混着修。
- 播放器（features/player）改动最敏感：抽逻辑进 hooks 时渲染结构不动；完成后用 `pnpm tauri:dev` 做冒烟（首页→详情→起播→切集/清晰度→弹幕）。

## 重构执行模式

当任务是"执行重构 / 继续 P3 / 下一个阶段"这类时：

1. 读 [docs/refactor-plan.md](../../../docs/refactor-plan.md) 全文，确定当前阶段（P0~P6）和其中的步骤号（C1~C10）。
2. `git log --oneline` 对照已完成到哪一步（commit message 里带阶段号，如 `refactor(service): P1-C1 …`）。
3. 严格按该步骤执行：**功能保持不变，纯移动不改逻辑**；rank.rs 拆域、stream.rs 拆职责这类按计划里的映射走。
4. 门禁通过后提交，message 标注阶段号；发现 bug 不混进移动 commit，按「提交纪律」单独 fix。
5. 红线（计划"明确不做"）：不动 `signer/`、`media/platform/{mf,vt}`、`src-tauri/vendor/`、commands 薄层约定；不改业务行为/UI/文案；不 monorepo 化。

## 常见坑

- IPC 响应必须走 `call<T>()`（zod 校验 + 错误归一），组件不要直接 `invoke`。
- `routeTree.gen.ts` 是生成物，不要手改；路由目录迁移后 vite.config.ts 的 `tanstackRouter` 配置要同步。
- 剧集 id 是 i64 精度：前端自定义 `parseSearch/stringifySearch` 做了防精度丢失，别绕开它传裸数字。
- 弹幕表情表 `utils/danmaku-emoji` 是生成物，改源脚本别改产物。
