//! Phase 0/1a 可行性 spike：`AVPlayer` + `AVPlayerLayer` 放进普通 GPUI 窗口。
//!
//! 门控：`HONGGUO_AV_SPIKE=1` 时 `main` 在装配任何业务之前转入本模块，
//! 默认启动路径完全不经过这里。
//!
//! 两种供数模式（`HONGGUO_AV_SPIKE_SOURCE`）：
//! - `file`（默认）：Phase 0 的整段内存样片，走 `Source::Bytes`；
//! - `online`：真实在线剧集，经 `hongguo-service` 的 `prepare` / `prefetch_stream`
//!   注册 `hongguo-stream` 渐进流，delegate 按 `(vid, 档位)` 绑定、按需解密供数。
//!
//! 在线模式用独立的 `hongguo-gpui-av-spike` 数据根（不碰用户档案）；设备档案
//! 走静态兜底，不需要登录。样片路径可用 `HONGGUO_AV_SPIKE_SAMPLE` 覆盖。
//!
//! 本模块只做 macos：`AVFoundation` 是系统框架，不引入任何外部 crate。

pub(crate) use hongguo_player::{avf, loader};
mod measure;

use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui_kit::div;
use gpui_kit::prelude::*;
use hongguo_api::detail::fetch_episode_list;
use hongguo_api::recommend::fetch_recommend_feed;
use hongguo_service::app_state::AppStateInner;
use hongguo_store::Store;
use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::{NSScreen, NSView};
use objc2_foundation::NSString;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

const ENABLE_ENV: &str = "HONGGUO_AV_SPIKE";
const SOURCE_ENV: &str = "HONGGUO_AV_SPIKE_SOURCE";
const SAMPLE_ENV: &str = "HONGGUO_AV_SPIKE_SAMPLE";
const DEFAULT_SAMPLE: &str = "target/av-spike/sample-fmp4.mp4";

/// 一集的供数来源 + 日志标签 + URL slug（只用于让 AVFoundation 不把两个
/// asset 当成同一个资源，delegate 不解析它）。
pub(crate) struct SpikeEpisode {
    pub label: String,
    pub slug: String,
    pub source: loader::Source,
}

pub(crate) fn requested() -> bool {
    std::env::var(ENABLE_ENV).is_ok_and(|value| value == "1")
}

pub(crate) fn run() {
    let online = std::env::var(SOURCE_ENV).is_ok_and(|value| value == "online");
    let episodes = if online {
        prepare_online()
    } else {
        prepare_file()
    };
    for episode in &episodes {
        log::info!(
            "[AvSpike] 数据源 {}：{}",
            episode.label,
            episode.source.label()
        );
    }

    let player = avf::player();
    let player_addr = Retained::as_ptr(&player) as usize;
    let layer = avf::player_layer(&player);
    // 图层强引用 player；泄漏我们这份 Retained 只是为了地址在整个进程内有效
    // （spike 一次性进程，退出即回收）。
    std::mem::forget(player);

    gpui_kit::application()
        .with_assets(gpui_kit::assets::AllAssets)
        .run(move |cx| {
            gpui_kit::init(cx);
            let refresh_hz = objc2::MainThreadMarker::new()
                .and_then(NSScreen::mainScreen)
                .map_or(100.0, |screen| screen.maximumFramesPerSecond() as f64);
            log::info!("[AvSpike] 主屏刷新率 {refresh_hz}Hz");

            let (window, _view) = gpui_kit::open_window(
                gpui_kit::WindowOptions {
                    window_bounds: Some(gpui_kit::WindowBounds::Windowed(
                        gpui_kit::Bounds::centered(
                            None,
                            gpui_kit::size(gpui_kit::px(1280.), gpui_kit::px(720.)),
                            cx,
                        ),
                    )),
                    ..Default::default()
                },
                cx,
                |_, cx| cx.new(|_| SpikeView),
            )
            .expect("spike 窗口创建失败");

            // 首帧之后再挂原生图层：GPUI 的 NSView 在首帧前可能还没有 layer。
            let layer_addr = Retained::as_ptr(&layer) as usize;
            let (attached_tx, attached_rx) = std::sync::mpsc::channel::<bool>();
            let layer_for_window = layer.clone();
            let _ = window.update(cx, |_, window, _| {
                window.on_next_frame(move |window, _| {
                    let attached = attach_layer(window, &layer_for_window);
                    let _ = attached_tx.send(attached);
                });
            });

            std::thread::Builder::new()
                .name("hongguo-av-spike".to_owned())
                .spawn(
                    move || match attached_rx.recv_timeout(Duration::from_secs(5)) {
                        Ok(true) => {
                            let [first, second] = episodes;
                            measure::run(player_addr, layer_addr, refresh_hz, first, second)
                        }
                        _ => {
                            log::error!("[AvSpike] AVPlayerLayer 未能挂到窗口，spike 中止");
                            std::process::exit(3);
                        }
                    },
                )
                .expect("spike 测量线程创建失败");
        });
}

