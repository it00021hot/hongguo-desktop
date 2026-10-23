//! Windows 系统播放器：MediaEngine frame-server 模式的 [`PlayerLike`] /
//! [`VideoPresenter`] 实现。
//!
//! 与 macOS 实现的 `player.rs` 对位：引擎自己解码、自己出声；本层只做控制面
//! （播放/暂停/seek/倍速/音量）、帧泵与舞台几何。与 macOS 的差异在画面通路：
//! 引擎经 `TransferVideoFrame` 把帧写进渲染器设备上的 BGRA 纹理，视频 quad
//! （vendor 补丁）把它与 GPUI 内容画进**同一张渲染目标**——没有独立合成层，
//! 缩放/黑边/窗口形态全部是同画布语义（对齐 nsh-platform-windows 的 video）。
//!
//! **每帧泵**（[`SystemPlayer::present_frame`]，由壳的 `submit_stage` 每帧带进
//! 来，即 vsync 节奏）：`OnVideoStreamTick` 报新帧才 `TransferVideoFrame`，
//! 暂停自然停在最后一帧。
//!
//! **每集一个新引擎**（对齐 nsh 的 video 纪律）：换源
//! （`SetSourceFromByteStream`）只换源不关旧源，旧解码器的显存帧池与 CPU 参考
//! 帧逐集滞留；`IMFMediaEngine` 没有「只关源」的 API（`Stop` 属 MediaSession）。
//! 每集新引擎 + Drop 里显式 `Shutdown`，旧解码器随引擎一起释放；解码设备是
//! 渲染器那台（[`mf::device_manager`] 进程级共享），新建引擎不再各自付设备的
//! 开销（2026-10-22 实测：换源复用 +49.9MB private/集；共享设备 + 新引擎后走平）。

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use gpui_pre_windows::video_quad::{VideoQuad, VideoRect};
use hongguo_player_api::{
    HostWindow, PlaySource, PlayerEvent, PlayerLike, PresenterBounds, PresenterFit, StallHook,
    VideoPresenter,
};
use tokio::sync::mpsc;
use windows::Win32::Foundation::{RECT, S_OK};
use windows::Win32::Graphics::Direct3D11::ID3D11Texture2D;
use windows::Win32::Media::MediaFoundation::{
    IMFByteStream, IMFMediaEngine, IMFMediaEngineEx, IMFMediaEngineNotify,
    MF_MEDIA_ENGINE_STATISTIC_FRAMES_DROPPED, MF_MEDIA_ENGINE_STATISTIC_FRAMES_RENDERED, MFARGB,
};
use windows::core::Interface;

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

/// 播放器自己持有的帧纹理：按（设备代际, 原生尺寸）失效重建。
struct FrameTexture {
    texture: ID3D11Texture2D,
    size: (u32, u32),
    generation: u64,
}

pub struct SystemPlayer {
    engine: IMFMediaEngine,
    extended: IMFMediaEngineEx,
    /// 视频 quad：帧的落点（渲染器每帧从这里取纹理画进渲染目标）。
    quad: VideoQuad,
    /// 在线流的字节流（引擎持有引用，这里只是明确保活）；本地文件为 None。
    _stream: Option<IMFByteStream>,
    paused: AtomicBool,
    rate: AtomicU64,
    staged: Mutex<Staged>,
    /// 窗口 DPI 缩放：GPUI 逻辑像素 → 物理像素。
    scale: Mutex<f32>,
    /// 帧纹理（按需建）；None = 还没有可用画面。
    frame: Mutex<Option<FrameTexture>>,
    /// 已探得的原生视频尺寸（打包 w<<32|h；metadata 未就绪为 0）。
    announced: AtomicU64,
    /// 期望倍速（用户偏好）与实际生效的倍速：metadata 就绪前引擎一律报「不支持」。
    applied_rate: AtomicU64,
    rate_retry_at: Mutex<Instant>,
    metrics_at: Mutex<Instant>,
    /// 落点是否已提交（avoid 每帧重复 set_frame；可见性翻转时清帧用）。
    presented: AtomicBool,
}

// SAFETY: Media Foundation 与 DirectComposition 的接口都是自由线程的；本结构的
// 所有方法只在 GPUI 主线程调用（`PlayerLike` 的调用点全在视图/事件回调里），
// `Drop` 在任意线程只做 Release。与 macOS 侧 `SendRetained` 同一套约定。
unsafe impl Send for SystemPlayer {}
unsafe impl Sync for SystemPlayer {}

