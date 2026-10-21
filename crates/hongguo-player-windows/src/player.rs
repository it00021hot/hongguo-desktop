//! Windows 系统播放器：MediaEngine 的 [`PlayerLike`] / [`VideoPresenter`] 实现。
//!
//! 与 macOS 实现的 `player.rs` 对位：引擎自己解码、自己出声、自己把画面合成进
//! DirectComposition 视觉；本层只做控制面（播放/暂停/seek/倍速/音量）、舞台几何
//! 下发与引擎统计取证。窗口形态切换不重建引擎，只改视觉的 offset/scale/clip
//! （[`SystemVideoStage`]）。
//!
//! 引擎渲染分辨率：rendering mode 默认把视频画进一块 640×480 的固定表面（spike
//! 实测），所以每次拿到原生尺寸就用 `UpdateVideoStream` 把落点重设成视频原生
//! 尺寸——画质与"是否全屏"解耦，缩放交给 DComp。

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use gpui_pre_windows::system_video::{SystemVideoLayer, SystemVideoStage};
use hongguo_player_api::{
    HostWindow, PlaySource, PlayerEvent, PlayerLike, PresenterBounds, PresenterFit, StallHook,
    VideoPresenter,
};
use tokio::sync::mpsc;
use windows::Win32::Foundation::RECT;
use windows::Win32::Media::MediaFoundation::{
    IMFByteStream, IMFMediaEngine, IMFMediaEngineEx, IMFMediaEngineNotify,
    MF_MEDIA_ENGINE_STATISTIC_FRAMES_DROPPED, MF_MEDIA_ENGINE_STATISTIC_FRAMES_RENDERED,
};

use super::{events, mf, stream, view};

/// 引擎统计取证周期（`HONGGUO_WM_METRICS=1` 才打）。
const METRICS_EVERY: Duration = Duration::from_secs(5);
/// 续播起点低于这个秒数时不 seek（与 macOS 侧同参：0 附近的起点没意义）。
const SEEK_EPSILON_SECS: f64 = 0.5;
/// 倍速补发的节流间隔：`IsPlaybackRateSupported` 是一次 COM 往返，逐帧问太贵。
const RATE_RETRY_EVERY: Duration = Duration::from_millis(250);

/// 取证开关（进程级只读一次）：`log_metrics` 每帧都会走到，每次读环境变量
/// 都要分配一个 String。
fn metrics_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var("HONGGUO_WM_METRICS").is_ok_and(|value| value == "1"))
}

/// 舞台几何下发的输入（每次 render 提交一次）。
#[derive(Clone, Copy)]
struct Staged {
    bounds: Option<PresenterBounds>,
    fit: PresenterFit,
    visible: bool,
}

struct ViewState {
    /// 视频位句柄：`attach` 时按需取，提交失败（渲染器重建）后清空重取。
    layer: Option<SystemVideoLayer>,
    /// 窗口 DPI 缩放：GPUI 逻辑像素 → DComp 物理像素。
    scale: f32,
}

/// 进程级「当前引擎」：MediaEngine 没有播放列表，每集重建一个引擎的代价是
/// 实测两笔——切集 287ms，以及**每集约 35MB 的原生解码器/设备开销堆积**
/// （进程内存实测每换一集涨约 35MB，而 Rust 侧缓存是平的，见壳的
/// `[Home][mem]` 日志）。
///
/// 这里把引擎留住：换集时换源（`SetSourceFromByteStream`）而不是重建。
/// 换源不成功（引擎已被 Shutdown / 窗口重建）就退回重建。
struct EngineCell {
    engine: IMFMediaEngine,
    extended: IMFMediaEngineEx,
    /// 事件回调的共享状态（换源时换事件通道，见 `events::NotifyShared`）。
    shared: Arc<events::NotifyShared>,
    /// 换源代数：每次换源 +1。旧句柄在 Drop 时只在「引擎现在放的还是我这一集」
    /// 时才停引擎——换源之后引擎已经在放新的一集，停它等于把新集也停了。
    epoch: AtomicU64,
    /// 引擎创建时的宿主窗口（HWND）：窗口重建后旧引擎的落点视觉作废，不复用。
    window: isize,
}

/// 进程级引擎槽：存着「当前这一集在用的引擎」，下一集换源复用它。
static ENGINE_SLOT: Mutex<Option<Arc<EngineCell>>> = Mutex::new(None);

