//! macOS 系统播放器：AVPlayer + AVPlayerLayer 的 [`PlayerLike`] / [`VideoPresenter`] 实现。
//!
//! AVPlayer 在 tokio 线程创建（系统播放器不要求主线程），图层在 GPUI render
//! 主线程挂到真实窗口（`view.rs`）。事件线程只做状态桥（首帧/缓冲/播完/失败），
//! 画面由 AVPlayerLayer 自己合成，不经过 GPUI 的帧通道。

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use hongguo_player_api::{
    HostWindow, PlaySource, PlayerEvent, PlayerLike, PresenterBounds, PresenterFit, VideoPresenter,
};
use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_foundation::{NSArray, NSString, NSURL};
use tokio::sync::mpsc;

use super::{avf, loader, view};

/// 事件线程轮询间隔（缓冲/播完/失败都从这里出）。
const EVENT_TICK: Duration = Duration::from_millis(100);

struct SendRetained(Retained<AnyObject>);

// SAFETY: AVPlayer/AVPlayerItem 的播放控制允许跨线程；对象由 Arc<SystemPlayer>
// 持有，事件线程只读状态。图层操作另有主线程约束（view.rs）。
unsafe impl Send for SendRetained {}
unsafe impl Sync for SendRetained {}

impl SendRetained {
    fn get(&self) -> &AnyObject {
        &self.0
    }

    fn clone_ref(&self) -> Self {
        Self(self.0.clone())
    }
}

pub struct SystemPlayer {
    player: SendRetained,
    /// delegate 是 weak：句柄必须活到这一集播放结束。
    _loader: Mutex<Option<loader::LoaderHandle>>,
    stop: Arc<AtomicBool>,
    event_thread: Mutex<Option<JoinHandle<()>>>,
    paused: AtomicBool,
    rate: AtomicU64,
    stage: Mutex<(Option<PresenterBounds>, PresenterFit)>,
    /// 已预加载的下一集 (vid, 档位)；AVQueuePlayer 的下一项。
    preloaded: Mutex<Option<(String, u32)>>,
    /// 预加载项的 delegate 句柄（weak delegate 必须保活）。
    next_loaders: Mutex<Vec<loader::LoaderHandle>>,
}

impl SystemPlayer {
    fn spawn_events(&self, events: mpsc::Sender<PlayerEvent>) {
        let player = self.player.clone_ref();
        let stop = Arc::clone(&self.stop);
        let thread = std::thread::Builder::new()
            .name("hongguo-av-events".to_owned())
            .spawn(move || {
                let player = player.get();
                // 等 item ready（最多 15s；失败/取消提前退出）。
                let deadline = Instant::now() + Duration::from_secs(15);
                loop {
                    if stop.load(Ordering::SeqCst) {
                        return;
                    }
                    let Some(item) = avf::current_item(player) else {
                        if Instant::now() >= deadline {
                            let _ = events.blocking_send(PlayerEvent::Failed(
                                "AVPlayerItem 未就绪（15s 超时）".into(),
                            ));
                            return;
                        }
                        std::thread::sleep(EVENT_TICK);
                        continue;
                    };
                    match avf::item_status(item) {
                        1 => break,
                        2 => {
                            let message = avf::item_error(item)
                                .unwrap_or_else(|| "AVPlayerItem 加载失败".into());
                            let _ = events.blocking_send(PlayerEvent::Failed(message));
                            return;
                        }
                        _ => {
                            if Instant::now() >= deadline {
                                let _ = events.blocking_send(PlayerEvent::Failed(
                                    "AVPlayerItem 未就绪（15s 超时）".into(),
                                ));
                                return;
                            }
                            std::thread::sleep(EVENT_TICK);
                        }
                    }
                }

                let mut current_ptr = 0usize;
                let mut buffering = false;
                let mut ended = false;
                loop {
                    if stop.load(Ordering::SeqCst) {
                        return;
                    }
                    let Some(item) = avf::current_item(player) else {
                        std::thread::sleep(EVENT_TICK);
                        continue;
                    };
                    let ptr = item as *const AnyObject as usize;
                    if ptr != current_ptr {
                        current_ptr = ptr;
                        let (width, height) = avf::item_presentation_size(item);
                        log::info!("[AvPresenter] 新 item ready，发 FirstFrame {width}×{height}");
                        let _ = events.blocking_send(PlayerEvent::FirstFrame { width, height });
                        let _ = events.blocking_send(PlayerEvent::Ready);
                        buffering = false;
                        ended = false;
                    }
                    std::thread::sleep(EVENT_TICK);
                    if avf::item_status(item) == 2 {
                        let message =
                            avf::item_error(item).unwrap_or_else(|| "AVPlayerItem 播放失败".into());
                        let _ = events.blocking_send(PlayerEvent::Failed(message));
                        return;
                    }
                    let keep_up = avf::item_likely_to_keep_up(item);
                    if !keep_up && !buffering {
                        buffering = true;
                        let _ = events.blocking_send(PlayerEvent::Buffering);
                    } else if keep_up && buffering {
                        buffering = false;
                        let _ = events.blocking_send(PlayerEvent::Ready);
                    }
                    let duration = avf::item_duration(item).seconds();
                    let position = avf::current_time(player).seconds();
                    if !ended
                        && duration > 0.0
                        && position >= duration - 0.25
                        && avf::player_rate(player) > 0.0
                    {
                        ended = true;
                        let _ = events.blocking_send(PlayerEvent::Ended);
                    }
                }
            })
            .expect("AVPlayer 事件线程创建失败");
        *self.event_thread.lock().expect("事件线程锁 poisoned") = Some(thread);
    }
}

