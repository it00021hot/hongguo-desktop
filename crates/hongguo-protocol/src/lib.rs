//! 自定义 URI 协议的供数逻辑（hongguo-protocol crate）。
//!
//! 三个 scheme（scheme 名定义在 [`hongguo_common::consts::scheme`]，本 crate
//! 按旧名再导出）：
//! - `hongguo-local://` —— 本地成品文件，支持 Range（[`local`]）
//! - `hongguo-stream://` —— 在线播放的内存渐进流（数据面在 `hongguo-stream`，
//!   这里只做 URL 解析与路由，见 [`serve`]）
//! - `hongguo-cover://` —— HEIC 封面下载转 JPEG，落盘缓存（[`cover`]）
//!
//! URL 形态（按平台二选一的 `http://{scheme}.localhost` / `{scheme}://localhost`）
//! 与 path 编解码在 [`url`]。
//!
//! **本 crate 不依赖 tauri**：供数函数只产出 `(状态码, 响应头, 响应体)` 元组
//! （[`hongguo_stream::range::ProtocolResponse`]），HTTP 组装与 Builder 注册是
//! 壳层的职责（协议注册归壳）。 GPUI
//! 壳不经过 URI 供数，不消费本 crate 的协议入口；[`pool`] 的工作池与 panic
//! 加固是协议路径的执行纪律，随供数逻辑同址。
//!
//! 执行模型（2026-10-07 事故后加固）：WKURLSchemeHandler 在**主线程**的
//! extern "C" 边界里调回调，panic 无法 unwind 出去，std 只能 abort 全进程。
//! 三层防线（[`pool`] + 壳层注册处）：
//! 1. 回调体整体 `catch_unwind`，panic 到不了 FFI 边界；
//! 2. 请求不「一条一线程」，由常驻工作池消费（[`pool`]）；
//! 3. serve 本身也 `catch_unwind`（壳层），炸了回 500，worker 与进程都活着。

pub mod cover;
pub mod local;
pub mod pool;
pub mod serve;
pub mod url;

pub use pool::{Job, WorkerPool, file_pool, guarded, panic_message, stream_pool};
pub use serve::{StreamServeError, respond_cover, respond_local, respond_stream};
pub use url::{
    COVER_SCHEME, LOCAL_SCHEME, STREAM_SCHEME, local_url, parse_stream_path, scheme_base,
    stream_url,
};
