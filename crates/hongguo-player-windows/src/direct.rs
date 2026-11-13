//! 内置软解直播放（Windows，GPUI 壳）：HEVC / ByteVC1 / ByteVC2 都由进程内解码器解，
//! 不经过系统 HEVC 解码器（不需要「HEVC 视频扩展」），也不转码。
//!
//! 分工（与 macOS 的 `direct.rs` 对位）：
//! - **声音与时钟**：一个只放声音的 MediaEngine（[`player::open_audio`]），喂的是遮掉视频轨的
//!   字节流（[`stream::AudioMask`]）。暂停、倍速、跳转、播完、缓冲都沿用它的实现与事件；
//!   它的 `GetCurrentTime` 就是画面的时钟。
//! - **画面**：解码线程把帧换成 BGRA 放进一个小队列（满了就等，即背压）；渲染泵（壳每帧经
//!   `submit_stage` 进来，vsync 节奏）取出「到点」的那一帧，`UpdateSubresource` 写进视频 quad 的
//!   纹理，视频 quad 与 GPUI 内容使用同一画布。
//! - **扣住声音**：起播与跳转后先不让引擎走，新会话的第一帧上屏后再放行，避免画面一出来就落后。
//!
//! Windows 默认使用本路径解码 HEVC / ByteVC1 / ByteVC2；运行验收仍需 Windows 实机。
//!
//! 线程：构造与 [`PlayerLike`] 调用在 GPUI 主线程（引擎是 STA 上建的）；建索引在 `softdec-open`
//! 线程；解码在 `softdec-direct` 线程。解码线程不碰引擎与 D3D 对象，只读渲染泵发布的时钟。

use std::collections::VecDeque;
use std::fs::File;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use hongguo_common::{AppError, AppResult};
use hongguo_player_api::{
    HostWindow, PlaySource, PlayerEvent, PlayerLike, PresenterBounds, PresenterFit, StallHook,
};
use hongguo_softdec::{IndexedVideo, bgra_len, copy_to_bgra};
use tokio::sync::mpsc;
use windows::Win32::Graphics::Direct3D11::{ID3D11DeviceContext, ID3D11Texture2D};

use super::player::{self, SystemPlayer, geometry};
use super::stream::{self, AudioMask, Feed};

/// 解码领先渲染最多几帧。BGRA 一帧 1080P 约 8MB，队列深度直接是内存占用。
const QUEUE_DEPTH: usize = 4;
/// 队列满时多久再看一次。
const READY_POLL: Duration = Duration::from_millis(4);
/// 帧比时钟晚多少秒就不再上屏（与 macOS 同参）。
const LATE_SECS: f64 = 0.1;
/// 渲染泵判断「到点」时给时钟的提前量：泵按 vsync 进来，帧到点后最晚一个刷新周期才被取走。
const DUE_AHEAD_SECS: f64 = 0.008;

/// 每集一个音轨 URL，日志里分得清。
static NEXT_ASSET: AtomicU64 = AtomicU64::new(1);

/// 一帧待上屏的画面。
struct Picture {
    pts: f64,
    width: usize,
    height: usize,
    bgra: Vec<u8>,
    /// 本会话的第一帧：不等时钟，立即上屏（声音正扣着等它）。
    first: bool,
}

struct Queue {
    /// 当前解码会话的代次。跳转时加一并清空队列；旧会话拿锁后一看代次不对就退出。
    generation: u64,
    pictures: VecDeque<Picture>,
    /// 用过的 BGRA 缓冲（复用，省掉每帧几 MB 的分配）。
    spare: Vec<Vec<u8>>,
}

impl Queue {
    fn recycle(&mut self, bgra: Vec<u8>) {
        if self.spare.len() < QUEUE_DEPTH + 2 {
            self.spare.push(bgra);
        }
    }
}

