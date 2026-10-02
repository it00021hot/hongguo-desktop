//! 自定义 URI 协议注册。
//!
//! 注册两个 scheme：
//! - `hongguo-local://` —— 本地成品文件，支持 Range（拖进度条要用）
//! - `hongguo-stream://` —— 在线播放的内存渐进流，同样支持 Range
//!
//! 用异步 handler：可以挂起等待数据就绪（在线播放「边下边看」的基础），
//! 而不是取不到就报错。

use tauri::http::{Request, Response, StatusCode};

use super::{local, stream, LOCAL_SCHEME, STREAM_SCHEME};

/// 注册全部自定义协议。必须在 `setup` 之前调用（Builder 阶段）。
pub fn register(builder: tauri::Builder<tauri::Wry>) -> tauri::Builder<tauri::Wry> {
    builder
        .register_asynchronous_uri_scheme_protocol(LOCAL_SCHEME, move |_ctx, request, responder| {
            let range = range_header(&request);
            responder.respond(serve_local(&request, range));
        })
        .register_asynchronous_uri_scheme_protocol(
            STREAM_SCHEME,
            move |_ctx, request, responder| {
                let range = range_header(&request);
                responder.respond(serve_stream(&request, range));
            },
        )
}

/// 取 `Range` 请求头。
fn range_header(request: &Request<Vec<u8>>) -> Option<String> {
    request
        .headers()
        .get("range")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
}

/// 供给本地文件。
fn serve_local(request: &Request<Vec<u8>>, range: Option<String>) -> Response<Vec<u8>> {
    // wry 把 `http://hongguo-local.localhost/f/<b64>` 转回 `hongguo-local://localhost/f/<b64>`，
    // 所以这里拿到的是 `/f/<b64>`，要去掉 `f/` 段才是编码后的路径。
    let path = request
        .uri()
        .path()
        .trim_start_matches('/')
        .strip_prefix("f/")
        .unwrap_or_else(|| request.uri().path().trim_start_matches('/'));
    match local::serve(path, range.as_deref()) {
        Ok((status, headers, data)) => build_response(status, headers, data),
        Err(msg) => text_response(StatusCode::BAD_REQUEST, &msg),
    }
}

/// 供给在线流。
fn serve_stream(request: &Request<Vec<u8>>, range: Option<String>) -> Response<Vec<u8>> {
    // 转回后的 URI 是 `hongguo-stream://localhost/<vid>`：vid 在 path 上，
    // host 恒为 localhost，所以只能取 path。
    let vid = request.uri().path().trim_start_matches('/');
    match stream::serve(vid, range.as_deref()) {
        Ok((status, headers, data)) => build_response(status, headers, data),
        Err(msg) => text_response(StatusCode::NOT_FOUND, &msg),
    }
}

/// 构造 HTTP 响应。
fn build_response(status: u16, headers: Vec<(String, String)>, body: Vec<u8>) -> Response<Vec<u8>> {
    let mut builder = Response::builder().status(status);
    for (k, v) in headers {
        builder = builder.header(k, v);
    }
    builder.body(body).unwrap_or_else(|_| {
        Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(Vec::new())
            .expect("空响应总能构造")
    })
}

/// 纯文本错误响应。
fn text_response(status: StatusCode, msg: &str) -> Response<Vec<u8>> {
    Response::builder()
        .status(status)
        .header("Content-Type", "text/plain; charset=utf-8")
        .body(msg.as_bytes().to_vec())
        .unwrap_or_else(|_| {
            Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Vec::new())
                .expect("空响应总能构造")
        })
}

#[cfg(test)]
mod tests {
    use tauri::http::Uri;

    #[test]
    fn stream_url_uses_the_localhost_form_wry_requires() {
        // 直接写 hongguo-stream://<vid> 不会被 WebView2 拦下来，协议永远收不到请求
        let url = crate::protocol::stream_url("7687919221593885758");
        assert_eq!(
            url,
            "http://hongguo-stream.localhost/7687919221593885758"
        );
    }

    #[test]
    fn stream_vid_survives_the_round_trip() {
        let url = crate::protocol::stream_url("v-42");
        let uri: Uri = url.parse().expect("URL 应可解析");
        // 请求进来时 wry 已把 scheme 换回自定义协议，host 恒为 localhost，vid 在 path 上
        assert_eq!(uri.path().trim_start_matches('/'), "v-42");
    }

    #[test]
    fn local_url_uses_the_localhost_form_too() {
        let url = crate::protocol::local::local_play_url("D:\\a.mp4").unwrap();
        assert!(
            url.starts_with("http://hongguo-local.localhost/f/"),
            "实际: {url}"
        );
    }

    #[test]
    fn local_path_strips_the_f_segment() {
        // 处理器收到的 path 是 `/f/<b64>`，必须剥掉 `f/` 才是编码后的路径
        let url = crate::protocol::local::local_play_url("D:\\a.mp4").unwrap();
        let uri: Uri = url.parse().unwrap();
        let raw = uri.path().trim_start_matches('/');
        assert!(raw.starts_with("f/"));
        let encoded = raw.strip_prefix("f/").expect("应剥掉 f/");
        assert_eq!(
            crate::protocol::local::resolve_path(encoded).unwrap(),
            std::path::PathBuf::from("D:\\a.mp4")
        );
    }
}
