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
| [webview-cdp-testing.md](./webview-cdp-testing.md) | Tauri WebView CDP / macOS WebDriver 调试 |

## 实现说明（已落地）

| 文档 | 用途 |
| --- | --- |
| [gpui-system-player-windows.md](./gpui-system-player-windows.md) | Windows GPUI 系统播放器：MediaEngine frame-server + 视频 quad |
| [mp4-muxer-plan.md](./mp4-muxer-plan.md) | 自写 MP4 封装替换 muxide 的设计记录（已落地） |
