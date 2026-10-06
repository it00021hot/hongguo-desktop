//! 自定义 URI 协议注册。
//!
//! 注册三个 scheme：
//! - `hongguo-local://` —— 本地成品文件，支持 Range（拖进度条要用）
//! - `hongguo-stream://` —— 在线播放的内存渐进流，同样支持 Range
//! - `hongguo-cover://` —— HEIC 封面下载转 JPEG（磁盘缓存）
//!
//! 用异步 handler：可以挂起等待数据就绪（在线播放「边下边看」的基础），
//! 而不是取不到就报错。

use tauri::http::{Request, Response, StatusCode};

use super::{cover, local, parse_stream_path, stream, COVER_SCHEME, LOCAL_SCHEME, STREAM_SCHEME};

/// 注册全部自定义协议。必须在 `setup` 之前调用（Builder 阶段）。
///
/// ⚠️ stream/local 的取数**必须丢到后台线程**：macOS 的 WKURLSchemeHandler
/// 在**主线程**上调本回调，而流的供给会阻塞等待数据就绪（`wait_until`/
/// `wait_cover`，最长 30s）——在回调里同步等，等于每次视频请求等数据就把
/// 整个 UI 冻住（「快速滚动几下就卡死」的根因；Windows 的 WebView2 回调在
/// 非 UI 线程，开发期从未暴露）。`responder` 本就设计为可跨线程回包。
pub fn register(builder: tauri::Builder<tauri::Wry>) -> tauri::Builder<tauri::Wry> {
    builder
        .register_asynchronous_uri_scheme_protocol(LOCAL_SCHEME, move |_ctx, request, responder| {
            let range = range_header(&request);
            let path = request.uri().path().to_string();
            std::thread::spawn(move || {
                responder.respond(serve_local(&path, range));
            });
        })
        .register_asynchronous_uri_scheme_protocol(
            STREAM_SCHEME,
            move |_ctx, request, responder| {
                let range = range_header(&request);
                let path = request.uri().path().to_string();
                std::thread::spawn(move || {
                    responder.respond(serve_stream(&path, range));
                });
            },
        )
        .register_asynchronous_uri_scheme_protocol(COVER_SCHEME, move |_ctx, request, responder| {
            // 首次命中的封面要下载 + ffmpeg 转码（秒级阻塞），丢到后台线程
            // 出结果再回包——协议回调里不能同步等它
            let path = request.uri().path().to_string();
            std::thread::spawn(move || {
                let response = cover::serve(&path);
                let response = response.unwrap_or_else(|_| {
                    (StatusCode::NOT_FOUND.as_u16(), Vec::new(), Vec::new())
                });
                responder.respond(build_response(response.0, response.1, response.2));
            });
        })
}

/// 取 `Range` 请求头。
fn range_header(request: &Request<Vec<u8>>) -> Option<String> {
    request
        .headers()
        .get("range")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
}

/// 供给本地文件。`path` 是请求 URI 的 path（`/f/<b64>`）。
fn serve_local(path: &str, range: Option<String>) -> Response<Vec<u8>> {
    // wry 把 `http://hongguo-local.localhost/f/<b64>` 转回 `hongguo-local://localhost/f/<b64>`，
    // 所以这里拿到的是 `/f/<b64>`，要去掉 `f/` 段才是编码后的路径。
    let path = path
        .trim_start_matches('/')
        .strip_prefix("f/")
        .unwrap_or_else(|| path.trim_start_matches('/'));
    match local::serve(path, range.as_deref()) {
        Ok((status, headers, data)) => build_response(status, headers, data),
        Err(msg) => text_response(StatusCode::BAD_REQUEST, &msg),
    }
}

/// 供给在线流。`path` 是请求 URI 的 path（`/s/{档位}/{vid}`）。
fn serve_stream(path: &str, range: Option<String>) -> Response<Vec<u8>> {
    // wry 转回后的 URI 是 `hongguo-stream://localhost/s/{档位}/{vid}`：host 恒为
    // localhost，档位与 vid 都在 path 上。档位不只是路由信息，它就是缓存的键，
    // 决定这份请求去读哪一份数据。
    let Some((definition, vid)) = parse_stream_path(path) else {
        return text_response(
            StatusCode::BAD_REQUEST,
            &format!("无法解析的流地址: {path}"),
        );
    };
    match stream::serve(vid, definition, range.as_deref()) {
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
    fn stream_url_uses_the_platform_scheme_form() {
        // Windows 走 http://{scheme}.localhost（WebView2 拦截约定），
        // macOS/Linux 走 {scheme}://localhost（WKURLSchemeHandler/WebKit 拦真 scheme）。
        // 形态给错的表现：<video> 拿到网络错误，MediaError 4 黑屏。
        let url = crate::protocol::stream_url("7687919221593885758", 1080);
        let base = crate::protocol::scheme_base(crate::protocol::STREAM_SCHEME);
        assert_eq!(url, format!("{base}/s/1080/7687919221593885758"));
    }

    #[test]
    fn stream_url_changes_with_definition() {
        // 同一集换清晰度时 URL 必须变，否则 <video> 认为资源没换、不会重新加载
        let base = crate::protocol::stream_url("v-42", 1080);
        assert_ne!(base, crate::protocol::stream_url("v-42", 720), "换档");
    }

    #[test]
    fn stream_path_survives_the_round_trip() {
        // 请求进来时 wry 已把 scheme 换回自定义协议，host 恒为 localhost，信息在 path 上
        let url = crate::protocol::stream_url("v-42", 720);
        let uri: Uri = url.parse().expect("URL 应可解析");
        assert_eq!(uri.path().trim_start_matches('/'), "s/720/v-42");
        assert_eq!(
            crate::protocol::parse_stream_path(uri.path()),
            Some((720, "v-42")),
            "带档位的 path 也要能取回档位与 vid"
        );
    }

    #[test]
    fn a_path_we_did_not_generate_is_rejected() {
        // path 只可能由 stream_url 生成，对不上就明确报错，不去猜 vid
        assert_eq!(crate::protocol::parse_stream_path("/v-42"), None);
        assert_eq!(crate::protocol::parse_stream_path("/s/abc/v-42"), None);
        assert_eq!(crate::protocol::parse_stream_path("/s/720"), None);
    }

    #[test]
    fn local_url_uses_the_platform_scheme_form() {
        let url = crate::protocol::local::local_play_url("D:\\a.mp4").unwrap();
        let base = crate::protocol::scheme_base(crate::protocol::LOCAL_SCHEME);
        assert!(url.starts_with(&format!("{base}/f/")), "实际: {url}");
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