// SAFETY: 与 [`SystemPlayer`] 同一套约定——Media Foundation 的接口是自由线程的，
// 引擎的调用点全在 GPUI 主线程；槽里的 `Arc` 只做所有权传递，不跨线程调方法。
unsafe impl Send for EngineCell {}
unsafe impl Sync for EngineCell {}

pub struct SystemPlayer {
    cell: Arc<EngineCell>,
    /// 本句柄打开时的换源代数（Drop 时据此判断引擎现在放的是不是我的这一集）。
    epoch: u64,
    /// 在线流的字节流（引擎持有引用，这里只是明确保活）；本地文件为 None。
    _stream: Option<IMFByteStream>,
    paused: AtomicBool,
    rate: AtomicU64,
    staged: Mutex<Staged>,
    state: Mutex<ViewState>,
    /// 已下发给引擎的渲染尺寸（打包 w<<32|h）。
    announced: AtomicU64,
    /// 期望倍速（用户偏好）与实际生效的倍速：metadata 就绪前引擎一律报「不支持」。
    applied_rate: AtomicU64,
    rate_retry_at: Mutex<Instant>,
    metrics_at: Mutex<Instant>,
}

// SAFETY: Media Foundation 与 DirectComposition 的接口都是自由线程的；本结构的
// 所有方法只在 GPUI 主线程调用（`PlayerLike` 的调用点全在视图/事件回调里），
// `Drop` 在任意线程只做 Release。与 macOS 侧 `SendRetained` 同一套约定。
unsafe impl Send for SystemPlayer {}
unsafe impl Sync for SystemPlayer {}

/// 打开一集系统播放器。返回 `None` 表示引擎起不来——调用方回落到自研上屏。
///
/// 引擎尽量复用：进程级槽里若还有**同一个窗口**的引擎，就换源接着用
/// （不重建引擎 = 不付重建的 287ms，也不堆原生解码器资源）。
pub fn open(
    source: PlaySource,
    start_secs: f64,
    rate: f64,
    events: mpsc::Sender<PlayerEvent>,
    on_stall: Option<StallHook>,
) -> Option<Arc<dyn PlayerLike>> {
    // 分段耗时：起播慢要能拆开看——「建引擎 / 挂源 / 起播」各占多少，
    // 才知道该优化哪一段（壳侧只记得到首帧的总时长）。
    let opened_at = Instant::now();
    let layer = view::current_layer()?;
    let window = view::window_id()?;

    // 复用路径：槽里的引擎属于同一个窗口才复用（窗口重建后旧引擎的落点
    // 视觉作废，复用它会黑屏）。
    let reused = ENGINE_SLOT.lock().unwrap_or_else(|e| e.into_inner()).take();
    if let Some(cell) = reused {
        if cell.window != window {
            log::info!("[WmPresenter] 宿主窗口已重建，不复用旧引擎");
        } else {
            cell.shared.attach(events.clone());
            match attach_source(&cell, source.clone(), on_stall.clone()) {
                Ok(byte_stream) => {
                    let epoch = cell.epoch.fetch_add(1, Ordering::SeqCst) + 1;
                    return finish_open(
                        cell,
                        byte_stream,
                        epoch,
                        start_secs,
                        rate,
                        opened_at,
                        layer,
                        Reused::Yes,
                    );
                }
                Err(error) => {
                    log::warn!("[WmPresenter] 复用引擎换源失败（{error}），重建引擎");
                }
            }
        }
    }

    // 新建引擎：回调状态与事件通道一起建（换源时只换通道、引擎不动）。
    let shared = Arc::new(events::NotifyShared::new(events));
    let notify: IMFMediaEngineNotify = events::Notify::new(Arc::clone(&shared)).into();
    let (engine, extended) = match mf::create_engine(layer.render_target(), &notify) {
        Ok(engine) => engine,
        Err(error) => {
            log::error!("[WmPresenter] MediaEngine 创建失败: {error}");
            return None;
        }
    };
    let engine_at = Instant::now();
    let cell = Arc::new(EngineCell {
        engine,
        extended,
        shared,
        epoch: AtomicU64::new(0),
        window,
    });
    let byte_stream = match attach_source(&cell, source, on_stall) {
        Ok(byte_stream) => byte_stream,
        Err(error) => {
            log::error!("[WmPresenter] {error}");
            return None;
        }
    };
    let epoch = cell.epoch.fetch_add(1, Ordering::SeqCst) + 1;
    finish_open(
        cell,
        byte_stream,
        epoch,
        start_secs,
        rate,
        opened_at,
        layer,
        Reused::No(engine_at),
    )
}