/// Phase 0 的整段内存样片；A/B 用同一份字节，只换 URL slug。
fn prepare_file() -> [SpikeEpisode; 2] {
    let path = std::env::var(SAMPLE_ENV).unwrap_or_else(|_| DEFAULT_SAMPLE.to_owned());
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) => {
            log::error!("[AvSpike] 读不到样片 {path}: {error}（可用 {SAMPLE_ENV} 指定路径）");
            std::process::exit(2);
        }
    };
    log::info!("[AvSpike] 样片 {path}（{} 字节）", bytes.len());
    let bytes: Arc<[u8]> = bytes.into();
    [
        SpikeEpisode {
            label: "file-A".into(),
            slug: "file-a".into(),
            source: loader::Source::Bytes(Arc::clone(&bytes)),
        },
        SpikeEpisode {
            label: "file-B".into(),
            slug: "file-b".into(),
            source: loader::Source::Bytes(bytes),
        },
    ]
}

/// 真实在线剧集：推荐流取一集 → 详情页取下一集 → prepare A / prefetch B。
///
/// 每一步都显式失败退出：spike 要的是可复现的实测数据，不是「静默跑了但
/// 其实没供数」。数据根独立，不碰用户档案。
fn prepare_online() -> [SpikeEpisode; 2] {
    // 数据根落在工作区：沙箱/CI 下系统 Application Support 可能不可写，而
    // 在线链路只需要一个独立库（设备档案走静态兜底，不碰用户真实档案）。
    let data_dir = std::env::current_dir()
        .unwrap_or_default()
        .join("target/av-spike/data");
    if let Err(error) = std::fs::create_dir_all(&data_dir) {
        log::error!("[AvSpike] 创建 spike 数据目录失败: {error}");
        std::process::exit(4);
    }
    // SAFETY: 此刻还在单线程启动阶段（GPUI 未启动），没有并发读环境变量。
    unsafe { std::env::set_var("HONGGUO_DATA_DIR", &data_dir) };
    log::info!("[AvSpike] 在线模式数据根 {}", data_dir.display());
    let db = match Store::open(hongguo_store::paths::db_file()) {
        Ok(db) => db,
        Err(error) => {
            log::error!("[AvSpike] 打开 spike 数据根失败: {error}");
            std::process::exit(4);
        }
    };
    let (device, source) = hongguo_service::device::adopt_sync(&db);
    log::info!(
        "[AvSpike] 在线模式设备档案 iid={}（{source}）",
        device.get("iid")
    );
    let state = Arc::new(AppStateInner::with_device(db, device));
    let result = hongguo_service::runtime::runtime()
        .block_on(async move { prepare_online_inner(&state).await });
    match result {
        Ok(episodes) => episodes,
        Err(error) => {
            log::error!("[AvSpike] 在线剧集准备失败: {error}");
            std::process::exit(4);
        }
    }
}

