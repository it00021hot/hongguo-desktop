# 移除旧壳 crate

用户已授权在备份分支 `codex/backup-tauri-final` 建立后删除 Tauri、GPUI 壳 crate。

1. 将 NSH 共用图标迁至 `assets/icons`，更新桌面、Android、iOS 构建引用。
2. 删除 `hongguo-tauri`、`hongguo-tauri-app`、`hongguo-gpui`，移除 workspace 成员及失效的旧壳构建入口。
3. 首轮保留共享业务、媒体、协议、播放器库和前端资源，后续按用户授权反查独占代码。
4. 更新 Cargo.lock，运行 workspace 检查、测试、脚本测试及 Windows 目标检查。

历史 Tauri 发布与 Android 工作流随旧壳移除；NSH 使用现有 `nsh-package` 本地打包入口。

后续清理（用户另行授权）：删除四个旧 `hongguo-player*` crate、GPUI vendor 补丁、
React/WebView 前端及专用工具。NSH 使用的语言包、表情和目录表迁至 assets。
`hongguo-softdec` 仍被 NSH 的 pixel_surface 实播夹具调用，保留可达的解码与合成样本；
`hongguo-protocol` 仍承担 service 与 NSH 的媒体标识编解码，保留 URL/路径契约，删除旧壳供数。
已移除旧音轨遮蔽器、NV12/WebView 色彩适配、旧协议线程池、封面代理与 Range 应答；
NSH 使用的渐进缓存、明文字节读取与 Range 解析保留，回归验证解密、取消、代次与 FIFO。
Anime4K npm 包仍供 NSH 着色器离线提取使用，保留为开发依赖，不将其误判为旧 Web 前端依赖。
