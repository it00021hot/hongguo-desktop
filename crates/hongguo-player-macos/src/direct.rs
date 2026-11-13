//! 内置软解直播放（macOS，GPUI 壳）：HEVC / ByteVC1 / ByteVC2 都由进程内解码器解，
//! 不经过系统视频解码器，也不转码。
//!
//! 分工：
//! - **声音与时钟**：[`AudioPlayer`]（AVPlayer），只读取同一集的音轨视图
//!   （[`loader::Source::Audio`]）。暂停、倍速、跳转、播完、缓冲都沿用它的实现与事件。
//! - **画面**：`AVSampleBufferDisplayLayer` 叠在播放层上（`view.rs` 的 overlay），它的时钟直接
//!   绑到 AVPlayerItem 的时间基——音画同步、暂停、倍速由系统保证，不用自己调度。
//! - **解码**：每次起播/跳转开一个解码会话线程，从目标前的关键帧解起，把帧包成带显示时间的
//!   `CMSampleBuffer` 送进显示层；显示层队列满了就等（背压），跳转时用代次号作废旧会话。
//!
//! 线程：本模块的构造与 [`PlayerLike`] 调用来自 GPUI 主线程；建索引在 `softdec-open` 线程
//! （要读网络，不能堵主线程）；解码在 `softdec-direct` 线程。只有绑定时钟这一步切回主线程。

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use hongguo_common::{AppError, AppResult};
use hongguo_player_api::{
    HostWindow, PlaySource, PlayerEvent, PlayerLike, PresenterBounds, PresenterFit,
};
use hongguo_softdec::IndexedVideo;
use objc2::runtime::AnyObject;
use objc2_foundation::NSArray;
use tokio::sync::mpsc;

use super::cv::{self, FramePacker, OpaqueTimebase};
use super::loader::{self, Source, StallHook};
use super::player::{AudioPlayer, FORWARD_BUFFER_SECS, SendRetained};
use super::{avf, view};

/// 显示层队列满时多久再看一次。
const READY_POLL: Duration = Duration::from_millis(4);
/// 等 AVPlayerItem 就绪（拿到时间基）时多久看一次。
const TIMEBASE_POLL: Duration = Duration::from_millis(10);
/// 帧比时钟晚多少秒就不再上屏（倍速下解码跟不上时宁可跳帧，也不让画面越落越远）。
const LATE_SECS: f64 = 0.1;
/// 样本缓冲上标的帧时长。显示层只按显示时间排程，这个值只是元数据。
const FRAME_SECS: f64 = 1.0 / 30.0;

/// 每个直播放 asset 用不同的 URL，避免 AVFoundation 拿上一集的缓存当这一集。
static NEXT_ASSET: AtomicU64 = AtomicU64::new(1);

pub struct DirectPlayer {
    audio: Arc<AudioPlayer>,
    shared: Arc<Shared>,
}

struct Shared {
    /// 原始数据（在线流或本地文件），解码与音轨视图都从这里读。
    media: Source,
    layer: SendRetained,
    player: SendRetained,
    index: Mutex<Option<Arc<IndexedVideo>>>,
    /// 当前解码会话的代次。上屏与清屏都在这把锁里做：跳转时代次加一并清屏，
    /// 旧会话哪怕已经包好了帧，拿锁后一看代次不对也不会再送进去。
    generation: Mutex<u64>,
    /// 当前解码会话的取消位。
    session: Mutex<Arc<AtomicBool>>,
    /// 最近一次要求的起点（秒）。索引还没建好时的跳转记在这里，建好后从这里起解。
    target: Mutex<f64>,
    stop: AtomicBool,
    /// 已绑定到显示层的时间基（指针值；0 = 还没绑）。
    timebase: AtomicUsize,
    /// 画面尺寸，高 32 位宽、低 32 位高。
    dims: AtomicU64,
    first_frame_sent: AtomicBool,
    events: mpsc::Sender<PlayerEvent>,
    /// 声音在等画面：起播与跳转后先不让时钟走，第一帧送进显示层再放行。
    /// 不这么做的话，时钟在解码器还从关键帧往目标点追的时候就跑起来了，
    /// 画面一出来就落后几百毫秒、只能丢帧去追（实测跳转后落后 0.3–0.5 秒、丢 11–19 帧）。
    hold: AtomicBool,
    /// 音轨播放器（放行时要看用户是否暂停、按什么倍速起播）。弱引用：播放器归 [`DirectPlayer`] 所有。
    audio: std::sync::OnceLock<std::sync::Weak<AudioPlayer>>,
}

