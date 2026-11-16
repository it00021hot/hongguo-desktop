# 移除旧壳 crate

用户已授权在备份分支 `codex/backup-tauri-final` 建立后删除 Tauri、GPUI 壳 crate。

1. 将 NSH 共用图标迁至 `assets/icons`，更新桌面、Android、iOS 构建引用。
2. 删除 `hongguo-tauri`、`hongguo-tauri-app`、`hongguo-gpui`，移除 workspace 成员及失效的旧壳构建入口。
3. 首轮保留共享业务、媒体、协议、播放器库和前端资源，后续按用户授权反查独占代码。
4. 更新 Cargo.lock，运行 workspace 检查、测试、脚本测试及 Windows 目标检查。

历史 Tauri 发布与 Android 工作流随旧壳移除；NSH 使用现有 `nsh-package` 本地打包入口。

后续清理（已完成，d59aeb18）：删除四个旧 `hongguo-player*` crate、GPUI vendor 补丁、
React/WebView 前端及专用工具。NSH 使用的语言包、表情和目录表迁至 assets。
已移除旧音轨遮蔽器、NV12/WebView 色彩适配、旧协议线程池、封面代理与 Range 应答；
NSH 使用的渐进缓存、明文字节读取与 Range 解析保留，回归验证解密、取消、代次与 FIFO。
当时保留的 Anime4K 开发依赖已在后续去除 Node.js 的迁移中移除；现以入库 WGSL、模型图及许可证为维护来源，由 `cargo build-tool enhancement-assets` 校验。

二轮清理（2026-11-15）：删除 `hongguo-softdec`（索引/软解会话与 playback 同构，唯一
消费者 pixel_surface 夹具改接 `hongguo-playback`，合成样本迁 `hongguo-media/tests/fixtures`）、
`hongguo-protocol`（138 行 URL 形态并入 `hongguo-common::media_url`）与
`hongguo-media-platform-macos/ios` 纯转发壳（门面按目标直连 videotoolbox）。