struct Shared {
    feed: Feed,
    mask: Arc<AudioMask>,
    index: Mutex<Option<Arc<IndexedVideo>>>,
    queue: Mutex<Queue>,
    /// 当前解码会话的取消位。
    session: Mutex<Arc<AtomicBool>>,
    /// 最近一次要求的起点（秒）。索引还没建好时的跳转记在这里，建好后从这里起解。
    target: Mutex<f64>,
    stop: AtomicBool,
    /// 渲染泵发布的时钟（秒，f64 位）。解码线程据此丢掉已经过时的帧。
    clock: AtomicU64,
    /// 画面尺寸，高 32 位宽、低 32 位高。
    dims: AtomicU64,
    first_frame_sent: AtomicBool,
    events: mpsc::Sender<PlayerEvent>,
    /// 声音在等画面（见模块注释）。
    hold: AtomicBool,
}

/// 视频 quad 上的帧纹理：按（设备代际, 尺寸）失效重建。
struct Texture {
    texture: ID3D11Texture2D,
    context: ID3D11DeviceContext,
    size: (u32, u32),
    generation: u64,
    /// 纹理里已经有画面（空纹理不发给视频位，免得闪黑）。
    has_frame: bool,
}

#[derive(Clone, Copy)]
struct Stage {
    bounds: Option<PresenterBounds>,
    fit: PresenterFit,
    visible: bool,
}

pub struct DirectPlayer {
    audio: Arc<SystemPlayer>,
    shared: Arc<Shared>,
    stage: Mutex<Stage>,
    texture: Mutex<Option<Texture>>,
    /// 落点是否已提交给视频 quad（可见性翻转时清帧用）。
    presented: AtomicBool,
}

// SAFETY: 与 `SystemPlayer` 同一套约定——D3D 纹理与设备上下文只在 GPUI 主线程（渲染泵）使用，
// 设备开了多线程保护；`Drop` 在任意线程只做 Release。
unsafe impl Send for DirectPlayer {}
unsafe impl Sync for DirectPlayer {}

/// 打开一集内置软解直播放。返回 `None` = 引擎或线程起不来。
pub fn open(
    source: PlaySource,
    start_secs: f64,
    rate: f64,
    events: mpsc::Sender<PlayerEvent>,
    on_stall: Option<StallHook>,
) -> Option<Arc<dyn PlayerLike>> {
    let feed = match source {
        PlaySource::Online { vid, definition } => Feed::Online {
            vid,
            definition,
            on_stall,
        },
        PlaySource::Local { path } => Feed::File(path),
    };
    let label = feed.label();
    let start_secs = start_secs.max(0.0);
    let mask = Arc::new(AudioMask::default());
    // 引擎在主线程建好、立刻挂上音轨视图；视图的读会等索引（起播线程建好后放行）。
    let byte_stream = stream::open_feed(feed.clone(), Arc::clone(&mask));
    let id = NEXT_ASSET.fetch_add(1, Ordering::Relaxed);
    let audio = player::open_audio(
        byte_stream,
        &format!("direct-{id}"),
        start_secs,
        rate,
        events.clone(),
    )?;
    let shared = Arc::new(Shared {
        feed,
        mask,
        index: Mutex::new(None),
        queue: Mutex::new(Queue {
            generation: 0,
            pictures: VecDeque::new(),
            spare: Vec::new(),
        }),
        session: Mutex::new(Arc::new(AtomicBool::new(true))),
        target: Mutex::new(start_secs),
        stop: AtomicBool::new(false),
        clock: AtomicU64::new(start_secs.to_bits()),
        dims: AtomicU64::new(0),
        first_frame_sent: AtomicBool::new(false),
        events,
        hold: AtomicBool::new(true),
    });
    let worker = Arc::clone(&shared);
    let spawned = std::thread::Builder::new()
        .name("softdec-open".into())
        .spawn(move || {
            let started = Instant::now();
            match prepare(&worker) {
                Ok(()) => log::info!(
                    "[WmDirect] {} 索引就绪 {}ms",
                    worker.feed.label(),
                    started.elapsed().as_millis()
                ),
                Err(_) if worker.stop.load(Ordering::SeqCst) => worker.mask.fail(),
                Err(error) => {
                    worker.mask.fail();
                    log::warn!("[WmDirect] {} 起播失败: {error}", worker.feed.label());
                    let _ = worker
                        .events
                        .blocking_send(PlayerEvent::Failed(error.to_string()));
                }
            }
        });
    if let Err(error) = spawned {
        shared.mask.fail();
        log::error!("[WmDirect] 起播线程起不来: {error}");
        return None;
    }
    log::info!("[WmDirect] 内置软解直播放已打开 {label}（起点 {start_secs:.1}s，{rate:.2}x）");
    Some(Arc::new(DirectPlayer {
        audio,
        shared,
        stage: Mutex::new(Stage {
            bounds: None,
            fit: PresenterFit::Contain,
            visible: false,
        }),
        texture: Mutex::new(None),
        presented: AtomicBool::new(false),
    }))
}

