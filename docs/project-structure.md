# 项目结构 / Project structure

当前主线使用 NSH 0.5.1 与纯 Rust 业务内核。旧 Tauri、GPUI 和 WebView 专用代码仅保留在备份分支。

## 分层

| 层 | 位置 | 职责 |
| --- | --- | --- |
| 装配 | `hongguo-nsh/src/{lib.rs,bootstrap.rs,shell/,navigation.rs}` | 根组件、服务装配、窗口及路由 |
| 功能 | `hongguo-nsh/src/features/<功能>/` | 页面状态、交互和该功能的 `presentation/` |
| 共享控件 | `hongguo-nsh/src/ui/` | 按钮、输入、选择器、浮层、图标、动效与复用的列表/骨架控件 |
| 业务 | `hongguo-service` | 内容读取、片库、预约、表情、评论、互动、账号、下载和归档 |
| 接口 | `hongguo-api` | 官方请求、签名接入与响应解析 |
| 持久化 | `hongguo-store` | 数据库、仓储和数据目录 |
| 公共基础 | `hongguo-common` | 模型、错误、文件 IO 和纯工具 |
| 播放与呈现 | `hongguo-playback`、`hongguo-nsh-video-*`、`hongguo-native-clock` | 解码调度、原生呈现、音轨和时钟 |
| 媒体能力 | `hongguo-media-*`、`hongguo-video*`、解码 crate | 容器、媒体类型、GPU、增强与编解码 |

功能目录包含 `browse`、`catalog`、`creator`、`detail`、`downloads`、`emoji`、`history`、`library`、`maintenance`、`new_drama`、`player`、`rank` 和 `settings`。
历史与下载各有独立入口；个人片库负责追更、收藏、点赞及预约列表。共用的内容流呈现位于 `features/series_feed.rs`。
窗口、顶栏与导航装配归 `shell/`，不由共享控件反向引用功能页。

## 业务边界

全局快捷键、系统通知、桌面窗口基础操作和翻译使用NSH原生API。
`boss_key`只保留注册事务与隐藏状态，`notifications`只保留应用owner及业务结果适配；
`window_modes/framework`执行窗口命令，`desktop`保留形态与隐身决策，平台层仅补充遮挡、
全局光标位置和macOS红绿灯显隐。移动方向控制与文件导出继续归各自宿主。
`i18n`装载共用中英文资源至NSH `Translations`，使用框架locale，保留语言偏好落盘及业务格式化。

2026-11-15 框架能力迁移通过Windows工作区构建、严格Clippy、NSH检查及格式检查。
独立数据目录实例确认实时语言切换和数字格式更新、真实流连续播放、小窗拖动、最大化还原、
退出小窗、全屏及Esc恢复。通知权限/展示/点击和老板键占用冲突尚未系统实测；
macOS、Android和iOS未在本轮构建或运行，Windows宿主的移动crate空壳不计作移动验证。

NSH 不直接依赖 `hongguo-api`，不构造 `ApiEnv`。页面只调用 service，service 从应用状态生成请求环境。
`hongguo-service::contracts` 仅公开 UI 消费的数据类型，不公开底层请求函数。

- `catalog_service`：推荐、找剧、搜索、榜单、上新、详情、作者与弹幕读取。
- `library_service`：片库分页、元数据补齐、批量删除和失败汇总。
- `reservation_service`：预约成员判定、列表与预约操作。
- `emoticon_service`：表情列表、鉴权、收藏及图片上传准备。
- `comments_service`：评论/剧评分页、发表、回复、点赞与删除校验。
- `interaction_service`：点赞、收藏和追更状态聚合及写操作。
- `personal_service`：与账号身份绑定的观看历史、书架快照及续播提示缓存。

UI 保留 signal、owner、来源代次、焦点、滚动、乐观反馈和错误文案。service 不依赖 NSH，后台工作通过 service runtime 调度。
移动端仍使用独立宿主；平台视频适配和共享 Apple 实现保持独立 crate。

## 编码与增强

ByteVC2 编码识别常量及函数归 `hongguo-media-types::codec`；API 的选流判断不引入解码实现。
`hongguo-bytevc2` 保留完整解码能力，并复用这些识别定义；VVdeC 与 oxideav 配置保持原样。
720p 等规格实际下发的 ByteVC2 流继续经 `hongguo-playback → hongguo-bytevc2` 解码。

工作区共 29 个 crate。平台归档编码契约统一位于 `hongguo-media-types::encoding`，包含请求、I420 帧与编码输出。
HEVC 参数集与 Annex-B 解析位于 `hongguo-video::hevc`，播放层直接消费容器能力，不依赖归档媒体层。

