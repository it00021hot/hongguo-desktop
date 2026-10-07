//! 自定义 URI 协议注册。
//!
//! 注册三个 scheme：
//! - `hongguo-local://` —— 本地成品文件，支持 Range（拖进度条要用）
//! - `hongguo-stream://` —— 在线播放的内存渐进流，同样支持 Range
//! - `hongguo-cover://` —— HEIC 封面下载转 JPEG（磁盘缓存）
//!
//! 用异步 handler：可以挂起等待数据就绪（在线播放「边下边看」的基础），
//! 而不是取不到就报错。
//!
//! 执行模型（2026-10-07 事故后加固）：WKURLSchemeHandler 在**主线程**的
//! extern "C" 边界里调这些回调，panic 无法 unwind 出去，std 只能 abort 全
//! 进程——当天实测：回调里的 `thread::spawn` 失败 panic → abort 卡死在内核
//! → 进程进入不可中断等待杀不死，数据库文件锁被占，新实例全部起不来。
//! 三层防线：
//! 1. 回调体整体 `catch_unwind`，panic 到不了 FFI 边界；
//! 2. 请求不再「一条一线程」，由常驻工作池消费（无界 per-request 线程
//!    正是上述 spawn 失败的温床）；
//! 3. serve 本身也 `catch_unwind`，炸了回 500，worker 与进程都活着。

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::mpsc::{self, Sender};
use std::sync::{Mutex, OnceLock};

use tauri::http::{Request, Response, StatusCode};

use super::{cover, local, parse_stream_path, stream, COVER_SCHEME, LOCAL_SCHEME, STREAM_SCHEME};

type Job = Box<dyn FnOnce() + Send + 'static>;

/// 流池大小：在线播放的 Range 请求（serve 内部有最长 30s 的等数据循环），
/// 与浏览器对单 host 的并发上限（≤6）对齐。
const STREAM_WORKERS: usize = 6;
/// 文件池大小：本地文件读 + 封面下载转码。与流池分开，流的长时间等待
/// 不至于把封面/本地播放饿死。
const FILE_WORKERS: usize = 6;

static STREAM_POOL: OnceLock<WorkerPool> = OnceLock::new();
static FILE_POOL: OnceLock<WorkerPool> = OnceLock::new();

/// 常驻工作线程池。进程生命周期内不销毁——退出时 worker 阻塞在 recv 上，
/// 随进程一起消亡，不存在「退出中还要起线程」的窗口。
struct WorkerPool {
    tx: Sender<Job>,
}

impl WorkerPool {
    fn new(workers: usize, prefix: &str) -> Self {
        let (tx, rx) = mpsc::channel::<Job>();
        let rx = std::sync::Arc::new(Mutex::new(rx));
        let prefix = prefix.to_string();
        for i in 0..workers {
            // Builder::spawn 返回 Result 而非 panic：个别 worker 起不来只是
            // 池变小，不构成事件
            let _ = std::thread::Builder::new()
                .name(format!("{prefix}-{i}"))
                .spawn({
                    let rx = std::sync::Arc::clone(&rx);
                    let prefix = prefix.clone();
                    move || loop {
                        // 持锁等待下一条任务。锁只在 recv 期间持有，任务在锁外
                        // 执行，任务里的 panic 不可能毒化这把锁
                        let job = rx
                            .lock()
                            .expect("池接收端的锁只在 recv 时持有，不会中毒")
                            .recv();
                        match job {
                            Ok(job) => {
                                if let Err(p) = catch_unwind(AssertUnwindSafe(job)) {
                                    log::error!(
                                        "[Protocol] {prefix} 工作线程任务 panic（已拦截）: {}",
                                        panic_message(&p)
                                    );
                                }
                            }
                            Err(_) => break, // 发送端是 static，正常到不了这里
                        }
                    }
                });
        }
        Self { tx }
    }

    /// 派发一条任务。池还在就进队列；池全灭（理论下限）回退直启线程，再
    /// 失败就记日志放弃——这条链路上没有任何会 panic 的调用。
    fn dispatch(&self, job: Job) {
        if let Err(err) = self.tx.send(job) {
            log::error!("[Protocol] 工作池不可用，回退直启线程");
            let _ = std::thread::Builder::new()
                .name("hongguo-proto-fallback".into())
                .spawn(move || (err.0)());
        }
    }
}

