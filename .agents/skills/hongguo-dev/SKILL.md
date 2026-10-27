---
name: hongguo-dev
description: hongguo-desktop（红果短剧双壳桌面客户端：Tauri 2 官方壳 + GPUI 自绘壳）项目开发 skill——开发流程、前后端开发规范、IPC 契约、调试与测试、提交纪律。凡在本仓库做任何开发（新增功能、修 bug、写组件/hooks/查询/Rust 命令、写 GPUI 壳界面、对接官方 API、调试 WebView、跑测试、打包发版），都应先读本 skill。本 skill 只写用户级 dev-habits skill 没有的项目特有规范，两者互补——任何开发动作先读 dev-habits 再读本 skill，缺一不可。
when_to_use: 在本仓库做任何开发（新增功能、修 bug、写组件/hooks/查询/Rust 命令、写 GPUI 壳界面、对接官方 API、调试 WebView、跑测试、打包发版）前；与用户级 dev-habits skill 互补双读。
---

# hongguo-desktop 开发 skill

双壳桌面应用：**Tauri 2 壳**（React 19 WebView：前端 `src/`，壳装配 `crates/hongguo-tauri` / `crates/hongguo-tauri-app`）与 **GPUI 自绘壳**（`crates/hongguo-gpui`，GPUI Kit）。两壳共用同一段服务面：后端 `crates/`（Rust workspace）`hongguo-service`（编排）→ `hongguo-api` + `hongguo-store` 分层。Tauri 壳与前端只通过 IPC command、事件推送、自定义协议 `hongguo-local/stream/cover` 三条通道交互；GPUI 壳进程内直调 service（async 经 tokio 桥、业务事件经事件泵），**没有 IPC**。

## 与用户级 dev-habits skill 的分工（先读它，再读本 skill）

用户级 `dev-habits` skill 是跨项目工程宪法（做事方式、代码质量、测试、安全 T0、传输契约、日志、性能并发、交流方式，门禁表见其 SKILL.md），**任何开发动作先读它**；本 skill 只写它没有的本仓库特有内容——双壳架构、IPC/协议契约、项目红线、目录归属、命令与发版。两者互补双读；本 skill 不重复 dev-habits 条款，重复处已删、以『条款名』指针代替（指向 dev-habits 的对应条款）。冲突按 dev-habits 仲裁阶梯裁决：项目约定（T1）优先于其 T2 条款，安全（T0）不可覆盖。

## 先判断动的是哪个壳，再按任务读对应参考文档

| 你要做的事                                                | 先读                                                                                                                              |
| --------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| **任何开发动作（通用工程宪法）**                          | 用户级 `dev-habits` skill——做事方式/代码质量/测试/安全/传输契约/日志/性能并发的门禁表在其 SKILL.md，先读它再读本表                 |
| **GPUI 壳**：界面 / 视图 / 主题 / 交互 / 壳装配（crates/hongguo-gpui） | `references/gpui.md`；**动可见界面前必须先经 Skill 工具调用 gpui-ui-design skill**（红线 15）                                      |
| **Tauri 壳前端**：组件、hooks、查询、store、样式、i18n、路由 | `references/frontend.md`                                                                                                          |
| 新增（或改动）IPC 命令、事件、外部 API 端点               | `references/ipc.md`（IPC / 协议是 Tauri 壳专属；外部 API 端点两壳共用）                                                           |
| 写后端：command、service、store、迁移、测试               | `references/backend.md`（command 层仅 Tauri；service/store 两壳共用）                                                             |
| 环境搭建 / 日常命令 / 调试 / 提交 / 发版                  | `references/workflow.md`（发版仅 Tauri；GPUI 壳调试与验收在 `gpui.md`）                                                           |
| 对接官方 App 接口（抓包/端点/参数）                       | `docs/hongguo-api-endpoints.md`（仓库文档，改 domain/api 前必读）                                                                 |
| 逆向分析官方 APK（协议/端点考证）                         | `references/reverse.md`                                                                                                           |

参考文档没覆盖的判断，回到两条元规则：**同一段逻辑只有一个明确归属**；**不为目录整齐制造转发文件**。

## 快速命令（详见 workflow.md）

```bash
make dev          # 启动开发模式（Vite + Tauri，端口 1420 固定、单实例）
make gpui         # 启动 GPUI 壳
make gpui-preview # GPUI 壳界面验收：离屏预览矩阵出 PNG（改界面后必跑，见 references/gpui.md）
make test         # 前端 vitest
make test-rust    # cargo test
make lint         # 质量闸门（一票否决）：ESLint + Prettier + cargo clippy×3（含 macOS 双目标交叉）
make release      # 打包发布版（Tauri 壳）
```

## 硬红线（违反即返工）

### 两壳共用

