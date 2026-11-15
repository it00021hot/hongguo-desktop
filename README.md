# 番果 · guava

番果（guava）是基于 NSH 0.5.1 与 Rust 的非官方红果短剧客户端，桌面目标为 Windows 10 1809+ / Windows 11、macOS，另有独立 Android / iOS 宿主。

[English](README.en.md) · [使用声明](NOTICE) · [第三方组件](THIRD-PARTY-NOTICES.md)

仅供非商业学习研究，与官方无关联，不存储或分发影视内容。使用前请阅读 [NOTICE](NOTICE) 与 [LICENSE](LICENSE)。

## 开发与打包

安装 Rust、对应平台 SDK 与 Node.js 后，在仓库根构建本项目固定的 NSH CLI（包含必要的原生宿主接入补丁），并将 `target/nsh-cli/debug` 加入当前终端 PATH：

```sh
cargo build --manifest-path vendor/nsh/Cargo.toml -p nsh-cli --target-dir target/nsh-cli
nsh run crates/hongguo-nsh
nsh check crates/hongguo-nsh
make lint
nsh package crates/hongguo-nsh --target macos
```

Windows 使用 `--target windows`，NSH 检测到 Inno Setup 时生成安装器；移动目标使用 `android`、`ios`、`ios-simulator`。应用元数据与平台打包配置统一位于 `crates/hongguo-nsh/nsh.json`。
`tools/package.json` 仅管理格式与着色器维护工具，运行客户端不需要安装这些 npm 依赖；格式检查先执行 `npm ci --prefix tools --ignore-scripts`。
普通开发自动校验、解包仓库随附的原生编解码 SDK，无需自行构建 FFmpeg、VVdeC 或 libde265。
客户安装包内置所需解码库，不安装 WebView2 或 HEVC 商店插件。

播放以 NSH Video 为基础，缺失编码由 libde265 / VVdeC 补齐，原生播放器提供音轨与时钟。
Windows 扩展 HEVC 优先 D3D11 VLD，Main10 使用 P010；实际硬解不可用才回退软解，不使用 FFmpeg。
平台实际验证范围见 [解码验证记录](docs/nsh-decoder-validation.md)。

应用标识为 `com.guava.app`。Windows 数据目录为 `%APPDATA%\guava`，macOS 为 `~/Library/Application Support/guava`，数据库为 `guava.db`；`GUAVA_DATA_DIR` 可显式覆盖数据根。下载位于所选下载根下的 `guava/<剧名>`。移动端使用新应用 ID 的私有容器。旧版数据保留在原处，不自动迁移或删除。

共用图标、表情和语言资源位于 `assets/`；NSH 界面与装配在 `crates/hongguo-nsh`。
功能页面与专用呈现归 `src/features/`，共享控件归 `src/ui/`，窗口与导航装配归 `src/shell/`。
业务请求通过 `hongguo-service`，NSH 不直接依赖 API 客户端；ByteVC2 解码能力保持不变。
工作区现为 29 个 crate；编码契约归媒体类型层，GPU 增强归视频 GPU 层，HEVC 解析归容器层。
全局快捷键、系统通知、桌面窗口基础操作与翻译优先使用 NSH 原生接口；平台补充仅保留框架缺失的能力。
结构边界见 [项目结构](docs/project-structure.md)，命令及环境见 [NSH 开发说明](.agents/skills/hongguo-dev/references/nsh.md)。

Tauri、GPUI 壳及其旧播放器、WebView 前端已从主线移除，完整旧版保存在
[`codex/backup-tauri-final`](https://github.com/it00021hot/hongguo-desktop/tree/codex/backup-tauri-final)。
