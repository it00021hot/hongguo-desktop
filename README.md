# 番果 · guava

番果是适用于 **Windows 和 macOS** 的非官方红果短剧客户端。可以刷推荐、按条件找剧、从上次进度续播，也可以选择分集下载到本地。

**[下载最新版](https://github.com/it00021hot/hongguo-desktop/releases/latest)** · [安装](#下载与安装) · [快速上手](#快速上手) · [更新日志](CHANGELOG.md) · [English](README.en.md)

## 下载与安装

在 [最新版发布页](https://github.com/it00021hot/hongguo-desktop/releases/latest) 的 **Assets** 中，按文件名后缀选择安装包。

- **Windows x64**（Windows 10 1809+ 或 Windows 11，Windows N 除外）。运行 `windows-x86_64-setup.exe` 中文安装器，或完整解压 `windows-x86_64-portable.zip` 后打开 `guava.exe`。
- **Mac · Apple 芯片（M 系列）**（macOS 14+）。选择 `macos-aarch64.dmg`。
- **Mac · Intel 芯片**（macOS 14+）。选择 `macos-x86_64.dmg`。

可在 macOS 的「关于本机」查看芯片类型。打开对应的 DMG 后，将番果拖入「应用程序」。

Windows 程序未做 Authenticode 签名。macOS 使用 ad hoc 签名，未经 Apple 公证。Android 和 iOS 宿主可从源码构建，GitHub Releases 提供桌面安装包。 [桌面发版说明](docs/desktop-release.md)

## 快速上手

### 刷推荐、找一部剧

打开「首页」浏览推荐、漫剧和真人剧。点击画面上的剧名进入详情，也可以在顶栏输入剧名或关键词。想按题材、时长和完结状态筛选时，打开「找剧」，那里也有排行榜和新剧。

### 选集、续播和调整画面

从「我的 → 历史」打开看过的剧，可以接着上次的进度看。播放时可选集、调倍速和清晰度，也可查看评论或发送带表情的弹幕。

| 默认快捷键 | 操作 |
| --- | --- |
| <kbd>空格</kbd> | 播放或暂停 |
| <kbd>←</kbd> / <kbd>→</kbd> | 快退或快进 5 秒 |
| <kbd>↑</kbd> / <kbd>↓</kbd> | 切剧或切集 |

可在「设置 → 播放快捷键」调整键位。

**窗口模式**包括全屏、小窗、竖屏、置顶和无边框。主窗口、小窗和竖屏窗口分别记住上次的位置与大小。

**画质增强**提供锐化和 Anime4K 2×。在支持的设备上，Anime4K 适用于 1080p 及以下动漫片源。增强只作用于播放画面。

### 下载分集、合并和清理

- 在播放页打开 **「下载」**，选中单集或输入集数范围，再到侧栏的「下载」查看任务。
- 在 **「一键合并」** 处理已下载的分集。同规格用「快速合并」无损直拼，混合规格可用「HEVC 合并」统一编码。
- 在 **「磁盘清理」** 按剧删除本地文件，保留剧集档案以便日后重新下载。

### 管理片库和桌面偏好

「我的」集中查看历史、收藏、点赞、追更和预约。登录后可同步收藏与追更。「设置」可配置下载位置和快捷键。桌面端支持中英文、浅色和深色主题、系统托盘、退出确认，以及用于快速隐藏窗口的老板键。

## 更新与数据

桌面版启动时会检查更新，也可在「设置」中手动检查。发现新版后，在「设置」选择「下载更新」。校验完成后，点击「立即更新」安装并重启，保留现有设置和登录状态。应用所在目录需可写。**0.7.0 及更早版本**请先手动安装一次最新版。

| 平台 | 默认数据目录 |
| --- | --- |
| Windows | `%APPDATA%\guava` |
| macOS | `~/Library/Application Support/guava` |

数据库文件为 `guava.db`。需要使用独立数据目录时，可设置 `GUAVA_DATA_DIR`。已下载视频位于所选下载目录下的 `guava/<剧名>`。

### 旧版数据

旧版数据库需手动迁移。以下操作会替换新版数据库，请先备份两边的数据。

<details>
<summary>展开迁移路径和步骤</summary>

| 平台 | 旧数据库默认路径 | 新数据库默认路径 |
| --- | --- | --- |
| Windows | `%APPDATA%\hongguo-downloader\hongguo.db` | `%APPDATA%\guava\guava.db` |
| macOS | `~/Library/Application Support/hongguo-downloader/hongguo.db` | `~/Library/Application Support/guava/guava.db` |

1. 完全退出新旧版本，包括托盘后台进程，分别备份两边的数据目录。
2. 如尚未创建新数据目录，先创建它。将其中已有的 `guava.db` 和旁路日志移到备份位置。
3. **复制**旧 `hongguo.db` 到新目录并改名为 `guava.db`。如果旧目录仍有 `hongguo.db-wal`、`hongguo.db-shm` 或 `hongguo.db-journal`，与主库一起复制，并改为对应的 `guava.db-wal`、`guava.db-shm` 或 `guava.db-journal`。
4. 启动新版，检查记录与设置，并保留原库和备份。

使用过自定义数据目录时，以实际路径为准。迁移仅复制数据库与旁路日志，已下载的视频和缓存仍在原路径。这些步骤适用于使用 `hongguo.db` 的旧桌面版。

</details>

## 开发与打包

桌面界面使用 NSH 0.6.1，业务内核使用 Rust。准备 Rust 1.99.1 和对应平台 SDK 后，在仓库根构建固定版本的 NSH CLI。

```sh
cargo build --locked --manifest-path vendor/nsh/Cargo.toml -p nsh-cli --target-dir target/nsh-cli
```

将 `target/nsh-cli/debug` 加入当前终端的 PATH，然后在仓库根运行。

```sh
nsh run crates/hongguo-nsh
nsh check crates/hongguo-nsh
```

macOS 本地开发也可用 `make dev`，它会配置 VVdeC 的等待策略。打包使用 `nsh package crates/hongguo-nsh --target <目标>`，目标可选 `macos`、`windows`、`android`、`ios` 或 `ios-simulator`。Windows 安装器需要 Inno Setup。

构建会校验并解包仓库随附的编解码 SDK。构建、打包和 `make lint` 无需 Node.js 或 npm。应用元数据和平台配置位于 [nsh.json](crates/hongguo-nsh/nsh.json)。

### 播放实现

普通视频通过 NSH Video 播放，专用解码器补齐 HEVC、ByteVC2 等播放场景。Windows 扩展 HEVC 优先使用 D3D11 硬解，硬解不可用时使用软件解码。在线播放使用渐进缓存，预取受服务端预算限制并优先保证当前播放。 [解码验证记录](docs/nsh-decoder-validation.md)

## 文档与反馈

| 文档 | 内容 |
| --- | --- |
| [NSH 开发说明](.agents/skills/hongguo-dev/references/nsh.md) | 工具链、各端打包与本地验证 |
| [项目结构](docs/project-structure.md) | 界面、业务服务、播放和解码的模块边界 |
| [桌面发版说明](docs/desktop-release.md) | Windows 和 macOS 的构建、签名与更新包 |
| [文档索引](docs/README.md) | 专题文档与历史方案 |

反馈问题时，请附应用版本、操作系统和复现步骤。播放问题可注明清晰度及是否启用了画质增强。 [提交 Issue](https://github.com/it00021hot/hongguo-desktop/issues)

## 使用声明

番果仅供个人非商业学习与研究，与红果短剧官方无关联。项目仓库不托管或分发影视内容。 [项目声明](NOTICE) · [许可证](LICENSE) · [第三方组件声明](THIRD-PARTY-NOTICES.md)

早期 Tauri 和 GPUI 版本的源码可查阅 [历史备份](https://github.com/it00021hot/hongguo-desktop/tree/codex/backup-tauri-final)。
