//! 自定义 URI 协议的 Tauri 注册胶水。
//!
//! 供数逻辑（URL 编解码、工作池、Range 应答、封面转码阶梯）都在
//! `hongguo-protocol` crate；本文件只做两件事（协议注册归壳）：
//! 1. 把三个 scheme 挂到 Tauri Builder（异步 handler，可挂起等数据就绪）；
//! 2. 把协议层的元组产物组装成 `tauri::http` 响应，并执行 serve 侧的
//!    `catch_unwind`（第三层 panic 防线：炸了回 500，worker 与进程都活着）。
//!
//! ⚠️ stream/local/cover 的取数**必须留在后台池**：macOS 的 WKURLSchemeHandler
//! 在**主线程**上调本回调，而流的供给会阻塞等待数据就绪（轮询循环，最长
//! 30s）——在回调里同步等，等于每次视频请求等数据就把整个 UI 冻住
//! （「快速滚动几下就卡死」的根因；Windows 的 WebView2 回调在非 UI 线程，
//! 开发期从未暴露）。`responder` 本就设计为可跨线程回包。

use std::panic::{AssertUnwindSafe, catch_unwind};

use tauri::http::{Request, Response, StatusCode};

use hongguo_common::consts::scheme::{
    COVER as COVER_SCHEME, LOCAL as LOCAL_SCHEME, STREAM as STREAM_SCHEME,
};
use hongguo_protocol::stream_pool;
use hongguo_protocol::{StreamServeError, respond_cover, respond_local, respond_stream};
use hongguo_protocol::{file_pool, guarded, panic_message};
use hongguo_stream::range::ProtocolResponse;

/// 注册全部自定义协议。必须在 `setup` 之前调用（Builder 阶段）。
pub fn register(builder: tauri::Builder<tauri::Wry>) -> tauri::Builder<tauri::Wry> {
    builder
        .register_asynchronous_uri_scheme_protocol(LOCAL_SCHEME, move |_ctx, request, responder| {
            guarded(file_pool(), LOCAL_SCHEME, move || {
                let range = range_header(&request);
                let path = request.uri().path().to_string();
                Box::new(move || {
                    let served =
                        catch_unwind(AssertUnwindSafe(|| respond_local(&path, range.as_deref())));
                    let response = match served {
                        Ok(Ok(resp)) => build_response(resp),
                        Ok(Err(msg)) => text_response(StatusCode::BAD_REQUEST, &msg),
                        Err(p) => panic_response(&p),
                    };
                    log_served(LOCAL_SCHEME, &path, range.as_deref(), &response);
                    responder.respond(response);
                })
            })
        })
        .register_asynchronous_uri_scheme_protocol(
            STREAM_SCHEME,
            move |_ctx, request, responder| {
                guarded(stream_pool(), STREAM_SCHEME, move || {
                    let range = range_header(&request);
                    let path = request.uri().path().to_string();
                    Box::new(move || {
                        let served = catch_unwind(AssertUnwindSafe(|| {
                            respond_stream(&path, range.as_deref())
                        }));
                        let response = match served {
                            Ok(Ok(resp)) => build_response(resp),
                            Ok(Err(StreamServeError::BadPath(msg))) => {
                                text_response(StatusCode::BAD_REQUEST, &msg)
                            }
                            Ok(Err(StreamServeError::Serve(msg))) => {
                                text_response(StatusCode::NOT_FOUND, &msg)
                            }
                            Err(p) => panic_response(&p),
                        };
                        log_served(STREAM_SCHEME, &path, range.as_deref(), &response);
                        responder.respond(response);
                    })
                })
            },
        )
        .register_asynchronous_uri_scheme_protocol(COVER_SCHEME, move |_ctx, request, responder| {
            // 首次命中的封面要下载 + ffmpeg 转码（秒级阻塞），丢到后台池
            // 出结果再回包——协议回调里不能同步等它
            guarded(file_pool(), COVER_SCHEME, move || {
                let path = request.uri().path().to_string();
                Box::new(move || {
                    let served = catch_unwind(AssertUnwindSafe(|| respond_cover(&path)));
                    let response = match served {
                        Ok(resp) => build_response(resp),
                        Err(p) => panic_response(&p),
                    };
                    log_served(COVER_SCHEME, &path, None, &response);
                    responder.respond(response);
                })
            })
        })
}

/// 协议请求日志（info 级）：三端共用一条口径——path / range / 状态 / 字节
/// 数。此前协议层全静默，「图片/视频挂了」在日志里没有任何痕迹，排查只能
/// 靠盲猜（2026-11 Android「像没网」实录）。body 只取 len，不拷贝。
fn log_served(scheme: &str, path: &str, range: Option<&str>, response: &Response<Vec<u8>>) {
    log::info!(
        "[Proto:{scheme}] {path} range={} → {} {}B",
        range.unwrap_or("-"),
        response.status(),
        response.body().len()
    );
}

/// 取 `Range` 请求头。
fn range_header(request: &Request<Vec<u8>>) -> Option<String> {
    request
        .headers()
        .get("range")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
}

/// serve panic 时的兜底响应：把错误给到请求方（`<img>` 的 onError、
/// `<video>` 的 MediaError 会接住），worker 与进程都活着。
fn panic_response(payload: &Box<dyn std::any::Any + Send>) -> Response<Vec<u8>> {
    text_response(
        StatusCode::INTERNAL_SERVER_ERROR,
        &format!("协议处理器内部错误: {}", panic_message(payload)),
    )
}

/// 把协议层的元组产物组装成 HTTP 响应。
///
/// 统一携带 `Access-Control-Allow-Origin: *`：播放页 `<video>` 以
/// `crossOrigin="anonymous"` 加载流与本地文件——WebGPU 画质增强要求画面
/// origin-clean，否则 `copyExternalImageToTexture` 判跨源直接拒绝；媒体
/// 请求能否过 CORS 检查就看协议响应有没有这个头（对齐官方 CDN 行为：
/// 其视频直链本就回 ACAO）。
fn build_response((status, headers, body): ProtocolResponse) -> Response<Vec<u8>> {
    let mut builder = Response::builder()
        .status(status)
        .header("Access-Control-Allow-Origin", "*");
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
                .status(status)
                .body(Vec::new())
                .expect("空响应总能构造")
        })
}