/// 2ms 轮询 AVPlayerItemVideoOutput，5s 汇总一次产帧节奏（仅取证）。
fn spawn_metrics(player: SendRetained, output: SendRetained, stop: Arc<AtomicBool>) {
    std::thread::Builder::new()
        .name("hongguo-av-metrics".to_owned())
        .spawn(move || {
            let player = player.get();
            let output = output.get();
            let mut last: Option<avf::CmTime> = None;
            let mut samples: Vec<(Instant, f64)> = Vec::new();
            let mut window = Instant::now();
            loop {
                if stop.load(Ordering::SeqCst) {
                    return;
                }
                let now = avf::current_time(player);
                if avf::has_new_frame(output, now)
                    && let Some(display) = avf::frame_display_time(output, now)
                    && Some(display) != last
                {
                    last = Some(display);
                    samples.push((Instant::now(), display.seconds()));
                }
                std::thread::sleep(Duration::from_millis(2));
                if window.elapsed() >= Duration::from_secs(5) && samples.len() > 3 {
                    let first = samples.first().unwrap().0;
                    let elapsed = samples.last().unwrap().0.duration_since(first).as_secs_f64();
                    let fps = (samples.len() - 1) as f64 / elapsed.max(0.001);
                    let mut gaps: Vec<f64> = samples
                        .windows(2)
                        .map(|pair| pair[1].0.duration_since(pair[0].0).as_secs_f64())
                        .collect();
                    gaps.sort_by(|a, b| a.partial_cmp(b).unwrap());
                    let max_gap = gaps.last().copied().unwrap_or(0.0);
                    log::info!(
                        "[AvPresenter][metrics] 5s frames={} presented={fps:.1}fps max_gap={:.1}ms gap_med={:.1}ms rate={:.2}",
                        samples.len(),
                        max_gap * 1000.0,
                        gaps[gaps.len() / 2] * 1000.0,
                        avf::player_rate(player)
                    );
                    samples.clear();
                    window = Instant::now();
                }
            }
        })
        .expect("metrics 线程创建失败");
}