/// 引擎是复用的还是新建的（只影响耗时日志的措辞）。
enum Reused {
    Yes,
    No(Instant),
}

/// 把一集的源挂到引擎上（新建与换源共用一条路）：在线流走字节流，本地文件走 URL。
fn attach_source(
    cell: &EngineCell,
    source: PlaySource,
    on_stall: Option<StallHook>,
) -> Result<Option<IMFByteStream>, String> {
    // 【实测结论】Media Engine 的换源（`SetSourceFromByteStream`）只换源、**不
    // 关旧源**：旧解码器的显存帧池与 CPU 参考帧要等旧源自己关闭才释放，实测
    // 逐集滞留（每集约 19MB 显存 / 55MB 系统内存，连播 6 集涨 95MB/265MB）。
    // 断字节流无效（drain 不是瓶颈）；重建引擎也无效（曲线逐点相同）⇒ 涨的
    // 是每台新设备/新解码器的常驻开销，MF 与驱动都不归还。`IMFMediaEngine`
    // 也没有「只关源」的 API（`Stop` 是 MediaSession 的）⇒ Media Engine 路线
    // 下无法主动释放，只能靠「不换解码器」（虚拟播放列表）根治。
    match source {
        PlaySource::Online { vid, definition } => {
            let stream = stream::open(vid.clone(), definition, on_stall);
            let url = windows::core::BSTR::from(format!("https://hongguo.local/stream/{vid}.mp4"));
            unsafe { cell.extended.SetSourceFromByteStream(&stream, &url) }
                .map_err(|error| format!("设置字节流源失败（{vid}@{definition}）: {error}"))?;
            Ok(Some(stream))
        }
        PlaySource::Local { path } => {
            let url = windows::core::BSTR::from(format!(
                "file:///{}",
                path.to_string_lossy().replace('\\', "/")
            ));
            unsafe { cell.engine.SetSource(&url) }
                .map_err(|error| format!("设置本地文件源失败（{}）: {error}", path.display()))?;
            Ok(None)
        }
    }
}

/// 起播收尾：定位起点、起播、组装句柄，并把引擎放回进程级槽（下一集换源复用）。
#[expect(
    clippy::too_many_arguments,
    reason = "起播参数面一一对应，不拆结构体反而失真"
)]
fn finish_open(
    cell: Arc<EngineCell>,
    byte_stream: Option<IMFByteStream>,
    epoch: u64,
    start_secs: f64,
    rate: f64,
    opened_at: Instant,
    layer: SystemVideoLayer,
    reused: Reused,
) -> Option<Arc<dyn PlayerLike>> {
    let source_at = Instant::now();
    unsafe {
        // 续播起点：metadata 就绪后引擎会按这个位置出画。
        if start_secs > SEEK_EPSILON_SECS {
            cell.engine.SetCurrentTime(start_secs).ok();
        }
        if let Err(error) = cell.engine.Play() {
            log::error!("[WmPresenter] Play 失败: {error}");
            return None;
        }
    }
    let play_at = Instant::now();
    let player = Arc::new(SystemPlayer {
        cell: Arc::clone(&cell),
        epoch,
        _stream: byte_stream,
        paused: AtomicBool::new(false),
        // 倍速在 metadata 就绪前 `IsPlaybackRateSupported` 一律 false（spike 实测）：
        // 期望值先记下，每帧按节流重试，就绪那一下补发（[`try_apply_rate`]）。
        rate: AtomicU64::new(rate.to_bits()),
        applied_rate: AtomicU64::new(1.0f64.to_bits()),
        // 往回拨一个节流周期：open 当下的首次补发不被节流挡掉。
        rate_retry_at: Mutex::new(
            Instant::now()
                .checked_sub(RATE_RETRY_EVERY)
                .unwrap_or_else(Instant::now),
        ),
        staged: Mutex::new(Staged {
            bounds: None,
            fit: PresenterFit::Contain,
            visible: false,
        }),
        state: Mutex::new(ViewState {
            layer: Some(layer),
            scale: 1.0,
        }),
        announced: AtomicU64::new(0),
        metrics_at: Mutex::new(Instant::now()),
    });
    player.try_apply_rate();
    // 放回进程级槽：下一集换源复用（而不是重建引擎）。
    *ENGINE_SLOT.lock().unwrap_or_else(|e| e.into_inner()) = Some(cell);
    match reused {
        Reused::Yes => log::info!(
            "[WmPresenter] 复用引擎换源起播（起点 {start_secs:.1}s，期望 {rate:.2}x）：挂源 {:.0}ms / 起播 {:.0}ms",
            source_at.duration_since(opened_at).as_secs_f64() * 1000.0,
            play_at.duration_since(source_at).as_secs_f64() * 1000.0,
        ),
        Reused::No(engine_at) => log::info!(
            "[WmPresenter] 系统播放器已打开（起点 {start_secs:.1}s，期望 {rate:.2}x）：建引擎 {:.0}ms / 挂源 {:.0}ms / 起播 {:.0}ms",
            engine_at.duration_since(opened_at).as_secs_f64() * 1000.0,
            source_at.duration_since(engine_at).as_secs_f64() * 1000.0,
            play_at.duration_since(source_at).as_secs_f64() * 1000.0,
        ),
    }
    Some(player)
}

