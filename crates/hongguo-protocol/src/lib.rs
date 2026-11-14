//! service 与 NSH 共用的媒体来源标识；实际供数由 NSH 的本机 HTTP 服务承担。

pub mod local;
pub mod url;

pub use url::{LOCAL_SCHEME, STREAM_SCHEME, local_url, parse_stream_path, scheme_base, stream_url};
