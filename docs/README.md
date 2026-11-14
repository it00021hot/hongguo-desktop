# 文档索引

以代码为唯一真相。Agent 开发规范在 `.agents/skills/hongguo-dev/`（先读用户级
`dev-habits`，再读项目 skill）；仓库地图见根目录 [`AGENTS.md`](../AGENTS.md)。

## 用户 / 产品

| 文档 | 用途 |
| --- | --- |
| [faq.md](./faq.md) | Windows HEVC / WebView2 / 杀软等排查（README 链出） |
| [app-naming.md](./app-naming.md) | 应用显示名三层（包名 / macOS InfoPlist / Windows NSIS） |

## 开发 / 对接

| 文档 | 用途 |
| --- | --- |
| [hongguo-api-endpoints.md](./hongguo-api-endpoints.md) | 官方接口抓包台账（路径 / 签名 / 响应 / 坑）——改端点前必读 |
| [hongguo-apk-endpoints.md](./hongguo-apk-endpoints.md) | APK 静态路径清单（「在用」列标本仓已接端点） |
| [req-hg-003-playback-parity.md](./req-hg-003-playback-parity.md) | 播放对齐调查交接、Craft 证据与运行时验收状态（进行中） |
| [issue-22-tauri-investigation.md](./issue-22-tauri-investigation.md) | Tauri 播放失败、历史越界、末集接力与迷你窗口恢复的排查、修复及验证记录 |
| [webview-cdp-testing.md](./webview-cdp-testing.md) | Tauri WebView CDP / macOS WebDriver 调试 |
| [nsh-shell-plan.md](./nsh-shell-plan.md) | NSH 0.5.0 四端壳实施方案（实施中，Linux/Web 后续迭代） |
| [nsh-decoder-plan.md](./nsh-decoder-plan.md) | NSH 0.5.0 已批准的完整播放方案与发布门槛 |
| [nsh-decoder-validation.md](./nsh-decoder-validation.md) | 新解码、PCM、平台呈现及正式包的实际验证结果和待测边界 |
| [native-media-build.md](./native-media-build.md) | 固定 FFmpeg 来源、跨端构建、包依赖闭包与静态重链接材料 |
| [nsh-feature-parity.md](./nsh-feature-parity.md) | NSH 与 Tauri 功能对齐、平台验收进度与缺口 |

## 实现说明（已落地）

| 文档 | 用途 |
| --- | --- |
| [gpui-system-player-windows.md](./gpui-system-player-windows.md) | Windows GPUI 系统播放器：MediaEngine frame-server + 视频 quad |
| [mp4-muxer-plan.md](./mp4-muxer-plan.md) | 自写 MP4 封装替换 muxide 的设计记录（已落地） |