impl SystemPlayer {
    /// 原生视频尺寸（metadata 未就绪时为 (0, 0)）。
    ///
    /// **一次 item 内尺寸不变**：metadata 就绪后缓存住（打包进 `announced`），
    /// 省掉每帧一次 `GetNativeVideoSize` 的 COM 往返——舞台几何与黑边两处
    /// 每帧都要问原生尺寸，不缓存就是每帧两次 COM 调用。
    fn native_size(&self) -> (u32, u32) {
        let announced = self.announced.load(Ordering::SeqCst);
        if announced != 0 {
            return ((announced >> 32) as u32, announced as u32);
        }
        let mut width = 0u32;
        let mut height = 0u32;
        unsafe {
            self.cell
                .engine
                .GetNativeVideoSize(Some(&mut width as *mut u32), Some(&mut height as *mut u32))
                .ok();
        }
        (width, height)
    }

    /// 把引擎的渲染落点设成视频原生尺寸（rendering mode 默认 640×480 固定表面）。
    fn announce_native_size(&self) {
        let (width, height) = self.native_size();
        if width == 0 || height == 0 {
            return;
        }
        let packed = (u64::from(width) << 32) | u64::from(height);
        if self.announced.swap(packed, Ordering::SeqCst) == packed {
            return;
        }
        let rect = RECT {
            left: 0,
            top: 0,
            right: width as i32,
            bottom: height as i32,
        };
        match unsafe {
            self.cell
                .extended
                .UpdateVideoStream(None, Some(&rect), None)
        } {
            Ok(()) => log::info!("[WmPresenter] 渲染落点设为原生 {width}×{height}"),
            Err(error) => {
                log::warn!("[WmPresenter] UpdateVideoStream({width}×{height}) 失败: {error}")
            }
        }
    }

    /// 提交舞台几何（幂等；几何没变时 [view::submit] 内部直接返回）。
    fn apply_stage(&self) {
        let staged = *self.staged.lock().expect("舞台锁 poisoned");
        // 先 announce（顺带把原生尺寸缓存住），再取尺寸：announce 之后
        // `native_size` 命中缓存，不再有 COM 往返。
        self.announce_native_size();
        let (native_w, native_h) = self.native_size();
        // 倍速可能因为「metadata 还没就绪」在 open 时被拒：每帧按节流补试。
        self.try_apply_rate();

        let Some(layer) = self.state.lock().expect("视频位锁 poisoned").layer.clone() else {
            return;
        };
        let stage = staged
            .bounds
            .filter(|bounds| staged.visible && !bounds.is_empty() && native_w > 0 && native_h > 0)
            .map(|bounds| {
                let scale = self.state.lock().expect("视频位锁 poisoned").scale;
                geometry(bounds, staged.fit, scale, (native_w, native_h))
            });
        let alive = view::submit(&layer, stage.as_ref());
        if !alive {
            // 渲染器重建（设备丢失）后旧视觉失效：清掉，下一次 attach 重取。
            self.state.lock().expect("视频位锁 poisoned").layer = None;
            // 引擎的画面落点就是那个作废的视觉：它已经画不出东西了，别让下一集
            // 换源复用它（换源会成功、但画面是黑的）——从槽里摘掉，下一集重建。
            let mut slot = ENGINE_SLOT.lock().expect("引擎槽锁 poisoned");
            if slot
                .as_ref()
                .is_some_and(|cell| std::ptr::eq(cell.as_ref(), self.cell.as_ref()))
            {
                *slot = None;
            }
        }
        self.log_metrics();
    }

