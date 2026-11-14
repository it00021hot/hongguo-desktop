# hongguo-desktop

当前客户端使用 NSH 0.5.0 与纯 Rust 业务内核，支持 Windows/macOS，另有独立 Android/iOS 宿主。
Tauri、GPUI 与 React/WebView 专用代码已移除；历史备份为 `codex/backup-tauri-final`，不得根据历史文档恢复旧壳。

## 开发前

先读用户级 `dev-habits`，再读 `.agents/skills/hongguo-dev/SKILL.md`；NSH 开发另读 `nsh`、`nsh-design-guides` 与项目 `DESIGN.md`。
现行接入与验证要求在 `.agents/skills/hongguo-dev/references/nsh.md`。

## 结构

- `crates/hongguo-nsh`：共享界面、服务装配、媒体 HTTP 供数；只消费共享业务服务。
- `hongguo-nsh-android` / `hongguo-nsh-ios`：独立移动宿主。
- `hongguo-nsh-video-*`、`hongguo-playback`、`hongguo-native-clock`：播放、原生呈现与音轨时钟。
- `hongguo-media-*`、`hongguo-hevc`、`hongguo-bytevc2`、`hongguo-vvdec-sys`：媒体、GPU 与专用解码核。
- `hongguo-service`、`hongguo-api`、`hongguo-store`、`hongguo-common`、`hongguo-signer`：业务、接口、持久化与签名。
- `hongguo-video`、`hongguo-stream`、`hongguo-protocol`：容器、解密、渐进数据与媒体来源标识。
- `hongguo-softdec`：仍供 NSH 实播夹具使用的解码器及合成样本，不用于正式播放调度。
- `assets/icons`、`assets/emoji`、`assets/locales`：共用资源。语言文案同步中英文。
- `vendor/native-codecs`：认证 SDK；默认开发不做原生解码库源码构建。
- `vendor/nsh` 与 `vendor/wgpu-hal`：固定版本源码与必要补丁，不参与项目格式化。

## 约束

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

`pnpm nsh` 启动，`pnpm nsh:check` 检查，`pnpm test` 验证包装脚本。
`make nsh-package TARGET=macos|windows|android|ios|ios-simulator` 打包。
`cargo clippy --workspace --all-targets -- -D warnings -D clippy::allow_attributes` 与
`cargo test --workspace --lib -- --test-threads=1` 验证 Rust；使用当前宿主 SDK 的动态库搜索路径。
原生 UI 预览及其类型声明需匹配 NSH CLI；未生成 `.nsh` 声明时不能把 TypeScript 检查报成通过。
历史 Tauri/GPUI 文档只作研究背景；现行流程以 Makefile 与 NSH 开发说明为准。
