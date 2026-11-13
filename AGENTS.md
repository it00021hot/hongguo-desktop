# AGENTS.md — hongguo-desktop

红果短剧（Hongguo）非官方桌面客户端：Tauri 2 官方壳（React 19 WebView）+ GPUI 自绘壳（GPUI Kit）+ 纯 Rust 业务内核。Windows / macOS，另有 Android 移动端路线（Tauri 壳 arm64 APK，CI 手动触发构建，详见 workflow.md「Android 构建」节）。仅供学习研究（PolyForm Noncommercial），不存储、不分发任何影视内容。

## 开始任何开发前

NSH 第三壳正在开发（NSH 0.4.0，纯 Rust）：`crates/hongguo-nsh` 是共享界面与装配，
`hongguo-nsh-android` / `hongguo-nsh-ios` 是独立移动端宿主。首版目标为 Windows、macOS、
Android、iOS，Linux/Web 后续迭代。开发前另读 `nsh` 与 `nsh-design-guides`，
项目接入与验证说明见 `.agents/skills/hongguo-dev/references/nsh.md`。

Tauri、GPUI、NSH 的播放服务固定使用内置解码能力，无壳选择开关或系统 HEVC 探测分支。
`PlayResponse.direct` 表示内置视频解码，
系统媒体只读取原 MP4 的音轨视图并提供时钟；H.264 不参与播放或下载选流。
下载记录中的旧文件失效时重新获取在线原流，显式打开不支持的本地文件则报错。
同规格原流无损合并；异规格素材统一编码为 HEVC，完成封存普通 MP4 后再拼接。
禁止恢复兼容播放转码、`.play.mp4`、grow/HLS 播放链路。

**动手前先读两个 skill：用户级 `dev-habits`（跨项目工程宪法，经 Skill 工具调用）→ `.agents/skills/hongguo-dev/SKILL.md`（本项目规范，只写 dev-habits 没有的内容）**，两者互补缺一不可，再按任务读对应参考文档：

> 用户级 `dev-habits` 等 skill 装自 <https://github.com/it00021hot/skills>；新机器缺失时补装：`npx skills add it00021hot/skills@dev-habits -g`（`-g` 装到 `~/.agents/skills/`，不带则装进当前项目）。`find-skills` / `impeccable` 不在此仓库，来自社区源。

| 你要做的事                                                                       | 先读                                                                                                                                                       |
| -------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 任何开发动作（通用工程宪法：做事方式 / 质量 / 测试 / 安全 / 契约 / 日志 / 并发） | 用户级 `dev-habits` skill（门禁表在其 SKILL.md，先读它再读下表）                                                                                           |
| 环境搭建 / 日常命令 / 调试 / 提交 / 发版 / Android 构建                          | `.agents/skills/hongguo-dev/references/workflow.md`                                                                                                        |
| 新增（或改动）IPC 命令、事件、外部 API 端点                                      | `.agents/skills/hongguo-dev/references/ipc.md`                                                                                                             |
| 写前端：组件 / hooks / 查询 / store / 样式 / i18n / 路由                         | `.agents/skills/hongguo-dev/references/frontend.md`                                                                                                        |
| 写后端：command / service / store / 迁移 / 测试                                  | `.agents/skills/hongguo-dev/references/backend.md`                                                                                                         |
| 写 GPUI 壳界面 / 界面视觉验收                                                    | `.agents/skills/hongguo-dev/references/gpui.md`——`make gpui-preview` 离屏预览矩阵，改界面后必跑；**动可见界面前必须先调用 `gpui-kit-design-guides` skill** |
| 对接官方 App 接口（抓包 / 端点 / 参数）                                          | `docs/hongguo-api-endpoints.md`                                                                                                                            |
| 逆向分析官方 APK（协议 / 端点考证）                                              | `.agents/skills/hongguo-dev/references/reverse.md`                                                                                                         |
| WebView 调试 / CDP 自动化                                                        | `docs/webview-cdp-testing.md`                                                                                                                              |

## 目录结构

