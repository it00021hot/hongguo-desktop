# 番果 · guava

番果（guava）是基于 NSH 0.5.1 与 Rust 的非官方红果短剧客户端，桌面目标为 Windows 10 1809+ / Windows 11、macOS，另有独立 Android / iOS 宿主。

[English](README.en.md) · [使用声明](NOTICE) · [第三方组件](THIRD-PARTY-NOTICES.md)

仅供非商业学习研究，与官方无关联，不存储或分发影视内容。使用前请阅读 [NOTICE](NOTICE) 与 [LICENSE](LICENSE)。

## 新版体验

`0.7.0` 桌面版使用 NSH 原生界面与 Rust 业务内核，支持 Windows 10 1809+ / Windows 11、macOS 14+，提供中文与英文界面、浅色与深色主题。

| 功能       | 使用方式                                                             |
| ---------- | -------------------------------------------------------------------- |
| 沉浸刷剧   | 推荐、漫剧、真人剧频道，切换剧集并按观看历史续播                     |
| 找剧与搜索 | 分类、排行榜、新剧与搜索联想；有片源的结果打开详情，纯关键词执行搜索 |
| 播放控制   | 选集、倍速、清晰度、全屏、小屏、竖屏、置顶与无边框窗口               |
| 弹幕与互动 | 弹幕及表情、评论、点赞、收藏、追更与预约                             |
| 本地管理   | 下载队列、个人片库、一键合并与磁盘清理                               |
| 桌面集成   | 托盘、老板键、快捷键与关闭确认                                       |

在线媒体采用渐进缓存，预取遵循服务端下发的时段、数量和字节预算，并优先保障当前播放。高峰时段可能暂停预取；缓存命中也不意味着所有片源都能立即出画面。

## 下载与安装