    /// 把期望倍速补发给引擎（引擎只在 metadata 就绪后才认这个值）。
    ///
    /// 节流到 [`RATE_RETRY_EVERY`]：`apply_stage` 每帧都会调它，而
    /// `IsPlaybackRateSupported` 是一次 COM 往返。
    fn try_apply_rate(&self) {
        let desired = self.rate();
        let applied = f64::from_bits(self.applied_rate.load(Ordering::SeqCst));
        if (desired - applied).abs() <= f64::EPSILON {
            return;
        }
        {
            let mut at = self.rate_retry_at.lock().expect("倍速重试锁 poisoned");
            if at.elapsed() < RATE_RETRY_EVERY {
                return;
            }
            *at = Instant::now();
        }
        if !mf::rate_supported(&self.cell.extended, desired) {
            log::debug!("[WmPresenter] 引擎暂不支持 {desired}x（metadata 未就绪？）");
            return;
        }
        match unsafe { self.cell.engine.SetPlaybackRate(desired) } {
            Ok(()) => {
                self.applied_rate.store(desired.to_bits(), Ordering::SeqCst);
                log::info!("[WmPresenter] 倍速生效 {desired:.2}x");
            }
            Err(error) => log::warn!("[WmPresenter] 设置 {desired}x 失败: {error}"),
        }
    }

    /// 引擎统计取证（5s 一次）：帧率/丢帧是「2x 零丢帧」验收的唯一客观读数。
    fn log_metrics(&self) {
        if !metrics_enabled() {
            return;
        }
        let mut last = self.metrics_at.lock().expect("取证时刻锁 poisoned");
        if last.elapsed() < METRICS_EVERY {
            return;
        }
        *last = Instant::now();
        let rendered = mf::statistic(
            &self.cell.extended,
            MF_MEDIA_ENGINE_STATISTIC_FRAMES_RENDERED,
        );
        let dropped = mf::statistic(
            &self.cell.extended,
            MF_MEDIA_ENGINE_STATISTIC_FRAMES_DROPPED,
        );
        log::info!(
            "[WmPresenter][metrics] 5s rendered={rendered:?} dropped={dropped:?} rate={:.2}x pos={:.2}s",
            self.rate(),
            unsafe { self.cell.engine.GetCurrentTime() }
        );
    }
}

/// 舞台几何换算：视频原生尺寸 → 舞台内的位置/缩放（`contain` 留黑边、
/// `cover` 溢出裁切，裁剪在视觉树里由裁剪视觉完成）。
///
/// **落点对齐到整物理像素**：视频位是独立的 DComp 视觉，它和 GPUI 内容不在
/// 同一张画布上；分数坐标 + 双线性采样会让画面最外圈半个像素采到渲染目标
/// 之外（透明），叠在透明洞上就是一条浅色细线（用户实测「窗口缩放时视频
/// 边缘有白线、像不在一个图层」）。取整后视频与目标 1:1 映射，边缘像素完整
/// 不透明；舞台原点同样取整，窗口拖动/缩放时不会出现 1px 抖动。
fn geometry(
    bounds: PresenterBounds,
    fit: PresenterFit,
    scale: f32,
    (native_w, native_h): (u32, u32),
) -> SystemVideoStage {
    let scale = f64::from(scale);
    let x = (f64::from(bounds.x) * scale).round();
    let y = (f64::from(bounds.y) * scale).round();
    let width = (f64::from(bounds.width) * scale).round().max(1.0);
    let height = (f64::from(bounds.height) * scale).round().max(1.0);
    let native_w = f64::from(native_w);
    let native_h = f64::from(native_h);
    let contain = (width / native_w).min(height / native_h);
    let cover = (width / native_w).max(height / native_h);
    let ratio = match fit {
        PresenterFit::Contain => contain,
        PresenterFit::Cover => cover,
    };
    // 视频在该比例下的整数像素尺寸（逐轴取整：contain 下两轴比例一致，
    // 取整误差 ≤0.5px，肉眼不可见；换来的是边缘采样不再越界）。
    let video_w = (native_w * ratio).round().max(1.0);
    let video_h = (native_h * ratio).round().max(1.0);
    SystemVideoStage {
        x: x as f32,
        y: y as f32,
        width: width as f32,
        height: height as f32,
        scale_x: (video_w / native_w) as f32,
        scale_y: (video_h / native_h) as f32,
        offset_x: ((width - video_w) / 2.0).round() as f32,
        offset_y: ((height - video_h) / 2.0).round() as f32,
    }
}

impl PlayerLike for SystemPlayer {
    fn pause(&self) {
        self.paused.store(true, Ordering::SeqCst);
        if let Err(error) = unsafe { self.cell.engine.Pause() } {
            log::warn!("[WmPresenter] Pause 失败: {error}");
        }
    }

