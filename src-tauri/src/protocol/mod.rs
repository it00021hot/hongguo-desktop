//! 自定义 URI 协议：视频流与本地文件供给。
//!
//! - [`local`]：`hongguo-local://` 本地成品文件（支持 Range 拖动）
//! - [`stream`]：`hongguo-stream://` 在线播放的内存渐进流

pub mod local;
pub mod range;
pub mod register;
pub mod stream;

/// 本地文件协议 scheme。
pub const LOCAL_SCHEME: &str = "hongguo-local";
/// 在线流协议 scheme。
pub const STREAM_SCHEME: &str = "hongguo-stream";

/// 在线流 URL。
///
/// ⚠️ 必须是 `http://{scheme}.localhost/…`，**不能**直接写 `hongguo-stream://…`。
/// Windows 的 WebView2 不支持页面内容直接请求非标准 scheme；wry 靠把
/// `http://{scheme}.…` 开头的请求拦下来再转回自定义协议来兜住这件事
/// （见 wry 的 `custom_protocol_workaround`）。直接给 `<video src="hongguo-stream://…">`
/// 不会触发协议处理器，表现就是「后端地址取到了、但永远黑屏、日志里一次请求都没有」。
pub fn stream_url(vid: &str) -> String {
    format!("http://{STREAM_SCHEME}.localhost/{vid}")
}

/// 本地成品 URL。`encoded` 是文件路径的 base64url 编码。
///
/// 同样必须走 `http://{scheme}.localhost` 形式，原因见 [`stream_url`]。
pub fn local_url(encoded: &str) -> String {
    format!("http://{LOCAL_SCHEME}.localhost/f/{encoded}")
}