1. **command 层禁止 `tokio::spawn`/`Handle::current`/`block_on`**——同步 command 跑在主线程 IPC 回调里没有 Tokio 上下文，会 panic→abort（2026-10-07 隐身闪退实录）。后台任务一律走 service 共用运行时 `hongguo_service::runtime`（command 内需要 Tauri 异步设施时用 tauri 的 async_runtime）；service 层同样禁裸用，且不得引用任何 UI 框架（静态闸）。有静态测试扫源码盯着。
4. **签名后不能再动 url/body/query**（`signer/ticket.rs` 产出 x-gorgon 等五件套后，连 query 顺序、URL 编码方式都会失配）。
5. **锁永远不跨 await**（parking_lot guard 非 Send，app_state.rs 模块注释是铁律）。
6. **Cargo release profile 绝不能加 `panic = "abort"`**——会让 wry objc2 catch 与 protocol 层 catch_unwind 两道防线静默失效。
9. **signer/ 是 JS 1:1 移植，禁改写法**；vendor/ 四个 patch 依赖有「上游修复后回收」约定。
10. **开发在 dev 分支**；提交信息按 dev-habits『commit message 规范』（Conventional Commits），本项目附加中文描述；纯移动/结构拆分与行为修改分开提交。
12. **对接官方接口禁止猜参数**——照 `docs/hongguo-api-endpoints.md` 抓包逐字段对齐（audit.py 出参数表），103008「无社区功能」这类报错八成是形态不对而不是权限。
13. **抓包对齐必须「请求 + 响应 + UI 效果」三点一线，只对齐请求等于没对齐**（2026-10-10 剧评回复返工实录）：① 响应 body 逐字段解剖——发送类接口（comment/add、reply/add）的响应是**完整对象回显**（头像/昵称/uid/时间/计数/评分），官方 App 把它整个插进列表当 UI 数据源，只取 code/id 丢弃对象 = 自己手拼简陋条目 = 缺 userId 删除按钮出不来、样式与官方 App 两样；② 计数类字段（digg_count 等）服务端有延迟，官方 App UI 是本地 ±1 乐观更新，不抄这层等于「点了没反应」；③ 对照官方 App 实操截图/逆向代码核对**渲染效果**（展开后长什么样、置顶不置顶、回显什么字段），「接口能通」不等于「效果对齐」。逆向官方 APK 时把它的 onSuccess 数据流（响应对象流向哪个缓存/列表）一并拆出来。
14. **改代码用精确编辑**（dev-habits『改代码用精确编辑』；本仓库多次事故：脚本盲改静默丢写入、混入错字）。Python 照常可以**写辅助工具**（如 `captures/addon.py` 抓包、`captures/audit.py` 参数审计，放工具目录正常提交）；只做一次性分析的临时脚本用完即删。提交前的门禁命令必须**裸跑**确认退出码——`pnpm x | tail && commit` 这类管道会吞掉失败，曾把坏状态带进提交。

### Tauri 壳专属

2. **前端组件不直接 `invoke`**——统一走 `service/commands/`（按域拆分的封装）与 `service/tauri/invoke.ts` 的 `call<T>()`（契约表 `service/tauri/registry.ts` 把命令名/入参/响应 schema 编译期钉死，zod 校验 + 错误 i18n 归一），否则后端契约变更在类型检查时不会暴露。
3. **事件名与载荷多处同步改**——service 发 `ServiceEvent`（名字常量在各域 events 模块）→ 壳 `crates/hongguo-tauri/src/events.rs` 的 `forward` 映射 → 前端 `service/tauri/types.ts` 的 `EVENTS`（载荷 schema 在 `service/tauri/events.ts`），漏改一处订阅静默失效。
7. **lib.rs 只做装配**；新模块在 lib.rs 保持私有（`mod xxx;` 无 pub）。
8. **启动顺序**：单实例插件必须第一个注册、数据库在 `.setup()` 里晚于它打开，否则二次启动撞库锁会被误报「数据损坏」。
11. **新 UI 必须保持项目风格**：按钮一律 shadcn `Button` variant + 语义令牌，禁止硬编码色值（`bg-red-500` 等）——唯一例外是播放器控制栏/互动栏/评论面板的播放器红色皮肤；详见 `references/frontend.md` 样式节（GPUI 壳的对应条在 `references/gpui.md` 界面规范）。

### GPUI 壳专属

15. **动 GPUI 壳可见界面前，必须先经 Skill 工具调用 `gpui-ui-design` skill**（按其指引读 design-guides——表格单元格里的「另见」不会触发调用，必须显式 invoke）；界面改动的验收一律 `make gpui-preview` 离屏预览矩阵逐张目检。壳内其余纪律（语义令牌、RenderImage、preview 矩阵扩展三步、cargo test 与 bin）见 `references/gpui.md`。

## 目录归属（结构规范）

前端 `pages/ + features/{player,update} + service/{tauri,commands,queries,schema} + stores/locales/utils/hooks`，`hongguo-api` 按域目录平铺（`client.rs` 统一入口 + `params.rs` 请求常量），GPUI 壳按域拆子模块（参照 `crates/hongguo-gpui/src/home/` 的模块树）。新代码按 `references/frontend.md` 的「目录归属判定」落位：同一逻辑只有一个明确归属，不堆巨石文件、不造转发层；大规模结构调整先出计划文档经确认再动代码。

**平台实现一律分 crate**（与媒体层 `hongguo-media-platform-*` 同一口径，不为目录整齐造转发文件）：

- 回放契约 `hongguo-player-api`（`PlayerEvent`/`PlaySource`/`PlayerLike`/舞台几何/`HostWindow`）——不吃 UI、不吃平台实现；
- 系统播放器实现 `hongguo-player-macos`（AVPlayer）/ `hongguo-player-windows`（MediaEngine + DComp 视频位），平台依赖全在各自 target 表，`#![cfg(target_os = …)]`；
- 门面 `hongguo-player` 按 target 分派；壳只依赖门面，`#[cfg(target_os)]` 只允许出现在门面与 `platform` 门面里，业务/视图层出现平台分支即返工。

GPUI 壳本身是单 crate；壳内**唯一**允许的平台代码是 `platform/`（业务语义门面，两端各有实现文件）与 `av_spike/`（macOS Phase 0 诊断，`HONGGUO_AV_SPIKE=1`）。新增平台能力先问「能不能进 `hongguo-player-*` / `hongguo-media-platform-*`」，不能才考虑 `platform/`。