    fn resume(&self) {
        self.paused.store(false, Ordering::SeqCst);
        if let Err(error) = unsafe { self.cell.engine.Play() } {
            log::warn!("[WmPresenter] Play 失败: {error}");
        }
        // 暂停/恢复会重置引擎的实际速率：把期望值补回去（就绪前由节流补试兜底）。
        self.applied_rate.store(1.0f64.to_bits(), Ordering::SeqCst);
        self.try_apply_rate();
    }

    fn is_paused(&self) -> bool {
        self.paused.load(Ordering::SeqCst)
    }

    fn position_secs(&self) -> f64 {
        unsafe { self.cell.engine.GetCurrentTime() }
    }

    fn duration_secs(&self) -> f64 {
        // 未就绪时引擎给 NaN/0：统一成 0，免得 UI 算出 NaN 进度。
        let duration = unsafe { self.cell.engine.GetDuration() };
        if duration.is_finite() { duration } else { 0.0 }
    }

    fn seek(&self, seconds: f64) {
        if let Err(error) = unsafe { self.cell.engine.SetCurrentTime(seconds.max(0.0)) } {
            log::warn!("[WmPresenter] seek {seconds:.1}s 失败: {error}");
        }
    }

    fn set_rate(&self, rate: f64) {
        // 只记期望值：引擎暂时不认（metadata 未就绪）时由 [`try_apply_rate`]
        // 在就绪后补发，UI 上的倍速与实际播放不会被静默拆开。
        self.rate.store(rate.to_bits(), Ordering::SeqCst);
        self.try_apply_rate();
    }

    fn rate(&self) -> f64 {
        f64::from_bits(self.rate.load(Ordering::SeqCst))
    }

    fn set_volume(&self, volume: f32) {
        unsafe {
            self.cell
                .engine
                .SetVolume(f64::from(volume.clamp(0.0, 1.0)))
        }
        .ok();
    }

    fn set_muted(&self, muted: bool) {
        unsafe { self.cell.engine.SetMuted(muted) }.ok();
    }

    fn dims(&self) -> (usize, usize) {
        let (width, height) = self.native_size();
        (width as usize, height as usize)
    }

    fn attach(&self, host: &HostWindow) {
        let mut state = self.state.lock().expect("视频位锁 poisoned");
        state.scale = host.scale_factor;
        if state.layer.is_none() {
            state.layer = view::current_layer();
        }
    }

    fn submit_stage(&self, bounds: Option<PresenterBounds>, fit: PresenterFit, visible: bool) {
        *self.staged.lock().expect("舞台锁 poisoned") = Staged {
            bounds,
            fit,
            visible,
        };
        self.apply_stage();
    }

    fn is_system(&self) -> bool {
        true
    }

    fn detach(&self) {
        self.staged.lock().expect("舞台锁 poisoned").visible = false;
        self.apply_stage();
    }

    fn preload_next(&self, _vid: &str, _definition: u32, _on_stall: Option<StallHook>) -> bool {
        // 引擎没有播放列表：预加载/接力由壳侧回落到重建（对齐 Tauri 卸载重挂）。
        false
    }

    fn advance_preloaded(&self, _vid: &str) -> bool {
        false
    }
}

impl VideoPresenter for SystemPlayer {
    fn submit(&self, bounds: Option<PresenterBounds>, fit: PresenterFit) {
        self.submit_stage(bounds, fit, true);
    }

    fn set_visible(&self, visible: bool) {
        self.staged.lock().expect("舞台锁 poisoned").visible = visible;
        self.apply_stage();
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
        PlayerLike::detach(self);
    }
}

impl Drop for SystemPlayer {
    fn drop(&mut self) {
        // 字节流是本集独占的：无论引擎是不是已经换源，都要 Close 让在途读退出
        // （否则旧集的读线程会一直挂到 30s 超时，每集漏一个线程）。
        if let Some(stream) = &self._stream {
            unsafe { stream.Close() }.ok();
        }
        // 只有「引擎现在放的还是我这一集」才停它：换源之后引擎已经在放新的
        // 一集（旧句柄这时才被壳丢掉），停它等于把新集也停了。
        if self.cell.epoch.load(Ordering::SeqCst) != self.epoch {
            return;
        }
        unsafe {
            self.cell.engine.Pause().ok();
            self.cell.engine.Shutdown().ok();
        }
    }
}
