# 红果播放器 · Hongguo Desktop

基于 NSH 0.5.0 与 Rust 的非官方红果短剧客户端，桌面目标为 Windows 10 1809+ / Windows 11、macOS，另有独立 Android / iOS 宿主。

[English](README.en.md) · [使用声明](NOTICE) · [第三方组件](THIRD-PARTY-NOTICES.md)

仅供非商业学习研究，与官方无关联，不存储或分发影视内容。使用前请阅读 [NOTICE](NOTICE) 与 [LICENSE](LICENSE)。

## 开发与打包

安装 Rust 与对应平台 SDK、Node.js、pnpm 后，在仓库根执行：

```sh
pnpm install --frozen-lockfile
pnpm nsh
pnpm nsh:check
pnpm test
make nsh-package TARGET=macos
```

Windows 使用 `TARGET=windows`，安装器构建另需 NSIS 3；移动宿主使用 `android`、`ios`、`ios-simulator`。
普通开发自动校验、解包仓库随附的原生编解码 SDK，无需自行构建 FFmpeg、VVdeC 或 libde265。
客户安装包内置所需解码库，不安装 WebView2 或 HEVC 商店插件。

播放以 NSH Video 为基础，缺失编码由 libde265 / VVdeC 补齐，原生播放器提供音轨与时钟。
Windows 扩展 HEVC 优先 D3D11 VLD，Main10 使用 P010；实际硬解不可用才回退软解，不使用 FFmpeg。
平台实际验证范围见 [解码验证记录](docs/nsh-decoder-validation.md)。

共用图标、表情和语言资源位于 `assets/`；NSH 界面与装配在 `crates/hongguo-nsh`。
原生 UI 预览的 TypeScript 声明由匹配版本 NSH CLI 生成，详见 [NSH 开发说明](.agents/skills/hongguo-dev/references/nsh.md)。

Tauri、GPUI 壳、专用播放器与 WebView 前端已从主线移除，完整旧版保存在
[`codex/backup-tauri-final`](https://github.com/it00021hot/hongguo-desktop/tree/codex/backup-tauri-final)。
