# hongguo-desktop

当前客户端使用 NSH 0.7.1 与纯 Rust 业务内核，支持 Windows/macOS，另有独立 Android/iOS 宿主。
Tauri、GPUI 与 React/WebView 专用代码已移除；历史备份为 `codex/backup-tauri-final`，不得根据历史文档恢复旧壳。

## 开发前

先读用户级 `dev-habits`，再读 `.agents/skills/hongguo-dev/SKILL.md`；NSH 开发另读 `nsh`、`nsh-design-guides` 与 `crates/hongguo-nsh/DESIGN.md`。
现行接入与验证要求在 `.agents/skills/hongguo-dev/references/nsh.md`。

## 结构

- `crates/hongguo-nsh`：共享界面、服务装配、媒体 HTTP 供数；只消费共享业务服务。
- NSH 的 `features/<功能>/presentation` 保存功能专用呈现，`ui/` 保存共享控件，`shell/` 保存窗口与导航装配；历史、下载与个人片库独立。
- `hongguo-service` 公开业务操作与类型契约；NSH 不直接依赖 `hongguo-api` 或构造请求环境。详细边界见 `docs/project-structure.md`。
- `hongguo-nsh-android` / `hongguo-nsh-ios`：独立移动宿主。
- `hongguo-nsh-video-*`、`hongguo-playback`、`hongguo-native-clock`：播放、原生呈现与音轨时钟。
- `hongguo-media-*`、`hongguo-hevc`、`hongguo-bytevc2`、`hongguo-vvdec-sys`：媒体、GPU 与专用解码核。
- 编码识别归 `hongguo-media-types::codec`，归档编码契约归 `hongguo-media-types::encoding`；ByteVC2 解码及 VVdeC/oxideav 保留。增强归 `hongguo-video-gpu::enhancement`，消费自身 `Settings`，不依赖 NSH 视频契约；HEVC 容器解析归 `hongguo-video::hevc`。
- `hongguo-service`、`hongguo-api`、`hongguo-store`、`hongguo-common`、`hongguo-signer`：业务、接口、持久化与签名。
- `hongguo-video`、`hongguo-stream`：容器、解密与渐进数据；媒体来源 URL 形态在 `hongguo-common::media_url`。
- `assets/icons`、`assets/emoji`、`assets/locales`：共用资源。语言文案同步中英文。
- `vendor/native-codecs`：认证 SDK；默认开发不做原生解码库源码构建。
- `vendor/nsh` 与 `vendor/wgpu-hal`：固定版本源码与必要补丁，不参与项目格式化。

## 约束

- 框架已有能力优先使用 NSH 原生接口；全局快捷键、通知、窗口基础操作与翻译不维护平行实现。平台补充只覆盖框架缺口。
- 普通视频使用 NSH Video，专用 libde265 / VVdeC 补齐编码缺口；不使用 FFmpeg。
- Windows 仅支持普通 Windows 10 1809+ / Windows 11，不支持 Windows N。扩展 HEVC 优先 D3D11 VLD，Main10 保留 P010；实际硬解失败才回退软件。
- 保留原流、分辨率、位深、色彩/HDR、crop、SAR、旋转、时间轴；GPU 完成前保留原生帧租约。
- 原生播放器承担音轨与时钟；失败回退保留位置与代次隔离。禁止恢复播放转码、`.play.mp4`、grow/HLS 链。
- service 不依赖 UI，不裸用 `tokio::spawn` / `Handle::current` / `block_on`；后台任务经 service runtime。锁不跨 await。
- 签名代码按官方抓包契约移植，不改常量或算法写法；签名后不修改 URL、body、query。接口参数查 `docs/hongguo-api-endpoints.md`，禁止猜测。
- 在线播放与下载保持既有选流规则；同规格无损合并，异规格封存普通 HEVC MP4 后拼接。
- Cargo 在仓库根执行；release 不设 `panic = "abort"`；严格 Clippy 禁止 `#[allow]`，必要时使用具名理由的 `#[expect]`。
- 代码只通过精确编辑修改，不用 Python 或脚本盲改。用户数据与活跃实例不用于自动化。
- 开发在 dev，Conventional Commits + 中文描述；无明确授权不提交推送。结构变更与行为变更分开提交。

## 命令

使用仓库固定 NSH CLI：`nsh run crates/hongguo-nsh` 启动，`nsh check crates/hongguo-nsh` 检查。
`nsh package crates/hongguo-nsh --target macos|windows|android|ios|ios-simulator` 打包；Android 限定 `--abi arm64-v8a`。
应用元数据与平台打包配置统一在 `crates/hongguo-nsh/nsh.json`；`nativeHost` 补丁仅接入框架尚未覆盖的独立宿主与媒体分发材料，通过 `tools/build` 的 Rust 工具执行，无需 Node.js。仓库根使用 `cargo build-tool <命令>` 准备 SDK、维护原生库与校验发版。使用前按 NSH 开发说明构建固定 CLI。
格式检查使用 `cargo fmt -- --check`；`cargo build-tool enhancement-assets` 校验着色器源码、模型图及哈希，`--patch` 仅输出更新哈希的精确补丁。项目自身不依赖 Node.js/npm。
`make lint` 与提交钩子检查当前宿主，`make lint-macos` 为可选交叉检查；工具通过 PATH 或显式环境变量定位，禁止写死个人安装目录或覆盖开发者已有的工具链配置。
`cargo build --workspace` 与
`cargo clippy --workspace --all-targets -- -D warnings -D clippy::allow_attributes` 验证 Rust；使用当前宿主 SDK 的动态库搜索路径。
仓库自有测试、离线预览、探针示例及测试专用接口已移除；第三方 vendor 源码保持原样。
历史 Tauri/GPUI 文档只作研究背景；现行流程以 Makefile 与 NSH 开发说明为准。