- `src/` 前端：React 19 + TS + Vite + TanStack Router/Query + Zustand + Tailwind v4 + shadcn/ui + zod。`pages/` + `features/` + `service/` + `stores/` + `components/`
- `crates/hongguo-tauri-app/` Tauri 壳的**纯配置 + 入口**：只承载 `tauri.conf.json` / `capabilities` / `icons` / `gen` 与 `generate_context!`（必须与配置同 crate），`lib.rs` 仅数行；全部装配见 `crates/hongguo-tauri/`
- `crates/hongguo-common/` 共用底座：领域模型（`model/`）、统一错误（`error.rs`，无 tauri 依赖）、纯工具（`utils/`）、跨端常量（`consts/`）。Tauri 壳与 GPUI 壳共用；常量归属规则见 `consts/mod.rs` 头注释
- `crates/hongguo-signer/` 请求签名：字节系五头签名 + 设备注册原语；**JS 1:1 移植禁改写法**，`constants.rs` 的值与服务端一一对应不得改动（无 common / tauri 依赖）
- `crates/hongguo-video/` MP4 解析与 CENC 解密：box/样本表/稀疏缓冲/流式计划 + CENC-AES-CTR + 密钥派生；`fixtures` 是 feature `test-support` 门控的测试辅助（下游测试造 MP4 样本用）
- `crates/hongguo-api/` 官方接口客户端。业务域 18 个：`calendar` / `danmaku` / `detail` / `discover` / `emoticon` / `history` / `interact` / `login` / `new_drama` / `play_strategy` / `play_url` / `rank` / `recommend` / `register` / `reservation` / `search` / `stream_pick` / `warmup`（另有 `client` / `params`）。改端点或参数前必读 `docs/hongguo-api-endpoints.md`。commentapi 互动枚举（object_type / comment_type / commit_source / data_type 等，值即契约）集中在 `params::interaction`，使用点只引用不写字面量
- `crates/hongguo-store/` 持久化：Turso 内嵌库（`db` 连接与 schema 迁移 / `entity/` 八个模块 `device` / `follow` / `local_history` / `merge` / `playback` / `series` / `settings` / `task` / `bridge` 同步门面）；`paths::ScopedDataDir` 是 feature `test-support` 门控的测试辅助（落盘用例把数据目录重定向到临时目录）。无 tauri 依赖
- `crates/hongguo-stream/` 在线流契约：内存渐进流（`ProgressiveStream`）、按 (vid, 档位) 分条的缓存（`StreamCache`）、Range 应答（`serve`）+ `range`（RangeSpec/ProtocolResponse）、进程级 `cache()` 单例。**填充侧（service）与应答侧（protocol）共用的上游**——两侧都只往下依赖它，禁止反向。无 tauri 依赖
- `crates/hongguo-bytevc2/` ByteVC2（`bvc2`）识别、参数集/样本 Annex-B 转换（对照 librempeg `cbs_bvc2` + 官方 `convertByteVC2Extradata`）与解帧（进程内 vvdec，`oxideav` 为回退）；两种解核共用的 8-bit I420 帧与缓冲池（`DecodedFrame` / `FramePool`）也在这里
- `crates/hongguo-softdec/` 进程内软解直播放会话，两壳共用、不分编码：读 `moov` 建样本索引、关键帧跳转、「只剩音轨」的 MP4 视图、解核分派（HEVC / ByteVC1 → `rusty_h265`，ByteVC2 → `hongguo-bytevc2` 的 vvdec）。数据由调用方回调供给，不碰网络与解密；`tests/fixtures/` 是合成的标准码流小样本。无 tauri/gpui 依赖
- `crates/hongguo-media/` 媒体公共底座：demux / hevc / mp4（MP4 写出层）/ mux（HEVC 封装，`ArchiveHevc.finish` 封存普通 faststart MP4）/ remux / transcode / codec_probe / disk_space / heif / backend；`tests/fixtures/` 放随代码走的样本。无 tauri 依赖
- `crates/hongguo-media-platform-api/` 平台媒体契约：`PlatformRequest` / 共享码率策略（`bitrate_for`）。实现 crate 的公共上游；本 crate 不依赖任何实现，引用方向反了就是 Cargo 依赖环
- `crates/hongguo-media-platform-macos/` macOS 平台实现：VideoToolbox 转码链（硬编优先、软编回落）；平台依赖全在 target 表，`#![cfg(target_os = "macos")]`，异目标空壳
- `crates/hongguo-media-platform-windows/` Windows 平台实现：Media Foundation HEVC 归档编解码 + WIC 封面静图；编码能力以真实 HEVC 编码探测，`#![cfg(target_os = "windows")]`，异目标空壳
- `crates/hongguo-media-platform-android/` Android 平台实现：MediaCodec HEVC 归档编解码（硬/软由系统选，契约同 media-platform-api）；`ndk` 依赖只进 android target 表，`#![cfg(target_os = "android")]`，异目标空壳
- `crates/hongguo-media-platform/` 平台层门面：按编译目标 re-export 实现 crate + 分派无状态操作（HEVC 归档转码 / 探测 / 清缓存 / HEIC 封面 `platform::cover`），`ffmpeg`（外部进程的探测与执行）与 `capability`（实际编码能力探测）聚合并留在此；转码两级分流编排仍在 service 的 `transcode_service::pipeline`。`platform::transcode_hevc_for_tests` 由 feature `test-support` 门控（壳 e2e 用）。无 tauri/gpui 依赖
- `crates/hongguo-protocol/` 自定义 URI 协议供数逻辑：`url`（按平台的 URL 形态与编解码）+ `local` + `cover`（HEIC 封面三级转码阶梯）+ `pool`（常驻工作池与 panic 加固）+ `serve`（三路供数入口，产出元组）。**无 tauri 依赖**——HTTP 组装与 Builder 注册是壳胶水（hongguo-tauri 的 `protocol.rs`）
- `crates/hongguo-service/` 业务编排层：`download` / `play` / `merge` / `login` / `series` / `settings` / `storage` / `transcode` / `capability` / `follow` / `history` 各有 `*_service`，另有 `device` 模块（设备档案）+ 全局业务状态 `AppState`（壳经 Tauri `manage` 注入，GPUI 壳直接构造）+ 共用任务运行时 `runtime`（后台任务任意线程可派发）+ 业务事件总线 `events`（`ServiceEvent`，壳订阅适配）。**不依赖任何 UI 框架**（源码静态闸扫 tauri 引用）：进度与状态只发布事件，发送机制由壳决定
- `crates/hongguo-tauri/` Tauri 装配层：`commands`（IPC 薄层）、`bootstrap`（启动装配）、`diagnostics`（panic 取证）、`protocol.rs`（协议注册胶水）、`events.rs`（service 事件 → 前端事件的原名原载荷转发）、`testing`（跨层 e2e 与守卫元测试）——`prepare()` 产出装好的 Builder，`build(generate_context!)` 留在 crates/hongguo-tauri-app
- `crates/hongguo-player-api/` 回放契约（无 UI、无平台实现依赖）：`PlayerEvent` / `PlaySource` / `PlayerLike`（统一控制面）/ `PresenterBounds` / `HostWindow`。系统播放器两端实现都吃这一份
- `crates/hongguo-player-macos/` macOS 内置软解播放器（`direct.rs` / `cv.rs`）：AVPlayer 只播音轨视图并充当时钟，`hongguo-softdec` 解出的帧经 `AVSampleBufferDisplayLayer` 上屏，唯一门面入口 `hongguo_player::open_direct`；无 AVPlayerLayer 系统视频路径。`#![cfg(target_os = "macos")]`，objc2/AppKit 依赖全在本 crate 的 target 表
- `crates/hongguo-player-windows/` Windows 播放器：HEVC/ByteVC1/ByteVC2 固定由 `hongguo-softdec` 解码，BGRA 帧写入同画布视频 quad；MediaEngine 只放音轨视图并提供时钟，无系统视频解码路径。Windows 真机运行仍待验。`#![cfg(target_os = "windows")]`，windows/objc2 互不越界
- `crates/hongguo-player/` 系统播放器门面：按编译目标分派到上面两个实现 crate，并把契约原样转出——壳只依赖它
- `crates/hongguo-gpui/` 完整 GPUI 壳：直接构造 `AppStateInner` 并调用 service/api，**操作与 Tauri 客户端一致**（不复制业务；`make gpui` 运行）。播放经 `hongguo-player` 内置软解，系统媒体只供音轨与时钟；service 的 async 族由壳内常驻 tokio 运行时经 oneshot 桥接，业务事件经 `events.rs` 事件泵（broadcast→mpsc）接进视图
- Rust 侧已 workspace 化：根 `Cargo.toml`（members / `resolver` / `[patch]` / `[profile]` 都在根），`Cargo.lock`、`target/`、`vendor/` 也都在仓库根；cargo 一律在**仓库根**跑（`--workspace`）。`vendor/` 被 `[workspace] exclude` 排除，不参与 lint/test/format
- `captures/` 抓包工具与数据（`addon.py` 抓包 / `audit.py` 参数审计），`docs/` 仓库文档，`scripts/` 构建辅助脚本

