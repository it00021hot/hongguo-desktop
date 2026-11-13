# 红果 NSH 壳 / Hongguo NSH shell

基于 NSH 0.4.0 的纯 Rust 第三壳，复用现有业务内核，直接调用红果 API。
首轮目标为 Windows、macOS、Android、iOS；Linux 与 Web 留待后续迭代。
当前仍在开发和验证中，尚未与 Tauri 功能完全对齐。

A pure Rust shell using NSH 0.4.0 and the existing business core. Initial targets
are Windows, macOS, Android and iOS. Linux and Web are deferred. Development and
platform validation are ongoing; feature parity with Tauri is incomplete.

从仓库根运行 / Run from the repository root:

```sh
make nsh
make nsh-check
make nsh-test
make nsh-preview
make nsh-package TARGET=macos
make nsh-package TARGET=android
make nsh-package TARGET=ios-simulator
```

移动端使用独立的纯 Rust 宿主：`hongguo-nsh-android` 与 `hongguo-nsh-ios`。
不要用 NSH 自动生成的通用移动端宿主替代，否则会遗漏私有数据目录初始化。

Mobile builds use dedicated pure Rust hosts, which configure private storage
before mounting the shared root. NSH's generic generated hosts omit that setup.

`make nsh-preview` 只打开独立的离线页面预览，复用生产展示组件，不初始化数据库、
访问官方接口或播放视频。主对照尺寸为 Tauri 默认的 1280×840。
预览使用 `target/nsh-layout-fixtures/player-frame.png` 的本地测试静帧，截图保存在
`target/nsh-preview/`；测试画面不随源码分发。字体诊断入口须使用独立构建缓存。

The layout preview uses local fixtures and the production presentation components,
with no database, API calls or media playback. A local test frame is required;
screenshots are written to `target/nsh-preview/`.

开发规范、环境要求与已验证边界见
[NSH 开发说明](../../.agents/skills/hongguo-dev/references/nsh.md)，
设计令牌与动效清单见 [DESIGN.md](DESIGN.md)，实施方案见
[nsh-shell-plan.md](../../docs/nsh-shell-plan.md)。
