# 番果 NSH 壳 / guava NSH shell

基于 NSH 0.7.0 的纯 Rust 客户端，通过共享 service 消费业务能力，不直接依赖 API 客户端。
功能入口与呈现归 `src/features/`，共享控件归 `src/ui/`，窗口与导航装配归 `src/shell/`。
结构与边界见 [项目结构](../../docs/project-structure.md)。
首轮目标为 Windows、macOS、Android、iOS；Linux 与 Web 留待后续迭代。
当前仍在开发和验证中，功能面以 `docs/nsh-feature-parity.md` 的进度记录为准。

播放以 NSH Video 为基础，专用 libde265 / VVdeC 补齐原生缺失能力；不依赖 FFmpeg。
原生播放器承担 AAC、音频输出、倍速和主时钟。客户无需另装解码包或运行库。
Windows 普通 HEVC 走 MediaEngine；ByteVC2、缺 HEVC 解码器或输出精度不足时启用内部帧适配。
在仓库根执行 `nsh run crates/hongguo-nsh`，通过原生宿主接入自动校验 `vendor/native-codecs/` 的 SDK，日常开发无需 CMake/libclang。

A pure Rust shell using NSH 0.7.0 and the existing business core. Initial targets
are Windows, macOS, Android and iOS. Feature presentation, shared controls and shell composition have separate owners; API requests are service-owned. Linux and Web are deferred. Development and
platform validation are ongoing; see docs/nsh-feature-parity.md for the progress record.

Playback uses NSH Video and bundled libde265/VVdeC for missing native capabilities.
Native players own audio, playback rate and timing. Customers install no codec packs or C/C++ runtime.
The repository includes authenticated codec binaries; normal development needs no CMake or libclang.

从仓库根运行 / Run from the repository root:

```sh
make nsh
make nsh-check
make nsh-package TARGET=macos
make nsh-package TARGET=windows
make nsh-package TARGET=android
make nsh-package TARGET=ios-simulator
```

Windows 打包由 NSH 生成便携 ZIP；有 Inno Setup 时生成 `guava-setup.exe`，不安装 WebView2。
NSH produces the Windows portable ZIP and an Inno Setup installer when its compiler is available.

应用名称为“番果 / guava”，应用 ID 为 `com.guava.app`。业务数据根为系统应用数据目录下的 `guava`，数据库为 `guava.db`；框架自己的存储和图片缓存按新应用 ID 隔离。显式数据根使用 `GUAVA_DATA_DIR`。不自动迁移或删除旧数据。

The app is named 番果 / guava, with identifier `com.guava.app`. Business data uses the system application-data directory's `guava` folder and database `guava.db`; framework storage and image caches use the new application identifier. `GUAVA_DATA_DIR` explicitly overrides the data root. Old data is not automatically migrated or deleted.

移动端使用独立的纯 Rust 宿主：`hongguo-nsh-android` 与 `hongguo-nsh-ios`。
不要用 NSH 自动生成的通用移动端宿主替代，否则会遗漏私有数据目录初始化。

Mobile builds use dedicated pure Rust hosts, which configure private storage
before mounting the shared root. NSH's generic generated hosts omit that setup.

仓库自有测试、离线预览与探针示例已移除。验证使用 `cargo build --workspace` 和严格 Clippy；
实际播放、视觉与移动端交互需在独立数据目录中验收。

Repository-owned tests, offline previews and probe examples have been removed.
Use workspace builds and strict Clippy; validate playback, visuals and mobile interactions
with an isolated data directory.

开发规范、环境要求与已验证边界见
[NSH 开发说明](../../.agents/skills/hongguo-dev/references/nsh.md)，
设计令牌与动效清单见 [DESIGN.md](DESIGN.md)，实施方案见
[nsh-shell-plan.md](../../docs/nsh-shell-plan.md)，解码策略与验收边界见
[nsh-decoder-plan.md](../../docs/nsh-decoder-plan.md)。