async fn prepare_online_inner(
    state: &hongguo_service::app_state::AppState,
) -> Result<[SpikeEpisode; 2], String> {
    let env = state.api_env();
    let mut items = Vec::new();
    for tab in ["16", "36", "39"] {
        match fetch_recommend_feed(tab, "", 0, &[], &env).await {
            Ok(page) => {
                log::info!("[AvSpike] 推荐 tab={tab} 返回 {} 条", page.items.len());
                items.extend(page.items);
            }
            Err(error) => log::warn!("[AvSpike] 推荐 tab={tab} 失败: {error}"),
        }
    }
    // 三个 tab 会有重叠；按 vid 去重后取前 8 个候选。
    let mut seen = std::collections::HashSet::new();
    let feed_items: Vec<_> = items
        .into_iter()
        .filter(|item| !item.vid.is_empty() && !item.series_id.is_empty())
        .filter(|item| seen.insert(item.vid.clone()))
        .take(8)
        .collect();
    if feed_items.is_empty() {
        return Err("推荐流没有可用 vid".to_string());
    }

    // 短剧单集常只有 6-10MB，8MB 预取头部会把整集下完，渐进等待就压不到。
    // 先只探测总长（Range 0-0，不下载正片），挑最大的一集；探测本身不
    // 触发填充，正式 prepare 之后到开播之间才真正开始下 A。
    const PROGRESSIVE_STRESS_BYTES: u64 = 24 * 1024 * 1024;
    let mut probed: Vec<(hongguo_api::discover::FeedItem, u32, u64)> = Vec::new();
    for item in &feed_items {
        match probe_candidate(state, &env, &item.vid).await {
            Ok((definition, total)) => {
                log::info!(
                    "[AvSpike] 候选《{}》{}@{} plain_len={total}",
                    item.title,
                    item.vid,
                    definition
                );
                probed.push((item.clone(), definition, total));
                if total >= PROGRESSIVE_STRESS_BYTES {
                    break;
                }
            }
            Err(error) => log::warn!("[AvSpike] 候选《{}》探测失败: {error}", item.title),
        }
    }
    if probed.is_empty() {
        return Err("没有候选剧集探测成功".to_string());
    }
    let best = probed
        .iter()
        .enumerate()
        .max_by_key(|(_, (_, _, total))| *total)
        .map(|(index, _)| index)
        .expect("probed 非空");
    let (item, definition_a, total_a) = probed.swap_remove(best);
    if total_a < PROGRESSIVE_STRESS_BYTES {
        log::warn!(
            "[AvSpike] 所有候选都小于 24MB（最大 {total_a}B）：8MB 预取头部可能已覆盖全集，渐进等待未被压测"
        );
    }
    log::info!(
        "[AvSpike] 在线剧集《{}》series={} 推荐 vid={}",
        item.title,
        item.series_id,
        item.vid
    );

    // 同剧下一集优先；详情页失败或已是最后一集时用推荐流里的另一条。
    let mut next = None;
    if let Ok(list) = fetch_episode_list(&item.series_id, &env).await {
        let index = list
            .episodes
            .iter()
            .position(|episode| episode.vid == item.vid)
            .unwrap_or(0);
        let candidate = list
            .episodes
            .get(index + 1)
            .or_else(|| index.checked_sub(1).and_then(|i| list.episodes.get(i)));
        if let Some(episode) = candidate
            && episode.vid != item.vid
        {
            next = Some((episode.vid.clone(), format!("第{}集", episode.vid_index)));
        }
    }
    let (vid_b, next_label) = next
        .or_else(|| {
            feed_items
                .iter()
                .find(|other| other.vid != item.vid && !other.vid.is_empty())
                .map(|other| (other.vid.clone(), format!("《{}》", other.title)))
        })
        .ok_or_else(|| "找不到第二集可切".to_string())?;

    // 选中的 A 转正：命中缓存，resume_fill 续填（探测阶段可能只填了头部）。
    let prepared_a = hongguo_service::play_service::online::prepare(
        state,
        &item.vid,
        "av-spike-a",
        Some(definition_a),
    )
    .await
    .map_err(|error| format!("prepare A 失败: {error}"))?;
    log::info!(
        "[AvSpike] A {}@{} 渐进流就绪，plain_len={total_a}",
        item.vid,
        prepared_a.definition
    );

    // B：按生产语义走 prefetch_stream（mark_prefetch + 有界头部填充）；
    // 失败或 20s 内没注册就回落 prepare（此时把 A 标成预取，避免被逐出）。
    let mut definition_b = None;
    if let Err(error) =
        hongguo_service::play_service::online::prefetch_stream(state, &vid_b, "av-spike-b").await
    {
        log::warn!("[AvSpike] prefetch B 失败（转 prepare）: {error}");
    } else {
        definition_b = wait_ready_definition(&vid_b, Duration::from_secs(20)).await;
    }
    let definition_b = match definition_b {
        Some(definition) => definition,
        None => {
            log::warn!("[AvSpike] prefetch B 未就绪，回落 prepare（A 已标记预取保活）");
            hongguo_stream::cache().mark_prefetch(&item.vid, prepared_a.definition);
            hongguo_service::play_service::online::prepare(state, &vid_b, "av-spike-b", None)
                .await
                .map_err(|error| format!("prepare B 失败: {error}"))?
                .definition
        }
    };
    let total_b = wait_total(&vid_b, definition_b, Duration::from_secs(30))
        .await
        .ok_or_else(|| format!("B {vid_b} 30s 内未注册渐进流"))?;
    log::info!("[AvSpike] B {vid_b}@{definition_b} 渐进流就绪，plain_len={total_b}");

    // A 仍是当前集：填充优先保它；B 保持预取标记，切集时只做接力测量。
    hongguo_stream::cache().set_current(&item.vid);
    Ok([
        SpikeEpisode {
            label: format!("online-A《{}》第1集", item.title),
            slug: format!("a-{}-{}", item.vid, prepared_a.definition),
            source: loader::Source::Stream {
                vid: item.vid.clone(),
                definition: prepared_a.definition,
                on_stall: None,
            },
        },
        SpikeEpisode {
            label: format!("online-B{next_label}"),
            slug: format!("b-{vid_b}-{definition_b}"),
            source: loader::Source::Stream {
                vid: vid_b,
                definition: definition_b,
                on_stall: None,
            },
        },
    ])
}