/// 第一帧已经送进显示层：放行被扣住的声音（用户自己暂停着的不动）。
fn release_hold(shared: &Shared) {
    if !shared.hold.swap(false, Ordering::SeqCst) {
        return;
    }
    let Some(audio) = shared.audio.get().and_then(std::sync::Weak::upgrade) else {
        return;
    };
    if !audio.paused.load(Ordering::SeqCst) {
        avf::play_immediately(audio.player.get(), audio.rate() as f32);
    }
}

/// 打开一集内置软解直播放。返回 `None` = 线程都起不来。
pub fn open(
    source: PlaySource,
    start_secs: f64,
    rate: f64,
    events: mpsc::Sender<PlayerEvent>,
    on_stall: Option<StallHook>,
) -> Option<Arc<dyn PlayerLike>> {
    let media = match source {
        PlaySource::Online { vid, definition } => Source::Stream {
            vid,
            definition,
            on_stall,
        },
        PlaySource::Local { path } => Source::File(path),
    };
    let label = media.label();
    // item 要等索引建好（知道视频 trak 在哪才能遮掉它）才挂上，先起一个空的播放器。
    let no_items: &[&AnyObject] = &[];
    let player = avf::queue_player(&NSArray::from_slice(no_items));
    avf::disable_auto_advance(&player);
    avf::disable_wait_to_minimize_stalling(&player);
    avf::set_volume(&player, 1.0);
    let audio = AudioPlayer::assemble(player.clone(), rate);
    audio.spawn_events(events.clone());
    let shared = Arc::new(Shared {
        media,
        layer: SendRetained(cv::display_layer()),
        player: SendRetained(player),
        index: Mutex::new(None),
        generation: Mutex::new(0),
        session: Mutex::new(Arc::new(AtomicBool::new(true))),
        target: Mutex::new(start_secs.max(0.0)),
        stop: AtomicBool::new(false),
        timebase: AtomicUsize::new(0),
        dims: AtomicU64::new(0),
        first_frame_sent: AtomicBool::new(false),
        events,
        hold: AtomicBool::new(true),
        audio: std::sync::OnceLock::new(),
    });
    let _ = shared.audio.set(Arc::downgrade(&audio));
    let worker_audio = Arc::clone(&audio);
    let worker_shared = Arc::clone(&shared);
    let spawned = std::thread::Builder::new()
        .name("softdec-open".into())
        .spawn(move || {
            let started = Instant::now();
            match prepare(&worker_audio, &worker_shared) {
                Ok(()) => log::info!(
                    "[Direct] {} 索引就绪 {}ms",
                    worker_shared.media.label(),
                    started.elapsed().as_millis()
                ),
                Err(_) if worker_shared.stop.load(Ordering::SeqCst) => {}
                Err(error) => {
                    log::warn!("[Direct] {} 起播失败: {error}", worker_shared.media.label());
                    let _ = worker_shared
                        .events
                        .blocking_send(PlayerEvent::Failed(error.to_string()));
                }
            }
        });
    if let Err(error) = spawned {
        log::error!("[Direct] 起播线程起不来: {error}");
        return None;
    }
    log::info!("[Direct] 内置软解直播放已打开 {label}（起点 {start_secs:.1}s，{rate:.2}x）");
    Some(Arc::new(DirectPlayer { audio, shared }))
}