/// 解码侧读原始字节（不遮视频轨）。
fn read(
    feed: &Feed,
    file: &mut Option<File>,
    offset: u64,
    length: u64,
    cancelled: &AtomicBool,
    bytes: &mut Vec<u8>,
) -> AppResult<()> {
    if cancelled.load(Ordering::SeqCst) {
        return Err(AppError::Cancelled);
    }
    stream::read_exact(feed, file, offset, length, cancelled, bytes).map_err(|message| {
        if cancelled.load(Ordering::SeqCst) {
            AppError::Cancelled
        } else {
            AppError::Media(message)
        }
    })
}

/// 建索引、放行音轨视图、开第一个解码会话。在 `softdec-open` 线程上跑。
fn prepare(shared: &Arc<Shared>) -> AppResult<()> {
    let length = stream::total_bytes(&shared.feed)
        .ok_or_else(|| AppError::Media("拿不到这一集的总长度".into()))?;
    let mut file = None;
    let index = Arc::new(IndexedVideo::open(length, |offset, size, bytes| {
        read(&shared.feed, &mut file, offset, size, &shared.stop, bytes)
    })?);
    if shared.stop.load(Ordering::SeqCst) {
        return Err(AppError::Cancelled);
    }
    shared.mask.ready(Arc::clone(&index));
    *shared.index.lock().expect("索引锁 poisoned") = Some(Arc::clone(&index));
    log::info!(
        "[WmDirect] {} codec={:?} 源色彩 {:?}",
        shared.feed.label(),
        index.codec(),
        index.color()
    );
    let target = *shared.target.lock().expect("起点锁 poisoned");
    start_session(shared, target);
    Ok(())
}

/// 作废当前解码会话、清空待上屏队列，从 `target` 起开一个新会话。索引还没建好时什么都不做。
/// 纹理里的旧画面保留：跳转期间停在旧画面上，比闪一下黑好。
fn start_session(shared: &Arc<Shared>, target: f64) {
    let Some(index) = shared.index.lock().expect("索引锁 poisoned").clone() else {
        return;
    };
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut session = shared.session.lock().expect("会话锁 poisoned");
        session.store(true, Ordering::SeqCst);
        *session = Arc::clone(&cancel);
    }
    let generation = {
        let mut queue = shared.queue.lock().expect("队列锁 poisoned");
        queue.generation += 1;
        while let Some(picture) = queue.pictures.pop_front() {
            queue.recycle(picture.bgra);
        }
        queue.generation
    };
    let session_shared = Arc::clone(shared);
    let spawned = std::thread::Builder::new()
        .name("softdec-direct".into())
        .spawn(move || decode_session(&session_shared, &index, target, generation, &cancel));
    if let Err(error) = spawned {
        log::error!("[WmDirect] 解码线程起不来: {error}");
        let _ = shared
            .events
            .try_send(PlayerEvent::Failed(format!("解码线程起不来: {error}")));
    }
}

