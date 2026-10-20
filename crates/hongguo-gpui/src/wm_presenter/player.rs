//! Windows 系统播放器：MediaEngine 的 [`PlayerLike`] / [`VideoPresenter`] 实现。
//!
//! 与 macOS 的 `av_presenter/player.rs` 对位：引擎自己解码、自己出声、自己把画面
//! 合成进 DirectComposition 视觉；本层只做控制面（播放/暂停/seek/倍速/音量）、
//! 舞台几何下发与引擎统计取证。窗口形态切换不重建引擎，只改视觉的
//! offset/scale/clip（[`SystemVideoStage`]）。
//!
//! 引擎渲染分辨率：rendering mode 默认把视频画进一块 640×480 的固定表面（spike
//! 实测），所以每次拿到原生尺寸就用 `UpdateVideoStream` 把落点重设成视频原生
//! 尺寸——画质与"是否全屏"解耦，缩放交给 DComp。

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gpui_kit::Window;
use gpui_pre_windows::system_video::{SystemVideoLayer, SystemVideoStage};
use hongguo_playback::{PlaySource, PlayerEvent, PresenterBounds, PresenterFit, VideoPresenter};
use tokio::sync::mpsc;
use windows::Win32::Foundation::RECT;
use windows::Win32::Media::MediaFoundation::{
    IMFByteStream, IMFMediaEngine, IMFMediaEngineEx, IMFMediaEngineNotify,
    MF_MEDIA_ENGINE_STATISTIC_FRAMES_DROPPED, MF_MEDIA_ENGINE_STATISTIC_FRAMES_RENDERED,
};

use super::{events, mf, stream, view};
use crate::home::player::{PlayerLike, StallHook};

/// 引擎统计取证周期（`HONGGUO_WM_METRICS=1` 才打）。
const METRICS_EVERY: Duration = Duration::from_secs(5);
/// 续播起点低于这个秒数时不 seek（与 macOS 侧同参：0 附近的起点没意义）。
const SEEK_EPSILON_SECS: f64 = 0.5;
/// 倍速补发的节流间隔：`IsPlaybackRateSupported` 是一次 COM 往返，逐帧问太贵。
const RATE_RETRY_EVERY: Duration = Duration::from_millis(250);

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

