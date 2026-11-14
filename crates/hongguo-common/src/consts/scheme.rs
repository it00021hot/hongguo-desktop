//! 自定义 URI scheme 名（前后端契约）：前端 CSP（`tauri.conf.json` 的 `media-src`）与 WebView 的拦截注册都依赖这些字面量。
//!
//! 改名等于改契约，前端与 Rust 必须同 commit；值一旦发布，非破坏性升级不得更改。

/// 本地成品文件播放（支持 Range 拖动进度条）。
pub const LOCAL: &str = "hongguo-local";

/// 在线播放的内存渐进流（同样支持 Range）。
pub const STREAM: &str = "hongguo-stream";