/// 建索引、挂上音轨 item、开第一个解码会话。在 `softdec-open` 线程上跑。
fn prepare(audio: &AudioPlayer, shared: &Arc<Shared>) -> AppResult<()> {
    let stop = &shared.stop;
    let length = loader::total_bytes(&shared.media, stop)
        .ok_or_else(|| AppError::Media("拿不到这一集的总长度".into()))?;
    let mut reader = Reader::new(&shared.media)?;
    let index = Arc::new(IndexedVideo::open(length, |offset, size, bytes| {
        reader.read(offset, size, stop, bytes)
    })?);
    *shared.index.lock().expect("索引锁 poisoned") = Some(Arc::clone(&index));
    if stop.load(Ordering::SeqCst) {
        return Err(AppError::Cancelled);
    }

    let id = NEXT_ASSET.fetch_add(1, Ordering::Relaxed);
    let url = avf::custom_url(&format!("direct-{id}"))
        .ok_or_else(|| AppError::Internal("直播放 URL 构造失败".into()))?;
    let asset = avf::asset_with_url(&url);
    let handle = loader::attach(
        &asset,
        Source::Audio {
            inner: Box::new(shared.media.clone()),
            index: Arc::clone(&index),
        },
    );
    let item = avf::item_with_asset(&asset);
    avf::set_spectral_pitch(&item);
    avf::set_preferred_forward_buffer_duration(&item, FORWARD_BUFFER_SECS);
    avf::replace_item(audio.player.get(), &item);
    *audio
        .current_loader
        .lock()
        .expect("当前 loader 锁 poisoned") = Some(handle);
    *audio.current_asset.lock().expect("当前 asset 锁 poisoned") = Some(SendRetained(asset));

    let target = *shared.target.lock().expect("起点锁 poisoned");
    if target > 0.5 {
        avf::seek_exact(audio.player.get(), avf::CmTime::secs(target));
    }
    // 不在这里起播：`hold` 初始为真，第一帧上屏时由解码会话放行（见 [`release_hold`]）。
    log::info!(
        "[Direct] {} codec={:?} 源色彩 {:?}",
        shared.media.label(),
        index.codec(),
        index.color()
    );
    start_session(shared, target);
    Ok(())
}