/// 打开一集系统播放器。返回 `None` 表示引擎起不来——调用方回落到自研上屏。
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
    let quad = view::video()?;
    let device = quad.device()?;
    let generation = quad.generation();
    let notify: IMFMediaEngineNotify =
        events::Notify::new(Arc::new(events::NotifyShared::new(events))).into();
    let (engine, extended) = match mf::create_engine(&device, generation, &notify) {
        Ok(engine) => engine,
        Err(error) => {
            log::error!("[WmPresenter] MediaEngine 创建失败: {error}");
            return None;
        }
    };
    let engine_at = Instant::now();
    let byte_stream = match attach_source(&engine, &extended, source, on_stall) {
        Ok(byte_stream) => byte_stream,
        Err(error) => {
            // 引擎已挂了半个源：显式停掉再交给 Drop 释放，别留着自收。
            unsafe {
                engine.Pause().ok();
                engine.Shutdown().ok();
            }
            log::error!("[WmPresenter] {error}");
            return None;
        }
    };
    finish_open(
        engine,
        extended,
        quad,
        byte_stream,
        start_secs,
        rate,
        opened_at,
        engine_at,
    )
}

/// 把一集的源挂到引擎上：在线流走字节流，本地文件走 URL。
fn attach_source(
    engine: &IMFMediaEngine,
    extended: &IMFMediaEngineEx,
    source: PlaySource,
    on_stall: Option<StallHook>,
) -> Result<Option<IMFByteStream>, String> {
    match source {
        PlaySource::Online { vid, definition } => {
            let stream = stream::open(vid.clone(), definition, on_stall);
            let url = windows::core::BSTR::from(format!("https://hongguo.local/stream/{vid}.mp4"));
            unsafe { extended.SetSourceFromByteStream(&stream, &url) }
                .map_err(|error| format!("设置字节流源失败（{vid}@{definition}）: {error}"))?;
            Ok(Some(stream))
        }
        PlaySource::Local { path } => {
            let url = windows::core::BSTR::from(format!(
                "file:///{}",
                path.to_string_lossy().replace('\\', "/")
            ));
            unsafe { engine.SetSource(&url) }
                .map_err(|error| format!("设置本地文件源失败（{}）: {error}", path.display()))?;
            Ok(None)
        }
    }
}

/// 起播收尾：定位起点、起播、组装句柄。
#[expect(
    clippy::too_many_arguments,
    reason = "起播参数面一一对应，不拆结构体反而失真"
)]
fn finish_open(
    engine: IMFMediaEngine,
    extended: IMFMediaEngineEx,
    quad: VideoQuad,
    byte_stream: Option<IMFByteStream>,
    start_secs: f64,
    rate: f64,
    opened_at: Instant,
    engine_at: Instant,
) -> Option<Arc<dyn PlayerLike>> {
    unsafe {
        // 续播起点：metadata 就绪后引擎会按这个位置出画。
        if start_secs > SEEK_EPSILON_SECS {
            engine.SetCurrentTime(start_secs).ok();
        }
        if let Err(error) = engine.Play() {
            log::error!("[WmPresenter] Play 失败: {error}");
            engine.Shutdown().ok();
            return None;
        }
    }
    let play_at = Instant::now();
    let player = Arc::new(SystemPlayer {
        engine,
        extended,
        quad,
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
        scale: Mutex::new(1.0),
        frame: Mutex::new(None),
        announced: AtomicU64::new(0),
        metrics_at: Mutex::new(Instant::now()),
        presented: AtomicBool::new(false),
    });
    player.try_apply_rate();
    log::info!(
        "[WmPresenter] 系统播放器已打开（起点 {start_secs:.1}s，期望 {rate:.2}x）：建引擎 {:.0}ms / 挂源到起播 {:.0}ms",
        engine_at.duration_since(opened_at).as_secs_f64() * 1000.0,
        play_at.duration_since(engine_at).as_secs_f64() * 1000.0,
    );
    Some(player)
}

