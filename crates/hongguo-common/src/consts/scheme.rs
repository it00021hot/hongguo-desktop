//! 媒体来源的内部 URI 标识；由媒体供数层解析成临时 HTTP Range 地址。

/// 本地成品文件播放（支持 Range 拖动进度条）。
pub const LOCAL: &str = "hongguo-local";

/// 在线播放的内存渐进流（同样支持 Range）。
pub const STREAM: &str = "hongguo-stream";