pub struct SystemPlayer {
    engine: IMFMediaEngine,
    extended: IMFMediaEngineEx,
    /// 在线流的字节流（引擎持有引用，这里只是明确保活）；本地文件为 None。
    _stream: Option<IMFByteStream>,
    /// 引擎持有 notify 的引用；这里再持有一次，语义清晰。
    _notify: IMFMediaEngineNotify,
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
pub fn open(
    source: PlaySource,
    start_secs: f64,
    rate: f64,
    events: mpsc::Sender<PlayerEvent>,
    on_stall: Option<StallHook>,
) -> Option<Arc<dyn PlayerLike>> {
    let layer = view::current_layer()?;
    let notify: IMFMediaEngineNotify = events::Notify::new(events).into();
    let (engine, extended) = match mf::create_engine(layer.render_target(), &notify) {
        Ok(engine) => engine,
        Err(error) => {
            log::error!("[WmPresenter] MediaEngine 创建失败: {error}");
            return None;
        }
    };

    let byte_stream = match source {
        PlaySource::Online { vid, definition } => {
            let stream = stream::open(vid.clone(), definition, on_stall);
            let url = windows::core::BSTR::from(format!("https://hongguo.local/stream/{vid}.mp4"));
            if let Err(error) = unsafe { extended.SetSourceFromByteStream(&stream, &url) } {
                log::error!("[WmPresenter] 设置字节流源失败（{vid}@{definition}）: {error}");
                return None;
            }
            Some(stream)
        }
        PlaySource::Local { path } => {
            let url = windows::core::BSTR::from(format!(
                "file:///{}",
                path.to_string_lossy().replace('\\', "/")
            ));
            if let Err(error) = unsafe { engine.SetSource(&url) } {
                log::error!(
                    "[WmPresenter] 设置本地文件源失败（{}）: {error}",
                    path.display()
                );
                return None;
            }
            None
        }
    };

    unsafe {
        // 续播起点：metadata 就绪后引擎会按这个位置出画。
        if start_secs > SEEK_EPSILON_SECS {
            engine.SetCurrentTime(start_secs).ok();
        }
        if let Err(error) = engine.Play() {
            log::error!("[WmPresenter] Play 失败: {error}");
            return None;
        }
    }
    let player = Arc::new(SystemPlayer {
        engine,
        extended,
        _stream: byte_stream,
        _notify: notify,
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
    log::info!("[WmPresenter] 系统播放器已打开（起点 {start_secs:.1}s，期望 {rate:.2}x）");
    Some(player)
}

impl SystemPlayer {
    /// 原生视频尺寸（metadata 未就绪时为 (0, 0)）。
    fn native_size(&self) -> (u32, u32) {
        let mut width = 0u32;
        let mut height = 0u32;
        unsafe {
            self.engine
                .GetNativeVideoSize(Some(&mut width as *mut u32), Some(&mut height as *mut u32))
                .ok();
        }
        (width, height)
    }

    /// 把引擎的渲染落点设成视频原生尺寸（rendering mode 默认 640×480 固定表面）。
    fn announce_native_size(&self, width: u32, height: u32) {
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
        match unsafe { self.extended.UpdateVideoStream(None, Some(&rect), None) } {
            Ok(()) => log::info!("[WmPresenter] 渲染落点设为原生 {width}×{height}"),
            Err(error) => {
                log::warn!("[WmPresenter] UpdateVideoStream({width}×{height}) 失败: {error}")
            }
        }
    }

    /// 提交舞台几何（幂等；几何没变时 [view::submit] 内部直接返回）。
    fn apply_stage(&self) {
        let staged = *self.staged.lock().expect("舞台锁 poisoned");
        let (native_w, native_h) = self.native_size();
        self.announce_native_size(native_w, native_h);
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
        let enabled = std::env::var("HONGGUO_WM_METRICS").is_ok_and(|value| value == "1");
        if !enabled {
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

/// 舞台几何换算：视频原生尺寸 → 舞台内的位置/缩放（`contain` 留黑边、
/// `cover` 溢出裁切，裁剪在视觉树里由裁剪视觉完成）。
fn geometry(
    bounds: PresenterBounds,
    fit: PresenterFit,
    scale: f32,
    (native_w, native_h): (u32, u32),
) -> SystemVideoStage {
    let scale = f64::from(scale);
    let width = f64::from(bounds.width) * scale;
    let height = f64::from(bounds.height) * scale;
    let native_w = f64::from(native_w);
    let native_h = f64::from(native_h);
    let contain = (width / native_w).min(height / native_h);
    let cover = (width / native_w).max(height / native_h);
    let ratio = match fit {
        PresenterFit::Contain => contain,
        PresenterFit::Cover => cover,
    };
    SystemVideoStage {
        x: (f64::from(bounds.x) * scale) as f32,
        y: (f64::from(bounds.y) * scale) as f32,
        width: width as f32,
        height: height as f32,
        scale_x: ratio as f32,
        scale_y: ratio as f32,
        offset_x: ((width - native_w * ratio) / 2.0) as f32,
        offset_y: ((height - native_h * ratio) / 2.0) as f32,
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

    fn attach(&self, window: &Window) {
        let mut state = self.state.lock().expect("视频位锁 poisoned");
        state.scale = window.scale_factor();
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
        unsafe {
            self.engine.Pause().ok();
            self.engine.Shutdown().ok();
        }
        // 字节流是引擎的最后一份引用之一：显式 Close 让在途读尽快退出。
        if let Some(stream) = &self._stream {
            unsafe { stream.Close() }.ok();
        }
    }
}