impl SystemPlayer {
    /// 原生视频尺寸（metadata 未就绪时为 (0, 0)）。
    ///
    /// **一次 item 内尺寸不变**：metadata 就绪后缓存住（打包进 `announced`），
    /// 省掉每帧一次 `GetNativeVideoSize` 的 COM 往返。
    fn native_size(&self) -> (u32, u32) {
        let announced = self.announced.load(Ordering::SeqCst);
        if announced != 0 {
            return ((announced >> 32) as u32, announced as u32);
        }
        let mut width = 0u32;
        let mut height = 0u32;
        unsafe {
            self.engine
                .GetNativeVideoSize(Some(&mut width as *mut u32), Some(&mut height as *mut u32))
                .ok();
        }
        if width != 0 && height != 0 {
            self.announced.store(
                (u64::from(width) << 32) | u64::from(height),
                Ordering::SeqCst,
            );
        }
        (width, height)
    }

    /// 帧泵：壳每帧经 `submit_stage` 进来一次（vsync 节奏）。
    ///
    /// ① `OnVideoStreamTick` 报新帧才 `TransferVideoFrame` 进纹理（暂停自然
    /// 停在最后一帧）；② 按舞台几何把纹理落点提交给视频 quad；③ 倍速补发与
    /// 统计取证顺路。
    fn present_frame(&self) {
        self.try_apply_rate();
        let staged = *self.staged.lock().expect("舞台锁 poisoned");
        if !staged.visible {
            if self.presented.swap(false, Ordering::SeqCst) {
                self.quad.clear_frame();
            }
            return;
        }
        let (width, height) = self.native_size();
        let Some(bounds) = staged
            .bounds
            .filter(|bounds| !bounds.is_empty() && width > 0)
        else {
            return;
        };
        if !self.ensure_frame_texture(width, height) {
            return;
        }
        let frame = self.frame.lock().expect("帧纹理锁 poisoned");
        let Some(frame) = frame.as_ref() else {
            return;
        };
        let mut pts = 0i64;
        // SAFETY: a call on a live engine; S_FALSE（无新帧）是成功码，windows
        // crate 不区分成功码，所以走裸 vtable（对齐 nsh 的 tick）。
        let fresh = unsafe {
            (Interface::vtable(&self.engine).OnVideoStreamTick)(self.engine.as_raw(), &mut pts)
        } == S_OK;
        if fresh {
            let rect = RECT {
                left: 0,
                top: 0,
                right: width as i32,
                bottom: height as i32,
            };
            let black = MFARGB {
                rgbBlue: 0,
                rgbGreen: 0,
                rgbRed: 0,
                rgbAlpha: 255,
            };
            // SAFETY: 引擎在设备管理器那台设备上做 NV12→BGRA 转换写进纹理；
            // 设备开多线程保护，与渲染器并发安全。
            unsafe {
                self.engine
                    .TransferVideoFrame(&frame.texture, None, &rect, Some(&black))
                    .ok();
            }
        }
        let scale = *self.scale.lock().expect("DPI 锁 poisoned");
        let dest = geometry(bounds, staged.fit, scale, (width, height));
        self.quad
            .set_frame(frame.texture.clone(), (width, height), dest);
        self.presented.store(true, Ordering::SeqCst);
        self.log_metrics();
    }

    /// 帧纹理就位：没有或已失效（设备代际变了/尺寸变了）就重建。
    fn ensure_frame_texture(&self, width: u32, height: u32) -> bool {
        let generation = self.quad.generation();
        {
            let frame = self.frame.lock().expect("帧纹理锁 poisoned");
            if frame
                .as_ref()
                .is_some_and(|f| f.size == (width, height) && f.generation == generation)
            {
                return true;
            }
        }
        let texture = match self.quad.create_frame_texture(width, height) {
            Ok(texture) => texture,
            Err(error) => {
                log::warn!("[WmPresenter] 帧纹理创建失败（{width}×{height}）: {error}");
                return false;
            }
        };
        *self.frame.lock().expect("帧纹理锁 poisoned") = Some(FrameTexture {
            texture,
            size: (width, height),
            generation,
        });
        log::info!("[WmPresenter] 帧纹理就绪 {width}×{height}（代际 {generation}）");
        true
    }