/// 作废当前解码会话、清屏，从 `target` 起开一个新会话。索引还没建好时什么都不做
/// （[`prepare`] 建好后会用最新的起点开）。
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
        let mut generation = shared.generation.lock().expect("代次锁 poisoned");
        *generation += 1;
        // 只清队列不清当前画面：跳转期间停在旧画面上，比闪一下黑好。
        cv::flush(shared.layer.get());
        *generation
    };
    let session_shared = Arc::clone(shared);
    let spawned = std::thread::Builder::new()
        .name("softdec-direct".into())
        .spawn(move || decode_session(&session_shared, &index, target, generation, &cancel));
    if let Err(error) = spawned {
        log::error!("[Direct] 解码线程起不来: {error}");
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
    // 时钟（AVPlayerItem 的时间基）要等音轨 item 加载就绪才有。不在这里先等：
    // 从关键帧解到目标点那一段与 item 加载并行做，等到要送第一帧时才必须有时钟。
    let mut timebase: Option<*mut OpaqueTimebase> = None;
    let layer = shared.layer.get();
    let color = index.color();
    let mut reader = match Reader::new(&shared.media) {
        Ok(reader) => reader,
        Err(error) => {
            let _ = shared
                .events
                .blocking_send(PlayerEvent::Failed(error.to_string()));
            return;
        }
    };
    let mut packer: Option<FramePacker> = None;
    // 跳转后时钟要过一会儿才落到新位置；落地前帧的「早晚」没有意义，一律上屏。
    let mut landed = false;
    let (mut queued, mut dropped) = (0u64, 0u64);
    // 落地后上屏帧最多比时钟晚多少秒（倍速下解码是否跟得上的直接指标）。
    let mut worst_lag = 0f64;
    // 取证（HONGGUO_AV_METRICS=1，与系统播放器的产帧取证同一开关）：每 5 秒打一行这一窗口的上屏帧率、丢帧与落后量。
    let metrics = std::env::var("HONGGUO_AV_METRICS").is_ok_and(|value| value == "1");
    let mut window = (Instant::now(), queued, dropped, 0f64);
    let result = index.decode_from(
        target,
        |offset, size, bytes| reader.read(offset, size, cancel, bytes),
        |frame| {
            if cancelled() {
                return Err(AppError::Cancelled);
            }
            if metrics && window.0.elapsed() >= Duration::from_secs(5) {
                let secs = window.0.elapsed().as_secs_f64();
                let (clock_at, clock_rate) = timebase.map_or((f64::NAN, f64::NAN), |clock| {
                    (cv::timebase_seconds(clock), cv::timebase_rate(clock))
                });
                log::info!(
                    "[Direct][metrics] session={generation} {}x{} 5s 上屏 {:.1}fps 丢弃 {} 最大落后 {:.0}ms 时钟 {clock_at:.3}s×{clock_rate:.2} 帧 {:.3}s",
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
            let clock = match timebase {
                Some(clock) => clock,
                None => loop {
                    if cancelled() {
                        return Err(AppError::Cancelled);
                    }
                    if let Some(clock) = bind_timebase(shared) {
                        timebase = Some(clock);
                        break clock;
                    }
                    std::thread::sleep(TIMEBASE_POLL);
                },
            };
            while !cv::ready_for_more(layer) {
                if cancelled() {
                    return Err(AppError::Cancelled);
                }
                std::thread::sleep(READY_POLL);
            }
            let now = cv::timebase_seconds(clock);
            landed = landed || (now - target).abs() < 1.0;
            if landed && queued > 0 && pts + LATE_SECS < now {
                dropped += 1;
                return Ok(());
            }
            if landed {
                worst_lag = worst_lag.max(now - pts);
                window.3 = window.3.max(now - pts);
            }
            if packer.as_ref().is_none_or(|packer| !packer.fits(&frame, color)) {
                packer = FramePacker::new(frame.width, frame.height, color);
            }
            let sample = packer
                .as_ref()
                .and_then(|packer| packer.pack(&frame, pts, FRAME_SECS))
                .ok_or_else(|| AppError::Media("解码帧装不进显示缓冲".into()))?;
            {
                let current = shared.generation.lock().expect("代次锁 poisoned");
                if *current != generation {
                    return Err(AppError::Cancelled);
                }
                if let Some(reason) = cv::failure(layer) {
                    // 失败态要先 flush 才能再收帧（Apple 文档给的恢复方式）。
                    log::warn!("[Direct] 显示层失败，清空后继续: {reason}");
                    cv::flush(layer);
                }
                cv::enqueue(layer, &sample);
            }
            queued += 1;
            if queued == 1 {
                release_hold(shared);
                let (width, height) = (frame.width, frame.height);
                shared
                    .dims
                    .store(((width as u64) << 32) | height as u64, Ordering::Relaxed);
                // 这行是「这一集确实走了内置软解」的可观察证据。
                log::info!(
                    "[Direct] session={generation} codec={:?} {width}x{height} target={target:.3}s pts={pts:.3}s decode_ms={}",
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
            log::warn!("[Direct] session={generation} 解码失败: {error}");
            let _ = shared
                .events
                .blocking_send(PlayerEvent::Failed(error.to_string()));
        }
    }
    log::info!(
        "[Direct] session={generation} 结束：上屏 {queued} 帧，丢弃 {dropped} 帧，最大落后 {:.0}ms，{}ms",
        worst_lag * 1000.0,
        started.elapsed().as_millis()
    );
}

/// 把显示层的时钟绑到 AVPlayerItem 的时间基上。item 还没就绪返回 `None`。
fn bind_timebase(shared: &Shared) -> Option<*mut OpaqueTimebase> {
    let bound = shared.timebase.load(Ordering::SeqCst);
    if bound != 0 {
        return Some(bound as *mut OpaqueTimebase);
    }
    let item = avf::current_item(shared.player.get())?;
    if avf::item_status(item) != 1 {
        return None;
    }
    let timebase = avf::item_timebase(item);
    if timebase.is_null() {
        return None;
    }
    let layer = shared.layer.clone_ref();
    let address = timebase as usize;
    on_main(move || cv::set_control_timebase(layer.get(), address as *mut OpaqueTimebase));
    shared.timebase.store(address, Ordering::SeqCst);
    Some(timebase)
}

/// 解码读数据。本地文件只开一次（一集几千个样本，逐样本开文件是纯开销）；在线流按需等待与续填。
enum Reader<'a> {
    File(std::fs::File),
    Other(&'a Source),
}

impl<'a> Reader<'a> {
    fn new(media: &'a Source) -> AppResult<Self> {
        match media {
            Source::File(path) => std::fs::File::open(path)
                .map(Reader::File)
                .map_err(|error| AppError::Io(error.to_string())),
            other => Ok(Reader::Other(other)),
        }
    }

    fn read(
        &mut self,
        offset: u64,
        length: u64,
        cancelled: &AtomicBool,
        bytes: &mut Vec<u8>,
    ) -> AppResult<()> {
        if cancelled.load(Ordering::SeqCst) {
            return Err(AppError::Cancelled);
        }
        match self {
            Reader::File(file) => {
                use std::io::{Read, Seek, SeekFrom};
                let length =
                    usize::try_from(length).map_err(|_| AppError::Media("读取范围过大".into()))?;
                file.seek(SeekFrom::Start(offset))
                    .map_err(|error| AppError::Io(error.to_string()))?;
                bytes.resize(length, 0);
                file.read_exact(bytes)
                    .map_err(|error| AppError::Io(error.to_string()))
            }
            Reader::Other(source) => {
                match loader::read_exact(source, offset, length, cancelled, bytes) {
                    Ok(true) => Ok(()),
                    Ok(false) => Err(AppError::Cancelled),
                    Err(message) => Err(AppError::Media(message)),
                }
            }
        }
    }
}

#[link(name = "System")]
unsafe extern "C" {
    static _dispatch_main_q: u8;
    fn dispatch_sync_f(
        queue: *const c_void,
        context: *mut c_void,
        work: extern "C" fn(*mut c_void),
    );
    fn pthread_main_np() -> i32;
}

/// 在主线程上同步执行 `work`（已在主线程就直接执行）。调用方不能持有主线程在等的锁。
fn on_main<F: FnOnce() + Send>(work: F) {
    // SAFETY: 只查询当前线程。
    if unsafe { pthread_main_np() } != 0 {
        work();
        return;
    }
    extern "C" fn trampoline<F: FnOnce()>(context: *mut c_void) {
        // SAFETY: context 指向调用方栈上的 Option<F>，dispatch_sync_f 返回前一直有效。
        let slot = unsafe { &mut *context.cast::<Option<F>>() };
        if let Some(work) = slot.take() {
            work();
        }
    }
    let mut slot = Some(work);
    // SAFETY: 主队列常驻；同步派发，slot 在返回前有效。
    unsafe {
        dispatch_sync_f(
            (&raw const _dispatch_main_q).cast(),
            (&raw mut slot).cast(),
            trampoline::<F>,
        );
    }
}

impl PlayerLike for DirectPlayer {
    fn pause(&self) {
        self.audio.pause();
    }

    fn resume(&self) {
        self.audio.paused.store(false, Ordering::SeqCst);
        if !self.shared.hold.load(Ordering::SeqCst) {
            self.audio.resume();
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
        avf::pause(self.audio.player.get());
        self.audio.seek(seconds);
        start_session(&self.shared, seconds);
    }

    fn set_rate(&self, rate: f64) {
        self.audio.set_rate(rate);
        if !self.shared.hold.load(Ordering::SeqCst) && !self.audio.is_paused() {
            avf::set_rate(self.audio.player.get(), rate as f32);
        }
    }

    fn rate(&self) -> f64 {
        self.audio.rate()
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
        let _ = view::ensure(host, self.shared.layer.get());
    }

    fn submit_stage(&self, bounds: Option<PresenterBounds>, fit: PresenterFit, visible: bool) {
        view::submit(bounds, fit, visible);
    }

    fn detach(&self) {
        view::hide();
    }
}

impl Drop for DirectPlayer {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        self.shared
            .session
            .lock()
            .expect("会话锁 poisoned")
            .store(true, Ordering::SeqCst);
        let mut generation = self.shared.generation.lock().expect("代次锁 poisoned");
        *generation += 1;
        cv::flush_and_clear(self.shared.layer.get());
    }
}
