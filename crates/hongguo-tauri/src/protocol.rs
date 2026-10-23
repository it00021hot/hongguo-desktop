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
                    responder.respond(match served {
                        Ok(Ok(resp)) => build_response(resp),
                        Ok(Err(msg)) => text_response(StatusCode::BAD_REQUEST, &msg),
                        Err(p) => panic_response(&p),
                    });
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
                        responder.respond(match served {
                            Ok(Ok(resp)) => build_response(resp),
                            Ok(Err(StreamServeError::BadPath(msg))) => {
                                text_response(StatusCode::BAD_REQUEST, &msg)
                            }
                            Ok(Err(StreamServeError::Serve(msg))) => {
                                text_response(StatusCode::NOT_FOUND, &msg)
                            }
                            Err(p) => panic_response(&p),
                        });
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
                    responder.respond(match served {
                        Ok(resp) => build_response(resp),
                        Err(p) => panic_response(&p),
                    });
                })
            })
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

/// serve panic 时的兜底响应：把错误给到请求方（`<img>` 的 onError、
/// `<video>` 的 MediaError 会接住），worker 与进程都活着。
fn panic_response(payload: &Box<dyn std::any::Any + Send>) -> Response<Vec<u8>> {
    text_response(
        StatusCode::INTERNAL_SERVER_ERROR,
        &format!("协议处理器内部错误: {}", panic_message(payload)),
    )
}

/// 把协议层的元组产物组装成 HTTP 响应。
fn build_response((status, headers, body): ProtocolResponse) -> Response<Vec<u8>> {
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
                .status(status)
                .body(Vec::new())
                .expect("空响应总能构造")
        })
}
