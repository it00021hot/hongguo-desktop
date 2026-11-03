//! 三条协议路径的供数入口，只产出 `ProtocolResponse` 元组（状态码、响应头、响应体），HTTP 组装是壳层注册胶水的职责。
//!
//! 错误语义：[`respond_local`] / [`respond_stream`] 的 `Err` 由壳层映射为 400/404；[`respond_cover`] 自带 404 兜底（前端 `<img>` onError 落占位图）。

use hongguo_stream::range::ProtocolResponse;

use crate::cover;
use crate::local;
use crate::url::parse_stream_path;

/// 供给本地文件。`raw_path` 是请求 URI 的 path（`/f/<b64>`，wry 转回后信息都在 path 上）。
pub fn respond_local(raw_path: &str, range: Option<&str>) -> Result<ProtocolResponse, String> {
    let path = raw_path
        .trim_start_matches('/')
        .strip_prefix("f/")
        .unwrap_or_else(|| raw_path.trim_start_matches('/'));
    local::serve(path, range)
}

/// 在线流请求的路径错误：URL 不是本协议生成的形态（对不上就明确报错，不猜 vid）。
#[derive(Debug)]
pub enum StreamServeError {
    /// path 解析失败 → 壳层回 400
    BadPath(String),
    /// 流数据面拒绝（无此流/未注册）→ 壳层回 404
    Serve(String),
}

/// 供给在线流。`path` 是请求 URI 的 path（`/s/{档位}/{vid}`）；档位不只是路由信息，它就是缓存的键。
pub fn respond_stream(
    path: &str,
    range: Option<&str>,
) -> Result<ProtocolResponse, StreamServeError> {
    let Some((definition, vid)) = parse_stream_path(path) else {
        return Err(StreamServeError::BadPath(format!(
            "无法解析的流地址: {path}"
        )));
    };
    hongguo_stream::serve(vid, definition, range).map_err(StreamServeError::Serve)
}

/// 供给封面/头像转码。`path` 是请求 URI 的 path（`/c/<b64>`）；任何失败折叠为空体 404，前端 `<img>` onError 落占位图。
pub fn respond_cover(path: &str) -> ProtocolResponse {
    match cover::serve(path) {
        Ok(resp) => resp,
        Err(_) => (404, Vec::new(), Vec::new()),
    }
}