fn decode_session(
    shared: &Shared,
    index: &IndexedVideo,
    target: f64,
    generation: u64,
    cancel: &AtomicBool,
) {
    let started = Instant::now();
    let cancelled = || cancel.load(Ordering::SeqCst) || shared.stop.load(Ordering::SeqCst);
    let color = index.color();
    let mut file = None;
    // 跳转后时钟要过一会儿才落到新位置；落地前帧的「早晚」没有意义，一律上屏。
    let mut landed = false;
    let (mut queued, mut dropped) = (0u64, 0u64);
    let mut worst_lag = 0f64;
    // 取证（HONGGUO_AV_METRICS=1，与 macOS 直播放同一开关）。
    let metrics = std::env::var("HONGGUO_AV_METRICS").is_ok_and(|value| value == "1");
    let mut window = (Instant::now(), queued, dropped, 0f64);
    let result = index.decode_from(
        target,
        |offset, size, bytes| read(&shared.feed, &mut file, offset, size, cancel, bytes),
        |frame| {
            if cancelled() {
                return Err(AppError::Cancelled);
            }
            let clock = f64::from_bits(shared.clock.load(Ordering::SeqCst));
            if metrics && window.0.elapsed() >= Duration::from_secs(5) {
                let secs = window.0.elapsed().as_secs_f64();
                log::info!(
                    "[WmDirect][metrics] session={generation} {}x{} 5s 入队 {:.1}fps 丢弃 {} 最大落后 {:.0}ms 时钟 {clock:.3}s 帧 {:.3}s",
                    frame.width,
                    frame.height,
                    (queued - window.1) as f64 / secs,
                    dropped - window.2,
                    window.3 * 1000.0,
                    frame.pts.unwrap_or(f64::NAN)
                );
                window = (Instant::now(), queued, dropped, 0.0);
            }
            let pts = frame.pts.unwrap_or(target);
            // 背压：队列满了等渲染泵取走。
            let mut bgra = loop {
                {
                    let mut queue = shared.queue.lock().expect("队列锁 poisoned");
                    if queue.generation != generation {
                        return Err(AppError::Cancelled);
                    }
                    if queue.pictures.len() < QUEUE_DEPTH {
                        break queue.spare.pop().unwrap_or_default();
                    }
                }
                if cancelled() {
                    return Err(AppError::Cancelled);
                }
                std::thread::sleep(READY_POLL);
            };
            let clock = f64::from_bits(shared.clock.load(Ordering::SeqCst)).max(clock);
            landed = landed || (clock - target).abs() < 1.0;
            if landed && queued > 0 && pts + LATE_SECS < clock {
                dropped += 1;
                shared.queue.lock().expect("队列锁 poisoned").recycle(bgra);
                return Ok(());
            }
            if landed {
                worst_lag = worst_lag.max(clock - pts);
                window.3 = window.3.max(clock - pts);
            }
            let (width, height) = (frame.width, frame.height);
            let stride = width * 4;
            bgra.resize(bgra_len(width, height, stride), 0);
            if !copy_to_bgra(&frame, color, &mut bgra, stride) {
                return Err(AppError::Media("解码帧装不进显示缓冲".into()));
            }
            {
                let mut queue = shared.queue.lock().expect("队列锁 poisoned");
                if queue.generation != generation {
                    queue.recycle(bgra);
                    return Err(AppError::Cancelled);
                }
                queue.pictures.push_back(Picture {
                    pts,
                    width,
                    height,
                    bgra,
                    first: queued == 0,
                });
            }
            queued += 1;
            if queued == 1 {
                shared
                    .dims
                    .store(((width as u64) << 32) | height as u64, Ordering::Relaxed);
                // 这行是「这一集确实走了内置软解」的可观察证据。
                log::info!(
                    "[WmDirect] session={generation} codec={:?} {width}x{height} target={target:.3}s pts={pts:.3}s decode_ms={}",
                    index.codec(),
                    started.elapsed().as_millis()
                );
                if !shared.first_frame_sent.swap(true, Ordering::SeqCst) {
                    let _ = shared
                        .events
                        .blocking_send(PlayerEvent::FirstFrame { width, height });
                    let _ = shared.events.blocking_send(PlayerEvent::Ready);
                }
            }
            Ok(())
        },
    );
    match result {
        Ok(()) | Err(AppError::Cancelled) => {}
        Err(_) if cancelled() => {}
        Err(error) => {
            log::warn!("[WmDirect] session={generation} 解码失败: {error}");
            let _ = shared
                .events
                .blocking_send(PlayerEvent::Failed(error.to_string()));
        }
    }
    log::info!(
        "[WmDirect] session={generation} 结束：入队 {queued} 帧，丢弃 {dropped} 帧，最大落后 {:.0}ms，{}ms",
        worst_lag * 1000.0,
        started.elapsed().as_millis()
    );
}