/// 只探长度：取流表 + `Range: bytes=0-0`，不触发正片填充。
async fn probe_candidate(
    state: &hongguo_service::app_state::AppState,
    env: &hongguo_api::client::ApiEnv,
    vid: &str,
) -> Result<(u32, u64), String> {
    let capability = hongguo_api::stream_pick::DecoderCapability::from_hevc(
        hongguo_media_platform::capability::playback::hevc_playable(),
    );
    let play = hongguo_api::play_url::fetch_play_url(vid, None, capability, env)
        .await
        .map_err(|error| format!("取流表失败: {error}"))?;
    let client = hongguo_api::client::build_client(&state.settings().proxy)
        .map_err(|error| format!("建 client 失败: {error}"))?;
    let total = hongguo_api::client::probe_video_len(&client, &play.url)
        .await
        .map_err(|error| format!("探测长度失败: {error}"))?;
    Ok((play.definition, total))
}

async fn wait_total(vid: &str, definition: u32, timeout: Duration) -> Option<u64> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(total) = hongguo_stream::total_len(vid, definition) {
            return Some(total);
        }
        if Instant::now() >= deadline {
            return None;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn wait_ready_definition(vid: &str, timeout: Duration) -> Option<u32> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some((definition, _)) = hongguo_stream::cache().ready_any(vid) {
            return Some(definition);
        }
        if Instant::now() >= deadline {
            return None;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

struct SpikeView;

impl gpui_kit::Render for SpikeView {
    fn render(
        &mut self,
        _window: &mut gpui_kit::Window,
        _cx: &mut gpui_kit::Context<Self>,
    ) -> impl gpui_kit::IntoElement {
        div()
            .size_full()
            .bg(gpui_kit::rgb(0x000000))
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .text_color(gpui_kit::rgb(0xffffff))
                    .child("HONGGUO_AV_SPIKE=1 — AVPlayer / AVPlayerLayer"),
            )
    }
}

/// 把 `AVPlayerLayer` 作为子层挂到 GPUI 窗口的 `NSView` 上。
///
/// 用 `msg_send!` 而不是 typed binding：`CALayer` 属于 objc2-quartz-core，
/// 本 crate 目前没有这个依赖，原样保留「零新依赖」约束。
fn attach_layer(window: &gpui_kit::Window, layer: &AnyObject) -> bool {
    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        log::error!("[AvSpike] 取不到窗口句柄");
        return false;
    };
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        log::error!("[AvSpike] 窗口句柄不是 AppKit");
        return false;
    };
    // SAFETY: gpui-pre-macos 的 AppKitWindowHandle.ns_view 是窗口自身持有的
    // GPUIView；本函数在 gpui 主线程回调里执行。
    let view = unsafe { &*(handle.ns_view.as_ptr() as *const NSView) };
    let mut view_layer: *mut AnyObject = unsafe { msg_send![view, layer] };
    if view_layer.is_null() {
        let _: () = unsafe { msg_send![view, setWantsLayer: true] };
        view_layer = unsafe { msg_send![view, layer] };
    }
    if view_layer.is_null() {
        log::error!("[AvSpike] GPUI 的 NSView 没有 layer");
        return false;
    }

    let bounds = view.bounds();
    let gravity = NSString::from_str("AVLayerVideoGravityResizeAspect");
    // SAFETY: layer/view_layer 都是有效 Objective-C 对象；frame 用 AppKit 的
    // NSRect（CGRect 布局），autoresizingMask 18 = Width|Height。
    unsafe {
        let _: () = msg_send![layer, setFrame: bounds];
        let _: () = msg_send![layer, setAutoresizingMask: 18u32];
        let _: () = msg_send![layer, setZPosition: 1.0f64];
        let _: () = msg_send![layer, setVideoGravity: &*gravity];
        let _: () = msg_send![view_layer, addSublayer: layer];
    }
    log::info!(
        "[AvSpike] AVPlayerLayer 已挂到窗口（{:.0}×{:.0}pt）",
        bounds.size.width,
        bounds.size.height
    );
    true
}
