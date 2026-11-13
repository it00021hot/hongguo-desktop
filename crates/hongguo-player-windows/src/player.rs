//! 内置视频会话的音轨播放器；视频帧与尺寸由 direct 模块独立提供。

use super::{events, mf, view};
use gpui_pre_windows::video_quad::{VideoQuad, VideoRect};
use hongguo_player_api::{HostWindow, PlayerEvent, PlayerLike, PresenterBounds, PresenterFit};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use windows::Win32::Media::MediaFoundation::{
    IMFByteStream, IMFMediaEngine, IMFMediaEngineEx, IMFMediaEngineNotify,
};

const RATE_RETRY_EVERY: Duration = Duration::from_millis(250);
const SEEK_EPSILON_SECS: f64 = 0.5;

pub(super) struct SystemPlayer {
    pub(super) engine: IMFMediaEngine,
    extended: IMFMediaEngineEx,
    pub(super) quad: VideoQuad,
    stream: IMFByteStream,
    pub(super) paused: AtomicBool,
    rate: AtomicU64,
    applied_rate: AtomicU64,
    rate_retry_at: Mutex<Instant>,
    pub(super) scale: Mutex<f32>,
}

// SAFETY: MediaEngine control and the quad are used on the owning GPUI thread.
unsafe impl Send for SystemPlayer {}
unsafe impl Sync for SystemPlayer {}

pub(super) fn open_audio(
    byte_stream: IMFByteStream,
    label: &str,
    start_secs: f64,
    rate: f64,
    events: mpsc::Sender<PlayerEvent>,
) -> Option<Arc<SystemPlayer>> {
    let quad = view::video()?;
    let notify: IMFMediaEngineNotify =
        events::Notify::new(Arc::new(events::NotifyShared::audio_only(events))).into();
    let (engine, extended) = mf::create_engine(&notify)
        .map_err(|error| log::error!("[WmDirect] 音轨引擎创建失败: {error}"))
        .ok()?;
    let configured = unsafe {
        let url = windows::core::BSTR::from(format!("https://hongguo.local/direct/{label}.mp4"));
        extended
            .SetSourceFromByteStream(&byte_stream, &url)
            .and_then(|()| {
                if start_secs > SEEK_EPSILON_SECS {
                    engine.SetCurrentTime(start_secs)?;
                }
                Ok(())
            })
    };
    if let Err(error) = configured {
        unsafe { engine.Shutdown() }.ok();
        log::error!("[WmDirect] 设置音轨源失败: {error}");
        return None;
    }
    let player = Arc::new(SystemPlayer {
        engine,
        extended,
        quad,
        stream: byte_stream,
        paused: AtomicBool::new(false),
        rate: AtomicU64::new(rate.to_bits()),
        applied_rate: AtomicU64::new(1.0f64.to_bits()),
        rate_retry_at: Mutex::new(
            Instant::now()
                .checked_sub(RATE_RETRY_EVERY)
                .unwrap_or_else(Instant::now),
        ),
        scale: Mutex::new(1.0),
    });
    player.try_apply_rate();
    Some(player)
}

impl SystemPlayer {
    pub(super) fn play_engine(&self) {
        if let Err(error) = unsafe { self.engine.Play() } {
            log::warn!("[WmDirect] Play 失败: {error}");
        }
        self.applied_rate.store(1.0f64.to_bits(), Ordering::SeqCst);
        self.try_apply_rate();
    }

    pub(super) fn try_apply_rate(&self) {
        let desired = self.rate();
        if (desired - f64::from_bits(self.applied_rate.load(Ordering::SeqCst))).abs()
            <= f64::EPSILON
        {
            return;
        }
        {
            let mut at = self.rate_retry_at.lock().expect("倍速锁 poisoned");
            if at.elapsed() < RATE_RETRY_EVERY {
                return;
            }
            *at = Instant::now();
        }
        if !mf::rate_supported(&self.extended, desired) {
            return;
        }
        match unsafe { self.engine.SetPlaybackRate(desired) } {
            Ok(()) => {
                self.applied_rate.store(desired.to_bits(), Ordering::SeqCst);
            }
            Err(error) => log::warn!("[WmDirect] 设置 {desired}x 失败: {error}"),
        }
    }
}

pub(super) fn geometry(
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
    let ratio = match fit {
        PresenterFit::Contain => (width / native_w).min(height / native_h),
        PresenterFit::Cover => (width / native_w).max(height / native_h),
    };
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
        unsafe { self.engine.Pause() }
            .map_err(|error| log::warn!("[WmDirect] Pause 失败: {error}"))
            .ok();
    }
    fn resume(&self) {
        self.paused.store(false, Ordering::SeqCst);
        self.play_engine();
    }
    fn is_paused(&self) -> bool {
        self.paused.load(Ordering::SeqCst)
    }
    fn position_secs(&self) -> f64 {
        unsafe { self.engine.GetCurrentTime() }
    }
    fn duration_secs(&self) -> f64 {
        let duration = unsafe { self.engine.GetDuration() };
        if duration.is_finite() { duration } else { 0.0 }
    }
    fn seek(&self, seconds: f64) {
        unsafe { self.engine.SetCurrentTime(seconds.max(0.0)) }
            .map_err(|error| log::warn!("[WmDirect] seek 失败: {error}"))
            .ok();
    }
    fn set_rate(&self, rate: f64) {
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
        (0, 0)
    }
    fn attach(&self, host: &HostWindow) {
        *self.scale.lock().expect("DPI 锁 poisoned") = host.scale_factor;
    }
    fn submit_stage(&self, _: Option<PresenterBounds>, _: PresenterFit, _: bool) {}
    fn detach(&self) {}
}

impl Drop for SystemPlayer {
    fn drop(&mut self) {
        unsafe {
            self.engine.Pause().ok();
            self.engine.Shutdown().ok();
            self.stream.Close().ok();
        }
    }
}