impl DirectPlayer {
    /// 渲染泵：壳每帧经 `submit_stage` 进来一次。发布时钟 → 取到点的帧写进纹理 →
    /// 首帧上屏就放行声音 → 按舞台几何把纹理落点交给视频 quad。
    fn pump(&self) {
        self.audio.try_apply_rate();
        let clock = PlayerLike::position_secs(&*self.audio);
        if clock.is_finite() && !self.shared.hold.load(Ordering::SeqCst) {
            self.shared.clock.store(clock.to_bits(), Ordering::SeqCst);
        }
        let clock = f64::from_bits(self.shared.clock.load(Ordering::SeqCst));
        if let Some(picture) = self.take_due(clock) {
            let uploaded = self.upload(&picture);
            let first = picture.first;
            self.shared
                .queue
                .lock()
                .expect("队列锁 poisoned")
                .recycle(picture.bgra);
            if uploaded && first {
                self.release_hold();
            }
        }
        let stage = *self.stage.lock().expect("舞台锁 poisoned");
        let quad = &self.audio.quad;
        if !stage.visible {
            if self.presented.swap(false, Ordering::SeqCst) {
                quad.clear_frame();
            }
            return;
        }
        let Some(bounds) = stage.bounds.filter(|bounds| !bounds.is_empty()) else {
            return;
        };
        let texture = self.texture.lock().expect("帧纹理锁 poisoned");
        let Some(texture) = texture.as_ref().filter(|texture| texture.has_frame) else {
            return;
        };
        let scale = *self.audio.scale.lock().expect("DPI 锁 poisoned");
        let dest = geometry(bounds, stage.fit, scale, texture.size);
        quad.set_frame(texture.texture.clone(), texture.size, dest);
        self.presented.store(true, Ordering::SeqCst);
    }

    /// 取该上屏的那一帧：会话首帧立即取；否则取显示时间不晚于时钟的最后一帧，更早的跳过。
    fn take_due(&self, clock: f64) -> Option<Picture> {
        let mut queue = self.shared.queue.lock().expect("队列锁 poisoned");
        if queue.pictures.front().is_some_and(|picture| picture.first) {
            return queue.pictures.pop_front();
        }
        let mut due = None;
        while queue
            .pictures
            .front()
            .is_some_and(|picture| picture.pts <= clock + DUE_AHEAD_SECS)
        {
            let next = queue.pictures.pop_front();
            if let Some(Picture { bgra, .. }) = std::mem::replace(&mut due, next) {
                queue.recycle(bgra);
            }
        }
        due
    }

    /// 把一帧 BGRA 写进帧纹理（没有或已失效就先重建）。
    fn upload(&self, picture: &Picture) -> bool {
        let (Ok(width), Ok(height)) = (u32::try_from(picture.width), u32::try_from(picture.height))
        else {
            return false;
        };
        let quad = &self.audio.quad;
        let generation = quad.generation();
        let mut slot = self.texture.lock().expect("帧纹理锁 poisoned");
        let stale = slot.as_ref().is_none_or(|texture| {
            texture.size != (width, height) || texture.generation != generation
        });
        if stale {
            let created = quad
                .create_frame_texture(width, height)
                .and_then(|texture| {
                    let device = quad.device().ok_or_else(|| {
                        windows::core::Error::from_hresult(windows::Win32::Foundation::E_FAIL)
                    })?;
                    let context = unsafe { device.GetImmediateContext() }?;
                    Ok((texture, context))
                });
            match created {
                Ok((texture, context)) => {
                    *slot = Some(Texture {
                        texture,
                        context,
                        size: (width, height),
                        generation,
                        has_frame: false,
                    });
                    log::info!("[WmDirect] 帧纹理就绪 {width}×{height}（代际 {generation}）");
                }
                Err(error) => {
                    log::warn!("[WmDirect] 帧纹理创建失败（{width}×{height}）: {error}");
                    *slot = None;
                    return false;
                }
            }
        }
        let Some(texture) = slot.as_mut() else {
            return false;
        };
        // SAFETY: 纹理是 DEFAULT 用途、非多重采样，允许 UpdateSubresource；源缓冲至少
        // `bgra_len(宽, 高, 宽×4)` 字节（解码线程按这个长度填的）。渲染器设备开了多线程保护。
        unsafe {
            texture.context.UpdateSubresource(
                &texture.texture,
                0,
                None,
                picture.bgra.as_ptr().cast(),
                width * 4,
                0,
            );
        }
        texture.has_frame = true;
        true
    }