    /// 把期望倍速补发给引擎（引擎只在 metadata 就绪后才认这个值）。
    ///
    /// 节流到 [`RATE_RETRY_EVERY`]：`present_frame` 每帧都会调它，而
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
        if !mf::rate_supported(&self.extended, desired) {
            log::debug!("[WmPresenter] 引擎暂不支持 {desired}x（metadata 未就绪？）");
            return;
        }
        match unsafe { self.engine.SetPlaybackRate(desired) } {
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
        let rendered = mf::statistic(&self.extended, MF_MEDIA_ENGINE_STATISTIC_FRAMES_RENDERED);
        let dropped = mf::statistic(&self.extended, MF_MEDIA_ENGINE_STATISTIC_FRAMES_DROPPED);
        log::info!(
            "[WmPresenter][metrics] 5s rendered={rendered:?} dropped={dropped:?} rate={:.2}x pos={:.2}s",
            self.rate(),
            unsafe { self.engine.GetCurrentTime() }
        );
    }
}

/// 舞台几何换算：视频原生尺寸 → 窗口内的视频矩形（物理像素；`contain` 留
/// 黑边、`cover` 溢出由 GPUI 裁——视频 quad 在场景之下，黑边就是舞台背景）。
///
/// **落点对齐到整物理像素**：分数坐标 + 双线性采样会让画面最外圈半个像素
/// 采到纹理之外，叠出一条浅色细线。取整后视频与纹理 1:1 映射，边缘像素完整；
/// 舞台原点同样取整，窗口拖动/缩放时不会出现 1px 抖动。
fn geometry(
    bounds: PresenterBounds,
    fit: PresenterFit,
    scale: f32,
    (native_w, native_h): (u32, u32),
) -> VideoRect {
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
    VideoRect {
        x: ((width - video_w) / 2.0 + x).round() as f32,
        y: ((height - video_h) / 2.0 + y).round() as f32,
        width: video_w as f32,
        height: video_h as f32,
    }
}

impl PlayerLike for SystemPlayer {
    fn pause(&self) {
        self.paused.store(true, Ordering::SeqCst);
        if let Err(error) = unsafe { self.engine.Pause() } {
            log::warn!("[WmPresenter] Pause 失败: {error}");
        }
    }

    fn resume(&self) {
        self.paused.store(false, Ordering::SeqCst);
        if let Err(error) = unsafe { self.engine.Play() } {
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
        unsafe { self.engine.GetCurrentTime() }
    }

    fn duration_secs(&self) -> f64 {
        // 未就绪时引擎给 NaN/0：统一成 0，免得 UI 算出 NaN 进度。
        let duration = unsafe { self.engine.GetDuration() };
        if duration.is_finite() { duration } else { 0.0 }
    }

    fn seek(&self, seconds: f64) {
        if let Err(error) = unsafe { self.engine.SetCurrentTime(seconds.max(0.0)) } {
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
        unsafe { self.engine.SetVolume(f64::from(volume.clamp(0.0, 1.0))) }.ok();
    }

    fn set_muted(&self, muted: bool) {
        unsafe { self.engine.SetMuted(muted) }.ok();
    }

    fn dims(&self) -> (usize, usize) {
        let (width, height) = self.native_size();
        (width as usize, height as usize)
    }

    fn attach(&self, host: &HostWindow) {
        *self.scale.lock().expect("DPI 锁 poisoned") = host.scale_factor;
    }

    fn submit_stage(&self, bounds: Option<PresenterBounds>, fit: PresenterFit, visible: bool) {
        *self.staged.lock().expect("舞台锁 poisoned") = Staged {
            bounds,
            fit,
            visible,
        };
        // 每帧泵就挂在这里：壳的舞台渲染每帧都调（vsync 节奏）。
        self.present_frame();
    }

    fn is_system(&self) -> bool {
        true
    }

    fn detach(&self) {
        self.staged.lock().expect("舞台锁 poisoned").visible = false;
        self.present_frame();
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
        self.present_frame();
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
        // 显式 Shutdown：不调的话引擎的媒体会话（连同旧源的解码器显存帧池）
        // 会被 MF 工作队列挂活到进程结束——这正是此前逐集滞留的根源之一。
        unsafe {
            self.engine.Pause().ok();
            self.engine.Shutdown().ok();
        }
        // 字节流是本集独占的：Close 让在途读尽快退出（否则旧集的读线程会
        // 一直挂到 30s 超时，每集漏一个线程）。
        if let Some(stream) = &self._stream {
            unsafe { stream.Close() }.ok();
        }
        // 本集的帧别留在视频 quad 上（下一集首帧前舞台显示封面，不该露出旧画面）。
        self.quad.clear_frame();
    }
}
