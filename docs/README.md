# 文档索引

以代码为唯一真相。Agent 开发规范在 `.agents/skills/hongguo-dev/`（先读用户级
`dev-habits`，再读项目 skill）；仓库地图见根目录 [`AGENTS.md`](../AGENTS.md)。
旧 Tauri/GPUI 壳的代码与文档只在备份分支 `codex/backup-tauri-final`。

## 用户 / 产品

| 文档 | 用途 |
| --- | --- |
| [faq.md](./faq.md) | Windows HEVC / 杀软等排查（README 链出） |

## 开发 / 对接（现行）

| 文档 | 用途 |
| --- | --- |
| [hongguo-api-endpoints.md](./hongguo-api-endpoints.md) | 官方接口抓包台账（路径 / 签名 / 响应 / 坑）——改端点前必读 |
| [hongguo-apk-endpoints.md](./hongguo-apk-endpoints.md) | APK 静态路径清单（「在用」列标本仓已接端点） |
| [红果首页沉浸刷剧-接口文档.md](./红果首页沉浸刷剧-接口文档.md) | 首页信息流→连播→进度上报主链路的接口参考 |
| [native-media-build.md](./native-media-build.md) | 固定原生编解码来源、跨端构建、包依赖闭包与静态重链接材料 |
| [nsh-decoder-plan.md](./nsh-decoder-plan.md) | NSH 已批准的完整播放方案与发布门槛 |
| [nsh-decoder-validation.md](./nsh-decoder-validation.md) | 新解码、PCM、平台呈现及正式包的实际验证结果和待测边界 |
| [nsh-softdec-integration-plan.md](./nsh-softdec-integration-plan.md) | 内置解码契约（帧供给协议与四端呈现）；DirectSession 章节已随 softdec 删除过期 |
| [nsh-shell-plan.md](./nsh-shell-plan.md) | NSH 四端壳实施方案（实施中，Linux/Web 后续迭代） |
| [nsh-windows-cross-check.md](./nsh-windows-cross-check.md) | macOS 宿主上交叉检查 Windows MSVC 目标的手册 |
| [hevc-archive-migration.md](./hevc-archive-migration.md) | HEVC 归档/合并契约（hvc1/hvcC、ArchiveHevc、marker） |
| [nsh-feature-parity.md](./nsh-feature-parity.md) | NSH 功能验收进度与缺口（对齐基准 Tauri 壳已移至备份分支） |
| [req-hg-003-playback-parity.md](./req-hg-003-playback-parity.md) | 播放对齐调查交接、Craft 证据与运行时验收状态（进行中） |

## 实施与排查记录（历史）

这些记录的写作时点涉及已删除的旧壳，只作背景研究，不据此恢复旧路径。

| 文档 | 用途 |
| --- | --- |
| [nsh-menu-audit-20261113.md](./nsh-menu-audit-20261113.md) | 非播放器菜单 NSH vs Tauri 源码行级审查（2026-11-13） |
| [issue-22-tauri-investigation.md](./issue-22-tauri-investigation.md) | Tauri 播放失败、历史越界、末集接力的排查记录（修复已下沉共享 service） |
| [nsh-native-pixel-video-plan.md](./nsh-native-pixel-video-plan.md) | 已淘汰的像素视频早期方案 |
| [softdec-direct-play-plan.md](./softdec-direct-play-plan.md) | softdec 直播放的演进史（2026-11-11 起；crate 已于 2026-11-15 删除） |
| [builtin-playback-cleanup.md](./builtin-playback-cleanup.md) | 播放转码专属接口清理记录 |
| [remove-legacy-shell-crates.md](./remove-legacy-shell-crates.md) | 删除旧壳 crate 的执行记录（d59aeb18 完成） |
| [mp4-muxer-plan.md](./mp4-muxer-plan.md) | 自写 MP4 封装替换 muxide 的设计记录（已落地） |