    /// 第一帧已经上屏：放行被扣住的声音（用户自己暂停着的不动）。
    fn release_hold(&self) {
        if !self.shared.hold.swap(false, Ordering::SeqCst) {
            return;
        }
        if !self.audio.paused.load(Ordering::SeqCst) {
            self.audio.play_engine();
        }
    }
}

impl PlayerLike for DirectPlayer {
    fn pause(&self) {
        PlayerLike::pause(&*self.audio);
    }

    fn resume(&self) {
        self.audio.paused.store(false, Ordering::SeqCst);
        // 声音扣着时不起播：第一帧上屏后由 [`DirectPlayer::release_hold`] 放行。
        if !self.shared.hold.load(Ordering::SeqCst) {
            self.audio.play_engine();
        }
    }

    fn is_paused(&self) -> bool {
        self.audio.is_paused()
    }

    fn position_secs(&self) -> f64 {
        self.audio.position_secs()
    }

    fn duration_secs(&self) -> f64 {
        self.audio.duration_secs()
    }

    fn seek(&self, seconds: f64) {
        let seconds = seconds.max(0.0);
        *self.shared.target.lock().expect("起点锁 poisoned") = seconds;
        // 先扣住声音（不改用户的暂停状态），新会话的第一帧上屏后再放行。
        self.shared.hold.store(true, Ordering::SeqCst);
        self.shared.clock.store(seconds.to_bits(), Ordering::SeqCst);
        if let Err(error) = unsafe { self.audio.engine.Pause() } {
            log::warn!("[WmDirect] 跳转前暂停失败: {error}");
        }
        PlayerLike::seek(&*self.audio, seconds);
        start_session(&self.shared, seconds);
    }

    fn set_rate(&self, rate: f64) {
        PlayerLike::set_rate(&*self.audio, rate);
    }

    fn rate(&self) -> f64 {
        PlayerLike::rate(&*self.audio)
    }

    fn set_volume(&self, volume: f32) {
        self.audio.set_volume(volume);
    }

    fn set_muted(&self, muted: bool) {
        self.audio.set_muted(muted);
    }

    fn dims(&self) -> (usize, usize) {
        let packed = self.shared.dims.load(Ordering::Relaxed);
        ((packed >> 32) as usize, (packed & 0xffff_ffff) as usize)
    }

    fn attach(&self, host: &HostWindow) {
        self.audio.attach(host);
    }

    fn submit_stage(&self, bounds: Option<PresenterBounds>, fit: PresenterFit, visible: bool) {
        *self.stage.lock().expect("舞台锁 poisoned") = Stage {
            bounds,
            fit,
            visible,
        };
        self.pump();
    }

    fn detach(&self) {
        self.stage.lock().expect("舞台锁 poisoned").visible = false;
        self.pump();
    }
}

impl Drop for DirectPlayer {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        self.shared.mask.fail();
        self.shared
            .session
            .lock()
            .expect("会话锁 poisoned")
            .store(true, Ordering::SeqCst);
        let mut queue = self.shared.queue.lock().expect("队列锁 poisoned");
        queue.generation += 1;
        queue.pictures.clear();
        queue.spare.clear();
        // 不清视频位的帧：同系统播放器——换集时保持上一集末帧，等新集首帧接棒。
        // 音轨引擎随 `audio` 一起 Shutdown（见 `SystemPlayer` 的 Drop）。
    }
}
