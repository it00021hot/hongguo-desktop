//! 红果取流链路（与 hg-nsh 的 mixed demo 同一条已验证链路，纯 Rust 形态）：
//! 推荐流选剧 → 分集列表 → `online::prepare` 取流 + CENC 解密 → 本地 HTTP
//! 供数桥。等注册完成才返回播放 URL（慢网下 AVPlayer 提前请求会吃 404，
//! AVPlayerItem 判加载失败后画面定格且不恢复）。

use std::sync::{Arc, OnceLock};

use hongguo_service::app_state::{AppState, AppStateInner};

/// 进程级服务面：内存库 + 静态设备档案（demo 不落盘、不登录）。
pub fn state() -> AppState {
    static STATE: OnceLock<AppState> = OnceLock::new();
    STATE
        .get_or_init(|| Arc::new(AppStateInner::default()))
        .clone()
}

/// 一集的标识与展示信息。
#[derive(Debug, Clone, PartialEq)]
pub struct Episode {
    /// 集号，从 1 开始
    pub index: u32,
    /// 平台侧的 vid（prepare 的键）
    pub vid: String,
}

/// 选剧结果：一部多集短剧与它的可播分集。
#[derive(Debug, Clone)]
pub struct Series {
    /// 平台侧剧集 id（弹幕接口的 book_id）
    pub series_id: String,
    /// 剧名
    pub title: String,
    /// 分集（按集号升序）
    pub episodes: Vec<Episode>,
    /// 起播下标（`episodes` 的下标，推荐位所在集）
    pub start: usize,
}

/// 一条弹幕的内容与出现时刻（视频内时间轴，秒）。
#[derive(Debug, Clone)]
pub struct DanmakuLine {
    pub time_s: f32,
    pub text: String,
}

/// 本地 HTTP 供数桥（tiny_http + `hongguo_protocol::respond_stream`）。
mod server {
    use std::io;
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::sync::OnceLock;

    use tiny_http::{Header, Method, Request, Response, Server};

    use hongguo_protocol::stream_pool;
    use hongguo_protocol::{StreamServeError, guarded, panic_message, respond_stream};

    static PORT: OnceLock<u16> = OnceLock::new();

    /// 起服务（幂等）并返回端口。
    pub fn start() -> u16 {
        *PORT.get_or_init(|| {
            let server = Server::http("127.0.0.1:0").expect("本地供数服务绑定失败");
            let port = server
                .server_addr()
                .to_ip()
                .expect("绑定结果必须是 IP 地址")
                .port();
            std::thread::Builder::new()
                .name("hongguo-nsh-demo-http".into())
                .spawn(move || {
                    loop {
                        let Ok(request) = server.recv() else {
                            break;
                        };
                        dispatch(request);
                    }
                })
                .expect("本地供数服务线程创建失败");
            port
        })
    }

    /// 播放地址：与 `hongguo-stream` 协议同形，走本机 HTTP。
    pub fn stream_url(definition: u32, vid: &str) -> String {
        format!("http://127.0.0.1:{}/s/{}/{}", start(), definition, vid)
    }

    fn dispatch(request: Request) {
        let range = request
            .headers()
            .iter()
            .find(|h| h.field.equiv("Range"))
            .map(|h| h.value.as_str().to_owned());
        let head_only = *request.method() == Method::Head;
        guarded(stream_pool(), "hongguo-nsh-demo-http", move || {
            Box::new(move || {
                if let Err(error) = serve(request, range.as_deref(), head_only) {
                    log::error!("[HgNshRs] 回包失败: {error}");
                }
            })
        });
    }

    fn serve(request: Request, range: Option<&str>, head_only: bool) -> Result<(), io::Error> {
        let path = request.url().to_owned();
        let served = catch_unwind(AssertUnwindSafe(|| respond_stream(&path, range)));
        let (status, headers, body) = match served {
            Ok(Ok(tuple)) => tuple,
            Ok(Err(StreamServeError::BadPath(message))) => (400, vec![], message.into_bytes()),
            Ok(Err(StreamServeError::Serve(message))) => (404, vec![], message.into_bytes()),
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
}

/// 从推荐流选一部多集短剧并拉回分集。跑在调用方的后台任务上下文里
/// （nsh 的 `background` / hongguo 的运行时皆可）。
pub async fn bootstrap() -> Result<Series, String> {
    let env = state().api_env();

    let mut items = Vec::new();
    for tab in ["16", "36", "39"] {
        match hongguo_api::recommend::fetch_recommend_feed(tab, "", 0, &[], &env).await {
            Ok(page) => items.extend(page.items),
            Err(e) => log::warn!("[HgNshRs] 推荐 tab={tab} 拉取失败: {e}"),
        }
    }
    // 要能切集：挑第一部多集、series_id 与 vid 齐全的剧
    let Some(item) = items
        .iter()
        .find(|i| !i.series_id.is_empty() && !i.vid.is_empty() && i.episode_cnt >= 2)
    else {
        return Err("推荐流里没有可用的多集短剧".into());
    };
    log::info!("[HgNshRs] 选中《{}》series={}", item.title, item.series_id);

    let list = hongguo_api::detail::fetch_episode_list(&item.series_id, &env)
        .await
        .map_err(|e| format!("分集列表拉取失败: {e}"))?;
    let episodes: Vec<Episode> = list
        .episodes
        .iter()
        .filter(|e| !e.vid.is_empty())
        .map(|e| Episode {
            index: e.vid_index,
            vid: e.vid.clone(),
        })
        .collect();
    if episodes.len() < 2 {
        return Err(format!(
            "《{}》只有 {} 个可播集，没法演示切集",
            list.title,
            episodes.len()
        ));
    }
    let start = list
        .episodes
        .iter()
        .position(|e| e.vid == item.vid)
        .unwrap_or(0);

    Ok(Series {
        series_id: item.series_id.clone(),
        title: if list.title.is_empty() {
            item.title.clone()
        } else {
            list.title
        },
        episodes,
        start,
    })
}

/// 拉一集的真实弹幕（commentapi，按出现时间升序）。密度与内容都来自
/// 平台——demo 不再造假弹幕。
pub async fn danmaku_for(vid: String, series_id: String) -> Result<Vec<DanmakuLine>, String> {
    let env = state().api_env();
    let all = hongguo_api::danmaku::fetch_danmaku_all(&vid, &series_id, &env)
        .await
        .map_err(|e| e.to_string())?;
    Ok(all
        .into_iter()
        .map(|d| DanmakuLine {
            time_s: d.offset_ms as f32 / 1000.0,
            text: d.text,
        })
        .collect())
}

/// 准备一集：取流 + 建解密计划 + 等渐进流注册，返回可直接喂 `Video` 的
/// 本地供数 URL。注册等待的上限对齐 serve 自身的 30s 预算。
pub async fn prepare(vid: String) -> Result<String, String> {
    const REGISTER_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
    const REGISTER_POLL: std::time::Duration = std::time::Duration::from_millis(20);

    let prepared =
        hongguo_service::play_service::online::prepare(&state(), &vid, "hongguo-nsh-demo", None)
            .await
            .map_err(|e| e.to_string())?;
    let deadline = std::time::Instant::now() + REGISTER_TIMEOUT;
    while hongguo_stream::total_len(&vid, prepared.definition).is_none() {
        if std::time::Instant::now() >= deadline {
            log::warn!(
                "[HgNshRs] {vid}@{} 注册等待超时，交由 serve 等待兜底",
                prepared.definition
            );
            break;
        }
        tokio::time::sleep(REGISTER_POLL).await;
    }
    Ok(server::stream_url(prepared.definition, &vid))
}
