//! 自定义 URI 协议：视频流、本地文件与封面转码供给。
//!
//! - [`local`]：`hongguo-local://` 本地成品文件（支持 Range 拖动）
//! - [`stream`]：`hongguo-stream://` 在线播放的内存渐进流
//! - [`cover`]：`hongguo-cover://` HEIC 封面下载转 JPEG（落盘缓存）

pub mod cover;
pub mod local;
pub mod range;
pub mod register;
pub mod stream;

/// 本地文件协议 scheme。
pub const LOCAL_SCHEME: &str = "hongguo-local";
/// 在线流协议 scheme。
pub const STREAM_SCHEME: &str = "hongguo-stream";
/// 封面转码协议 scheme。
pub const COVER_SCHEME: &str = "hongguo-cover";

/// 自定义协议对外的 URL 前缀（按平台选择形态）。
///
/// **Windows**：WebView2 不支持页面内容直接请求非标准 scheme，wry 靠把
/// `http://{scheme}.…` 开头的请求拦下来再转回自定义协议兜住（`custom_protocol_workaround`），
/// 必须给 `http://{scheme}.localhost/…`；直接给 `hongguo-stream://…` 不会触发
/// 协议处理器，表现就是「后端地址取到了、但永远黑屏、日志里一次请求都没有」。
///
/// **macOS / Linux**：WKWebView（WKURLSchemeHandler）与 webkit2gtk 拦的是
/// **真 scheme**，`http://hongguo-stream.localhost/…` 会被当成真实 HTTP 请求
/// 发往不存在的主机 → `<video>` 直接 MediaError 4（2026-10-06 macOS 全部
/// 视频打不开的根因）。必须给 `{scheme}://localhost/…`（`tauri://localhost`
/// 同款形态，wry 转给 handler 时 host 恒为 localhost）。
pub fn scheme_base(scheme: &str) -> String {
    if cfg!(target_os = "windows") {
        format!("http://{scheme}.localhost")
    } else {
        format!("{scheme}://localhost")
    }
}

/// 在线流 URL。
///
/// 形态按平台走 [`scheme_base`]（Windows = `http://{scheme}.localhost`，
/// macOS/Linux = `{scheme}://localhost`），细节见其文档。
///
/// 档位必须在路径里，有两个作用：`<video>` 靠 URL 变化感知「换了资源」并重新加载；
/// 它同时是缓存的键（见 [`stream`]），所以同一集的不同档位各占一份数据。
pub fn stream_url(vid: &str, definition: u32) -> String {
    format!("{}/s/{definition}/{vid}", scheme_base(STREAM_SCHEME))
}

/// 从在线流 URL 的 path 里取回 `(档位, vid)`。
///
/// 解析不了返回 `None`：path 一定由 [`stream_url`] 生成，对不上就是有别处在乱拼
/// URL，宁可明确报错也不要猜。
pub fn parse_stream_path(path: &str) -> Option<(u32, &str)> {
    let (definition, vid) = path
        .trim_start_matches('/')
        .strip_prefix("s/")?
        .split_once('/')?;
    Some((definition.parse().ok()?, vid))
}

/// 本地成品 URL。`encoded` 是文件路径的 base64url 编码。
///
/// 形态按平台走 [`scheme_base`]，原因见其文档。
pub fn local_url(encoded: &str) -> String {
    format!("{}/f/{encoded}", scheme_base(LOCAL_SCHEME))
}