Tauri 壳的前后端只通过三条通道交互：IPC command、事件推送、自定义协议 `hongguo-local/stream/cover`；GPUI 壳进程内直调 service（tokio 桥 + 事件泵），无 IPC。

## 常用命令

```bash
make dev        # 开发模式（Vite + Tauri，端口 1420 固定、单实例）——需普通终端，受限沙箱内会失败
make dev-sandbox # 沙箱模式开发：profile/数据/临时目录全重定向到 target/（agent/受限终端专用）
make gpui       # 启动 GPUI 壳
make gpui-app   # GPUI 壳打 macOS .app（通知/Dock 身份需要 bundle；裸二进制跑不起来）
make gpui-preview # 离屏渲染 UI 预览矩阵到 /tmp/hongguo-gpui-preview（macOS only，改界面后必跑）
make test       # 前端 vitest（= pnpm test）
make test-rust  # cargo test
make lint       # 质量闸门一票否决：ESLint + Prettier + cargo clippy×3（Windows 宿主 + macOS 双目标交叉）
make typecheck  # tsc 两个 tsconfig --noEmit
make fmt        # cargo fmt
make release    # tauri build 打包
```

Android APK 平时走 CI 手动触发（`gh workflow run android.yml --ref dev`）；Windows 本地真机出包用 `bash scripts/android-build.sh`（三段绕法的背景见 workflow.md）。