/// 打开一集系统播放器。返回的门面已实现 [`PlayerLike`]；图层由 render 侧
/// 调用 `attach` 挂载（tokio 线程不能碰 AppKit）。
///
/// 返回 `None` = 引擎起不来，调用方回落到自研上屏——与 Windows 实现同一套
/// 契约（门面 `hongguo_player::open` 的返回类型由两边实现共同决定）。
pub fn open(
    source: PlaySource,
    start_secs: f64,
    rate: f64,
    events: mpsc::Sender<PlayerEvent>,
    on_stall: Option<loader::StallHook>,
) -> Option<Arc<dyn PlayerLike>> {
    let (asset, loader_handle) = match source {
        PlaySource::Online { vid, definition } => {
            let url = avf::custom_url(&format!("{vid}-{definition}")).expect("自定义 scheme URL");
            let asset = avf::asset_with_url(&url);
            let handle = loader::attach(
                &asset,
                loader::Source::Stream {
                    vid: vid.clone(),
                    definition,
                    on_stall,
                },
            );
            (asset, Some(handle))
        }
        PlaySource::Local { path } => {
            let path = path.to_string_lossy().to_string();
            let url = NSURL::fileURLWithPath(&NSString::from_str(&path));
            (avf::asset_with_url(&url), None)
        }
    };
    let item = avf::item_with_asset(&asset);
    avf::set_spectral_pitch(&item);
    // 取证开关：附加视频输出，在系统后端的真实壳里量 2x 产帧节奏。
    let metrics = std::env::var("HONGGUO_AV_METRICS").is_ok_and(|value| value == "1");
    let metrics_output = metrics.then(|| {
        let output = avf::video_output();
        avf::add_output(&item, &output);
        output
    });
    let items = NSArray::from_slice(&[&*item]);
    let player = avf::queue_player(&items);
    avf::disable_auto_advance(&player);
    avf::set_volume(&player, 1.0);
    if start_secs > 0.5 {
        avf::seek_exact(&player, avf::CmTime::secs(start_secs));
    }
    avf::play(&player);
    if (rate - 1.0).abs() > 1e-9 {
        avf::set_rate(&player, rate as f32);
    }
    let metrics_player = player.clone();
    let system = Arc::new(SystemPlayer {
        player: SendRetained(player),
        _loader: Mutex::new(loader_handle),
        stop: Arc::new(AtomicBool::new(false)),
        event_thread: Mutex::new(None),
        paused: AtomicBool::new(false),
        rate: AtomicU64::new(rate.to_bits()),
        stage: Mutex::new((None, PresenterFit::Contain)),
        preloaded: Mutex::new(None),
        next_loaders: Mutex::new(Vec::new()),
    });
    system.spawn_events(events);
    if let Some(output) = metrics_output {
        spawn_metrics(
            SendRetained(metrics_player),
            SendRetained(output),
            Arc::clone(&system.stop),
        );
    }
    log::info!("[AvPresenter] 系统播放器已打开（起点 {start_secs:.1}s，{rate:.2}x）");
    Some(system)
}

impl PlayerLike for SystemPlayer {
    fn pause(&self) {
        self.paused.store(true, Ordering::SeqCst);
        avf::pause(self.player.get());
    }

    fn resume(&self) {
        self.paused.store(false, Ordering::SeqCst);
        avf::play(self.player.get());
        let rate = f64::from_bits(self.rate.load(Ordering::Relaxed));
        if (rate - 1.0).abs() > 1e-9 {
            avf::set_rate(self.player.get(), rate as f32);
        }
    }

    fn is_paused(&self) -> bool {
        self.paused.load(Ordering::SeqCst)
    }

    fn position_secs(&self) -> f64 {
        avf::current_time(self.player.get()).seconds()
    }

    fn duration_secs(&self) -> f64 {
        avf::current_item(self.player.get())
            .map(|item| avf::item_duration(item).seconds())
            .unwrap_or(0.0)
    }

    fn seek(&self, seconds: f64) {
        avf::seek_exact(self.player.get(), avf::CmTime::secs(seconds.max(0.0)));
    }

    fn set_rate(&self, rate: f64) {
        self.rate.store(rate.to_bits(), Ordering::Relaxed);
        avf::set_rate(self.player.get(), rate as f32);
    }

    fn rate(&self) -> f64 {
        f64::from_bits(self.rate.load(Ordering::Relaxed))
    }