安装包见 [Releases](https://github.com/it00021hot/hongguo-desktop/releases)。Windows 提供中文安装器和便携 ZIP；macOS 按 Apple Silicon、Intel 分别提供 DMG 与压缩归档，DMG 内将应用拖入「Applications」即可安装。

当前版本为 `0.7.0`。Windows 暂无 Authenticode 签名，macOS 为 ad hoc 签名、未公证。Windows N 不在支持范围内。

## 开发与打包

安装 Rust 与对应平台 SDK 后，在仓库根构建本项目固定的 NSH CLI（包含必要的原生宿主接入补丁），并将 `target/nsh-cli/debug` 加入当前终端 PATH：

```sh
cargo build --manifest-path vendor/nsh/Cargo.toml -p nsh-cli --target-dir target/nsh-cli
nsh run crates/hongguo-nsh
nsh check crates/hongguo-nsh
nsh package crates/hongguo-nsh --target macos
```

Windows 使用 `--target windows`，NSH 检测到 Inno Setup 时生成安装器；移动目标使用 `android`、`ios`、`ios-simulator`。应用元数据与平台打包配置统一位于 `crates/hongguo-nsh/nsh.json`。
构建、打包、原生 SDK 维护与发版校验统一使用 `tools/build` 的 Rust 工具（`cargo build-tool <命令>`），无需 Node.js。
格式检查使用 `cargo fmt -- --check`，着色器维护使用 `cargo build-tool enhancement-assets`；`make lint`、提交钩子与 CI 均无需 Node.js/npm。
`make lint` 检查当前宿主；可选 `make lint-macos` 才需要额外的 macOS Rust target 与交叉工具链。工具通过 PATH 或环境变量定位，不绑定个人安装目录。
普通开发自动校验、解包仓库随附的原生编解码 SDK，无需自行构建 FFmpeg、VVdeC 或 libde265。
客户安装包内置所需解码库，不安装 WebView2 或 HEVC 商店插件。

播放以 NSH Video 为基础，缺失编码由 libde265 / VVdeC 补齐，原生播放器提供音轨与时钟。
Windows 扩展 HEVC 优先 D3D11 VLD，Main10 使用 P010；实际硬解不可用才回退软解，不使用 FFmpeg。
平台实际验证范围见 [解码验证记录](docs/nsh-decoder-validation.md)。

## 桌面发版准备

`0.7.0` 的 CI 使用固定 NSH CLI 构建 Windows x64、Apple Silicon 和 Intel Mac 产物，版本 tag 触发完整检查后创建草稿 Release，再按发布授权公开。Windows 暂不做 Authenticode 签名，Mac 为 ad hoc 签名、未公证；安装和播放仍需对应设备验收。版本、产物和操作步骤见 [桌面发版说明](docs/desktop-release.md)。

应用标识为 `com.guava.app`。Windows 数据目录为 `%APPDATA%\guava`，macOS 为 `~/Library/Application Support/guava`，数据库为 `guava.db`；`GUAVA_DATA_DIR` 可显式覆盖数据根。下载位于所选下载根下的 `guava/<剧名>`。移动端使用新应用 ID 的私有容器。旧版数据保留在原处，不自动迁移或删除。

## 旧版数据

**0.7.0 默认不迁移旧版本数据**，首次启动使用独立的新数据库。使用 `hongguo.db` 的旧桌面版可按以下路径手动复制：

| 平台 | 旧数据库默认路径 | 新数据库默认路径 |
| --- | --- | --- |
| Windows | `%APPDATA%\hongguo-downloader\hongguo.db` | `%APPDATA%\guava\guava.db` |
| macOS | `~/Library/Application Support/hongguo-downloader/hongguo.db` | `~/Library/Application Support/guava/guava.db` |

1. 完全退出新旧版本，包括托盘后台进程，分别备份两个数据目录。
2. 创建新数据目录（如尚不存在），将其中已有的 `guava.db` 及其旁路日志移入备份位置。
3. **复制**旧 `hongguo.db` 到新目录并改名为 `guava.db`。若旧目录退出后仍有 `hongguo.db-wal`、`hongguo.db-shm` 或 `hongguo.db-journal`，须与主库成套复制，并分别改为 `guava.db-wal`、`guava.db-shm`、`guava.db-journal`；不要混用新旧日志，也不要在程序运行时只复制主库。
4. 启动 0.7.0 并检查记录与设置。此操作替换新版本数据库，**不会合并**两边的数据；原库和备份请保留。

设置过自定义数据目录时，以实际目录为准，0.7.0 使用 `GUAVA_DATA_DIR`。复制数据库不会搬移已下载视频或缓存，原文件路径仍需保持可访问。本说明适用于桌面数据库，不适用于移动端私有容器或更早的 JSON 数据文件。

## 项目结构

共用图标、表情和语言资源位于 `assets/`；NSH 界面与装配在 `crates/hongguo-nsh`。
功能页面与专用呈现归 `src/features/`，共享控件归 `src/ui/`，窗口与导航装配归 `src/shell/`。
业务请求通过 `hongguo-service`，NSH 不直接依赖 API 客户端；ByteVC2 解码能力保持不变。
工作区现为 30 个 crate（含 `tools/build` 构建工具）；编码契约归媒体类型层，GPU 增强归视频 GPU 层，HEVC 解析归容器层。
全局快捷键、系统通知、桌面窗口基础操作与翻译优先使用 NSH 原生接口；平台补充仅保留框架缺失的能力。
结构边界见 [项目结构](docs/project-structure.md)，命令及环境见 [NSH 开发说明](.agents/skills/hongguo-dev/references/nsh.md)。

Tauri、GPUI 壳及其旧播放器、WebView 前端已从主线移除，完整旧版保存在
[`codex/backup-tauri-final`](https://github.com/it00021hot/hongguo-desktop/tree/codex/backup-tauri-final)。
旧版停止维护，不再提供功能更新和问题修复；当前开发与问题反馈以 NSH 新版为准。