fn stream_pool() -> &'static WorkerPool {
    STREAM_POOL.get_or_init(|| WorkerPool::new(STREAM_WORKERS, "hongguo-proto-stream"))
}

fn file_pool() -> &'static WorkerPool {
    FILE_POOL.get_or_init(|| WorkerPool::new(FILE_WORKERS, "hongguo-proto-file"))
}

/// 注册全部自定义协议。必须在 `setup` 之前调用（Builder 阶段）。
///
/// ⚠️ stream/local/cover 的取数**必须留在后台池**：macOS 的 WKURLSchemeHandler
/// 在**主线程**上调本回调，而流的供给会阻塞等待数据就绪（轮询循环，最长
/// 30s）——在回调里同步等，等于每次视频请求等数据就把整个 UI 冻住
/// （「快速滚动几下就卡死」的根因；Windows 的 WebView2 回调在非 UI 线程，
/// 开发期从未暴露）。`responder` 本就设计为可跨线程回包。
pub fn register(builder: tauri::Builder<tauri::Wry>) -> tauri::Builder<tauri::Wry> {
    builder
        .register_asynchronous_uri_scheme_protocol(LOCAL_SCHEME, move |_ctx, request, responder| {
            guarded(file_pool(), LOCAL_SCHEME, move || {
                let range = range_header(&request);
                let path = request.uri().path().to_string();
                Box::new(move || {
                    let served = catch_unwind(AssertUnwindSafe(|| serve_local(&path, range)));
                    responder.respond(match served {
                        Ok(response) => response,
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
                        let served =
                            catch_unwind(AssertUnwindSafe(|| serve_stream(&path, range)));
                        responder.respond(match served {
                            Ok(response) => response,
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
                    let served = catch_unwind(AssertUnwindSafe(|| cover::serve(&path)));
                    responder.respond(match served {
                        Ok(Ok((status, headers, body))) => build_response(status, headers, body),
                        Ok(Err(_)) => build_response(
                            StatusCode::NOT_FOUND.as_u16(),
                            Vec::new(),
                            Vec::new(),
                        ),
                        Err(p) => panic_response(&p),
                    });
                })
            })
        })
}

/// 回调统一外壳：解析与派发都包在 `catch_unwind` 里。这是 panic 与 FFI
/// 边界之间的最后一道闸——哪怕未来有人在回调路径上引入新的 panic 源，
/// 也只是这一条请求失败，进程不会 abort。
fn guarded(pool: &'static WorkerPool, scheme: &str, mk_job: impl FnOnce() -> Job) {
    match catch_unwind(AssertUnwindSafe(mk_job)) {
        Ok(job) => pool.dispatch(job),
        Err(p) => log::error!(
            "[Protocol] {scheme} 回调本体 panic（已拦截，未穿透 FFI）: {}",
            panic_message(&p)
        ),
    }
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

/// serve panic 时的兜底响应：把错误给到请求方（`<img>` 的 onError、
/// `<video>` 的 MediaError 会接住），worker 与进程都活着。
fn panic_response(payload: &Box<dyn std::any::Any + Send>) -> Response<Vec<u8>> {
    text_response(
        StatusCode::INTERNAL_SERVER_ERROR,
        &format!("协议处理器内部错误: {}", panic_message(payload)),
    )
}

/// 把 `catch_unwind` 抓到的 payload 转成可读文本（默认 panic hook 只写
/// stderr，这里同时落到日志，方便事后从日志定位）。
fn panic_message(payload: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "（非文本 panic payload）".to_string()
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
    use std::sync::mpsc;
    use std::time::Duration;

    use tauri::http::Uri;

    /// 2026-10-07 事故回归：serve 侧 panic 只能损失那一条请求，工作池与
    /// 后续任务必须照常——「一条请求 panic 拖 abort 全进程」就是当天数据库
    /// 被锁一下午的根因。
    #[test]
    fn pool_serves_jobs_after_a_panicking_one() {
        let (tx, rx) = mpsc::channel();
        super::file_pool().dispatch(Box::new(move || panic!("模拟封面转码 panic")));
        super::file_pool().dispatch(Box::new(move || tx.send(1).unwrap()));
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(5)).unwrap(),
            1,
            "panic 的任务不能拖垮工作池"
        );
    }

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