    fn set_volume(&self, volume: f32) {
        avf::set_volume(self.player.get(), volume.clamp(0.0, 1.0));
    }

    fn set_muted(&self, muted: bool) {
        avf::set_muted(self.player.get(), muted);
    }

    fn dims(&self) -> (usize, usize) {
        avf::current_item(self.player.get())
            .map(avf::item_presentation_size)
            .unwrap_or((0, 0))
    }

    fn preload_next(
        &self,
        vid: &str,
        definition: u32,
        on_stall: Option<loader::StallHook>,
    ) -> bool {
        let mut preloaded = self.preloaded.lock().expect("预加载锁 poisoned");
        if preloaded
            .as_ref()
            .is_some_and(|(current, def)| current == vid && *def == definition)
        {
            return true;
        }
        if preloaded.is_some() {
            // 只维护一个下一项；旧预加载留在队列里会造成切集错位。
            return false;
        }
        let url = avf::custom_url(&format!("{vid}-{definition}")).expect("下一集 URL");
        let asset = avf::asset_with_url(&url);
        let handle = loader::attach(
            &asset,
            loader::Source::Stream {
                vid: vid.to_string(),
                definition,
                // 切到这一集后由同一 loader 在 8MB 处继续唤醒续填；
                // 未切之前 resume_fill 也会因 is_next 放行。
                on_stall,
            },
        );
        let item = avf::item_with_asset(&asset);
        avf::set_spectral_pitch(&item);
        // SAFETY: nil 表示追加到队尾；item 由队列持有。
        let nil: *mut AnyObject = std::ptr::null_mut();
        let _: () = unsafe { msg_send![self.player.get(), insertItem: &*item, afterItem: nil] };
        *preloaded = Some((vid.to_string(), definition));
        self.next_loaders
            .lock()
            .expect("预加载句柄锁 poisoned")
            .push(handle);
        log::info!("[AvPresenter] 预加载下一集 {vid}@{definition}");
        true
    }

    fn advance_preloaded(&self, vid: &str) -> bool {
        let mut slot = self.preloaded.lock().expect("预加载锁 poisoned");
        match slot.as_ref() {
            Some((preloaded_vid, _)) if preloaded_vid == vid => {
                slot.take();
            }
            _ => return false,
        }
        drop(slot);
        avf::advance_to_next_item(self.player.get());
        log::info!("[AvPresenter] advanceToNextItem 接力（{vid}）");
        true
    }

    fn attach(&self, host: &HostWindow) {
        // 只建/复用宿主；可见性由紧随其后的 submit_stage 决定，避免 attach
        // 先把层亮出来、下一行又按 system_ready=false 藏回去的抖动。
        let _ = view::ensure(host, self.player.get());
    }

    fn submit_stage(&self, bounds: Option<PresenterBounds>, fit: PresenterFit, visible: bool) {
        *self.stage.lock().expect("舞台锁 poisoned") = (bounds, fit);
        view::submit(bounds, fit, visible);
    }

    fn is_system(&self) -> bool {
        true
    }

    fn detach(&self) {
        view::hide();
    }
}

impl VideoPresenter for SystemPlayer {
    fn submit(&self, bounds: Option<PresenterBounds>, fit: PresenterFit) {
        self.submit_stage(bounds, fit, true);
    }

    fn set_visible(&self, visible: bool) {
        let (bounds, fit) = *self.stage.lock().expect("舞台锁 poisoned");
        view::submit(bounds, fit, visible);
    }

    fn play(&self) {
        self.resume();
    }

    fn pause(&self) {
        PlayerLike::pause(self);
    }

    fn seek(&self, seconds: f64) {
        PlayerLike::seek(self, seconds);
    }

    fn set_rate(&self, rate: f64) {
        PlayerLike::set_rate(self, rate);
    }

    fn flush(&self) {
        PlayerLike::pause(self);
    }
}

impl Drop for SystemPlayer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        avf::pause(self.player.get());
        if let Some(thread) = self
            .event_thread
            .lock()
            .expect("事件线程锁 poisoned")
            .take()
        {
            let _ = thread.join();
        }
    }
}
