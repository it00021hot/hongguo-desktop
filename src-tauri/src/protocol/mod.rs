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
///
/// 档位必须在路径里，有两个作用：`<video>` 靠 URL 变化感知「换了资源」并重新加载；
/// 它同时是缓存的键（见 [`stream`]），所以同一集的不同档位各占一份数据。
pub fn stream_url(vid: &str, definition: u32) -> String {
    format!("http://{STREAM_SCHEME}.localhost/s/{definition}/{vid}")
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
/// 同样必须走 `http://{scheme}.localhost` 形式，原因见 [`stream_url`]。
pub fn local_url(encoded: &str) -> String {
    format!("http://{LOCAL_SCHEME}.localhost/f/{encoded}")
}