包管理用 pnpm。门禁命令必须**裸跑**确认退出码——管道（如 `pnpm x | tail`）会吞掉失败。

## 硬红线（违反即返工，完整清单与背景见 skill）

1. command 层禁止 `tokio::spawn` / `Handle::current` / `block_on`——同步 command 跑在主线程 IPC 回调里没有 Tokio 上下文，会 panic→abort；后台任务一律走 service 的共用任务运行时 `hongguo_service::runtime`（command 内需要 Tauri 异步设施时用 tauri 的 async_runtime）。service 层同样不得裸用，且 **service 不引用任何 UI 框架**（两处都有静态测试扫源码盯着）。
2. 前端组件不直接 `invoke`——统一走 `service/commands/` 与 `service/tauri/invoke.ts` 的 `call<T>()`（契约表 `registry.ts` 编译期钉死，zod 校验 + 错误 i18n 归一）。
3. 事件名与载荷多处同步改：service 的 `ServiceEvent` 与名字常量（如 `crates/hongguo-service/src/download_service/events.rs` 的 `names`）↔ 壳转发 `crates/hongguo-tauri/src/events.rs` 的 `forward` ↔ 前端 `service/tauri/types.ts` 的 `EVENTS`（载荷 schema 在 `service/tauri/events.ts`），漏改一处会静默失效。
4. 签名（`signer/ticket.rs` 产出 x-gorgon 等五件套）之后不能再动 url/body/query——连 query 顺序、URL 编码方式都会失配；`signer/` 是 JS 1:1 移植，禁改写法。
5. 锁永远不跨 await（parking_lot guard 非 Send）。
6. Cargo release profile 绝不能加 `panic = "abort"`；clippy 带 `-D warnings -D clippy::allow_attributes`，确需豁免用 `#[expect(reason)]` 而非 `#[allow]`。
7. `crates/hongguo-tauri-app/src/lib.rs` 只做入口（数行）；装配在 `hongguo-tauri`，各 crate 边界显式定义对外 API，禁止 `pub use *` 泄底。
8. 启动顺序：单实例插件必须第一个注册，数据库在 `.setup()` 里晚于它打开。
9. 对接官方接口禁止猜参数——照 `docs/hongguo-api-endpoints.md` 抓包逐字段对齐。
10. 抓包对齐必须「请求 + 响应 + UI 效果」三点一线，只对齐请求等于没对齐：响应 body 逐字段解剖（发送类接口的响应是完整对象回显，丢弃=UI 缺字段）；计数类字段官方 App 做本地乐观 ±1；对照官方 App 实操效果核对渲染。
11. UI 用 shadcn `Button` variant + 语义令牌，禁止硬编码色值（`bg-red-500` 等）；唯一例外是播放器控制栏/互动栏/评论面板的播放器红色皮肤。
12. 禁止用 Python 脚本改项目代码——改代码只准 Edit/Write 工具；Python 可以写辅助工具（放工具目录正常提交），一次性分析脚本用完即删。
13. 开发在 dev 分支，conventional commits + 中文描述；纯结构移动/拆分与行为修改分开提交。
14. 大规模结构调整先出计划文档，经确认再动代码。

## 其他 gotcha

- 双壳可并存：`make gpui` 启动时经 `paths::set_shell_data_root("hongguo-gpui")` 把数据根钉到与默认根平级的独立目录（如 macOS `~/Library/Application Support/hongguo-gpui`，Tauri 壳用 `hongguo-downloader`），库/缓存互不共用、不撞库锁。
- Android 的桌面专属能力（single-instance / 全局快捷键 / 老板键 / rfd 选图选目录等）按平台门控，谓词统一 `#[cfg(not(any(target_os = "android", target_os = "ios")))]`——hongguo-tauri 无 build.rs，`#[cfg(desktop)]` 这类 tauri 注入 cfg 在该 crate 无效；数据根须在 setup 最先 `set_absolute_data_root` 钉到 app 私有目录（Android 上 `dirs::data_dir()` 为 None）。完整清单见 workflow.md「Android 构建」节。
- git hooks 需克隆后手动启用：`git config core.hooksPath .githooks`（pre-commit 跑三目标 clippy + cargo test + eslint）。
- CI 跑 dev 分支推送与 PR：三目标 clippy + cargo test + eslint + 新增文件 800 行上限闸门（`scripts/check-file-size.sh`，约 300 行是拆分提醒线）。
- 用户可见文案改动必须同步 `src/locales/zh-CN.json` 与 `en-US.json` 两份。
- 改名 / 换 logo 后重跑 `make assets`（离线，产物生成后提交）；升级 Tauri 后重跑 `make nsis`。