`hongguo-video-gpu::enhancement::Settings` 是 GPU 增强管线的参数类型；增强 shader 和授权文件位于该 crate 的 `shaders/enhancement/`。
`hongguo-video-color` 保持独立，供 WGSL 与平台 HLSL 共同使用。
NSH 的序列化 `Enhancement` 契约保持原状，在各平台适配边界转换；增强层不依赖 NSH 视频契约。

## 清理与验证边界

生产代码不再包含 `offline_qa`、`HONGGUO_OFFLINE_QA`、测试夹具或 store 的 QA 转发接口。
`hongguo-common::file_io` 保留同步落盘和拒绝符号链接的正式 IO 能力。
`HONGGUO_DATA_DIR` 继续作为显式数据目录覆盖，不再附带离线网络开关。

默认开发使用认证原生 SDK，不恢复测试、离线预览或探针专用入口。
验证命令见 [NSH 开发说明](../.agents/skills/hongguo-dev/references/nsh.md)。
构建、静态检查、真实目标编译、运行和播放验收分别记录，历史截图与测试记录不计作本轮验证。
历史方案中的旧壳、旧接口和旧路径保留其研究语境，以本文和当前代码为准。

2026-11-15 本轮已通过 Windows x64 工作区完整构建、严格 Clippy、NSH 检查及 Rust/Prettier 格式检查。
此前界面与服务重构阶段，独立数据目录中的新版窗口已创建并正常响应；此记录不代表本次 crate 合并的播放验收。
实际播放、账号操作及 macOS/Android/iOS 完整应用与真机尚未验收。

本次 crate 合并另已通过 Windows x64 全工作区构建与严格 Clippy、NSH 检查；macOS ARM64/x64 归档平台链及 ARM64 GPU/Apple 适配/播放层通过交叉严格 Clippy。
16 个增强 shader、模型及授权文件哈希保持一致，资源生成脚本与打包授权收集路径均已验证。Apple 适配保留最小 `objc2-io-surface` feature 依赖，为 CoreVideo 的 IOSurface 返回类型启用 CoreFoundation 类型契约。

## English

Global shortcuts, notifications, desktop window operations and translations use NSH APIs.
Application modules retain registration transactions, owner binding, window-mode decisions and business formatting.
Native supplements cover occlusion, the global cursor and macOS traffic lights; mobile hosts retain orientation and file export.
The framework migration passed the Windows workspace build, strict Clippy, NSH and formatting checks.
An isolated Windows instance confirmed live locale/number updates, continued playback, mini-window dragging,
maximize/restore, exit from mini mode, and fullscreen/Esc restoration. Notification delivery, shortcut conflicts,
macOS and mobile builds/runs remain unverified in this migration.

The NSH root assembles services and the application shell. Each feature owns its page state, interactions and presentation; `ui/` contains shared controls only.
History, downloads and personal libraries have separate feature entries. Window and navigation composition lives in `shell/`.

NSH consumes services and typed service contracts, without a direct dependency on `hongguo-api` or request-environment construction.
Services own network operations, authentication and business orchestration. Reactive ownership, focus, scrolling and optimistic feedback remain in the UI.

ByteVC2 recognition lives in `hongguo-media-types`; decoding remains in `hongguo-bytevc2` with VVdeC and oxideav unchanged.
The workspace contains 29 crates. Encoding contracts live in `hongguo-media-types::encoding`, and HEVC container helpers live in `hongguo-video::hevc`.
Playback consumes container helpers directly without depending on the archival media layer.
The enhancement pipeline lives in `hongguo-video-gpu::enhancement`; platform adapters convert the unchanged NSH wire contract.
Shared color conversion remains in `hongguo-video-color`; enhancement shaders and licenses are preserved under `hongguo-video-gpu/shaders/enhancement/`.

Offline QA switches and fixtures have been removed. Production file IO keeps synchronization and symbolic-link protections.
An explicit `HONGGUO_DATA_DIR` remains supported. Workspace builds and strict static checks do not replace platform playback and visual acceptance.
The UI/service refactor passed the Windows x64 workspace build, strict Clippy, NSH checks and formatting checks, and opened an isolated application window successfully. That window check does not constitute playback acceptance for the subsequent crate merge. Playback, account operations and complete applications on other platforms remain unverified.
The crate merge also passed the Windows x64 workspace build, strict Clippy and NSH checks, plus cross-target strict Clippy for both macOS archival platform chains and the ARM64 GPU, Apple adapter and playback crates. All 16 enhancement asset hashes are unchanged; asset generation and notice collection use the new paths. The Apple adapter retains the minimal IOSurface feature dependency required by CoreVideo's CoreFoundation return-type contract.
