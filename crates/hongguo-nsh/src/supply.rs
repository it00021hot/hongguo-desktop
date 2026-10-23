//! 本地 HTTP 供数桥：把 `hongguo-protocol` 的供数逻辑挂到本机端口上。
//!
//! 为什么需要它：nsh 的 `video` 元素与 `image` 元素只认 URL（`WebSource::Uri` /
//! `ImageSrc`），而在线流是**内存渐进流**、封面是 **HEIC 需转码**——两者都必须
//! 经 `hongguo-protocol` 现算。Tauri 壳用自定义 scheme + wry 拦截，GPUI 壳自己
//! 解码；NSH 壳走系统播放器，只有 HTTP 一条路（demo 已验证 MediaEngine 与
//! AVPlayer 都能吃）。
//!
//! 路径形态与协议 crate 完全一致，只是前面多了 `http://127.0.0.1:{port}`：
//! - `/s/{档位}/{vid}` → `respond_stream`（在线流 + Range）
//! - `/c/{base64url(远程地址)}` → `respond_cover`（HEIC → JPEG 三级阶梯）
//!
//! 执行纪律沿用协议层那套三层防线：请求进常驻工作池（不一条请求一条线程），
//! 回调体与 serve 各自 `catch_unwind`，炸了回 500 而不是 abort 全进程。

use std::io;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::OnceLock;

use base64::Engine;
use tiny_http::{Header, Method, Request, Response, Server};

use hongguo_protocol::{
    StreamServeError, guarded, panic_message, respond_cover, respond_stream, stream_pool,
};

/// 进程级端口（桥只起一次）。
static PORT: OnceLock<u16> = OnceLock::new();

/// 起桥（幂等）并返回端口。
pub fn start() -> u16 {
    *PORT.get_or_init(|| {
        let server = Server::http("127.0.0.1:0").expect("供数桥绑定失败");
        let port = server
            .server_addr()
            .to_ip()
            .expect("绑定结果必须是 IP 地址")
            .port();
        std::thread::Builder::new()
            .name("hongguo-nsh-http".into())
            .spawn(move || {
                loop {
                    let Ok(request) = server.recv() else {
                        break;
                    };
                    dispatch(request);
                }
            })
            .expect("供数桥线程创建失败");
        log::info!("[Supply] 本地供数桥就绪 127.0.0.1:{port}");
        port
    })
}

/// 在线流地址（喂 `video` 元素）。
pub fn stream_url(definition: u32, vid: &str) -> String {
    format!("http://127.0.0.1:{}/s/{definition}/{vid}", start())
}

/// 封面代理地址（喂 `image` 元素）：远程地址 base64url 后拼 path，
/// 与 Tauri 壳 `utils/cover.ts` 同一套编码。
pub fn cover_url(remote: &str) -> String {
    if remote.is_empty() {
        return String::new();
    }
    let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(remote.as_bytes());
    format!("http://127.0.0.1:{}/c/{encoded}", start())
}

fn dispatch(request: Request) {
    let range = request
        .headers()
        .iter()
        .find(|h| h.field.equiv("Range"))
        .map(|h| h.value.as_str().to_owned());
    let head_only = *request.method() == Method::Head;
    guarded(stream_pool(), "hongguo-nsh-http", move || {
        Box::new(move || {
            if let Err(error) = serve(request, range.as_deref(), head_only) {
                log::error!("[Supply] 回包失败: {error}");
            }
        })
    });
}

fn serve(request: Request, range: Option<&str>, head_only: bool) -> Result<(), io::Error> {
    let path = request.url().to_owned();
    let served = catch_unwind(AssertUnwindSafe(|| respond(&path, range)));
    let (status, headers, body) = match served {
        Ok(tuple) => tuple,
        Err(payload) => (500, vec![], panic_message(&payload).into_bytes()),
    };
    let mut response = Response::from_data(if head_only { Vec::new() } else { body })
        .with_status_code(tiny_http::StatusCode(status));
    for (name, value) in headers {
        if let Ok(header) = Header::from_bytes(name.as_bytes(), value.as_bytes()) {
            response = response.with_header(header);
        }
    }
    request.respond(response)
}

/// 路由：封面走 `/c/`，其余按在线流处理（与协议 crate 的路径语义一致）。
fn respond(path: &str, range: Option<&str>) -> (u16, Vec<(String, String)>, Vec<u8>) {
    if path.starts_with("/c/") {
        return respond_cover(path);
    }
    match respond_stream(path, range) {
        Ok(tuple) => tuple,
        Err(StreamServeError::BadPath(message)) => (400, vec![], message.into_bytes()),
        Err(StreamServeError::Serve(message)) => {
            // 未注册/未就绪：播放器会重试，日志留痕但降级到 debug 免得刷屏。
            log::debug!("[Supply] 流未就绪: {message}");
            (404, vec![], message.into_bytes())
        }
    }
}
