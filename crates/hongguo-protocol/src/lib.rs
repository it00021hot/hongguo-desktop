//! 自定义 URI 协议供数逻辑：`hongguo-local://` 本地成品（Range）、`hongguo-stream://` 在线渐进流（URL 解析与路由）、`hongguo-cover://` HEIC 封面转 JPEG。
//!
//! 本 crate 不依赖 tauri：供数函数只产出 `(状态码, 响应头, 响应体)` 元组，HTTP 组装与 Builder 注册是壳层职责；[`pool`] 的工作池与 catch_unwind 是协议路径的执行纪律。

pub mod cover;
pub mod grow;
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
