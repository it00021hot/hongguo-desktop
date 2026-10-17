//! 播放器句柄：宿主侧控制面（帧/进度/暂停/倍速/音量/seek）与 Drop 停机序。

use std::sync::Arc;
use std::sync::atomic::Ordering;

use crate::audio::AudioOut;

use super::Shared;

/// 引擎句柄。Drop 即停（任务中止、音频设备释放）。
pub struct PlayerHandle {
    pub(super) shared: Arc<Shared>,
    /// 音频设备槽：起播时就绪则当场填入；迟到接管的设备由接管任务填进来。
    /// 句柄经它保活——晚到的流若没人持有，出了接管任务的作用域就被析构。
    pub(super) audio: Arc<parking_lot::Mutex<Option<AudioOut>>>,
    pub(super) tasks: Vec<tokio::task::JoinHandle<()>>,
}

/// 倍速范围（与 Tauri 版 playback-prefs 的 MIN_RATE/MAX_RATE 一致）。
pub const MIN_RATE: f64 = 0.25;
pub const MAX_RATE: f64 = 4.0;

impl PlayerHandle {
    /// 当前上屏帧（NV12 CVPixelBuffer，引用计数克隆）。渲染线程调用。
    /// 当前上屏帧（macOS NV12 CVPixelBuffer / Windows NV12 系统内存，引用
    /// 计数克隆）。渲染线程调用。
    pub fn latest_frame(&self) -> Option<hongguo_media_platform::platform::video::VideoFrame> {
        self.shared.latest.lock().clone()
    }

    /// 视频分辨率。
    pub fn dims(&self) -> (usize, usize) {
        *self.shared.dims.lock()
    }

    /// 时长（秒）。
    pub fn duration_secs(&self) -> f64 {
        *self.shared.duration_secs.lock()
    }

    /// 当前播放位置（秒，主时钟）。
    pub fn position_secs(&self) -> f64 {
        self.shared.now()
    }

    pub fn is_paused(&self) -> bool {
        self.shared.paused.load(Ordering::Relaxed)
    }

    /// 暂停：音频设备停消费（时钟冻结），视频循环停在当前帧。
    pub fn pause(&self) {
        self.shared.paused.store(true, Ordering::Relaxed);
        if let Some(audio) = self.audio.lock().as_ref() {
            audio.device.set_user_paused(true);
        }
    }

    pub fn resume(&self) {
        self.shared.paused.store(false, Ordering::Relaxed);
        if let Some(audio) = self.audio.lock().as_ref() {
            audio.device.set_user_paused(false);
        }
    }

    /// 当前倍速。
    pub fn rate(&self) -> f64 {
        self.shared.rate()
    }

    /// 换倍速（0.25–4）。
    ///
    /// 环里还压着旧速率内容时走**分级**（见 [`Clock::set_rate`](crate::audio::Clock::set_rate)）：WSOLA
    /// 即刻按新倍速产新内容（排在环内旧内容后面），主时钟等旧内容按旧速率
    /// 放完才换轴——立即换轴会让画面比声音抢先最多 1s×Δrate（切倍速后
    /// 音画不同步的来源）。音频经 WSOLA 变速不变调，视频侧无需感知。
    pub fn set_rate(&self, rate: f64) {
        let rate = rate.clamp(MIN_RATE, MAX_RATE);
        let drain = self
            .audio
            .lock()
            .as_ref()
            .map(|a| a.buffered_secs())
            .unwrap_or(0.0);
        self.shared.clock.lock().set_rate(rate, drain);
    }

    /// 当前音量（0–1）。
    pub fn volume(&self) -> f32 {
        f32::from_bits(self.shared.volume.load(Ordering::Relaxed))
    }

    pub fn set_volume(&self, volume: f32) {
        let volume = volume.clamp(0.0, 1.0);
        self.shared
            .volume
            .store(volume.to_bits(), Ordering::Relaxed);
        if let Some(audio) = self.audio.lock().as_ref() {
            audio.set_volume(volume);
        }
    }

    /// 静音独立于音量（对齐 Tauri 版 playback-prefs：静音后音量值保留）。
    pub fn is_muted(&self) -> bool {
        self.shared.muted.load(Ordering::Relaxed)
    }

    pub fn set_muted(&self, muted: bool) {
        self.shared.muted.store(muted, Ordering::Relaxed);
        if let Some(audio) = self.audio.lock().as_ref() {
            audio.set_muted(muted);
        }
    }

    /// 跳到目标媒体时间：时钟立即换轴，音频设备进入冲流（丢弃旧缓冲），
    /// 视频/音频循环取到请求后各自冲解码器、从目标前的关键帧/首个样本续播。
    ///
    /// 堵在旧位置上的流读取会收到代次变更信号尽快退场（注销读者登记，
    /// 填充调度器立刻转向新位置）；水位暂停会被解除——冲流的丢弃收口要靠
    /// 设备回调跑起来，暂停着的设备会让冲流永远收不了口（三方互等 = 永久
    /// 缓冲）。徽标的 Buffering 事件由喂送循环在处理 seek 时发出。
    pub fn seek(&self, secs: f64) {
        let dur = (*self.shared.duration_secs.lock()).max(0.0);
        let target = secs.clamp(0.0, dur);
        self.shared.read_epoch.fetch_add(1, Ordering::Relaxed);
        self.shared.clock.lock().reset(target);
        // 重新武装起播闸：seek 后视频要从目标前的关键帧预滚，这段时间
        // 时钟与设备都不许走（与起播同一问题，见 Shared::rearm_gate）
        self.shared.rearm_gate(target);
        if let Some(audio) = self.audio.lock().as_ref() {
            audio.begin_flush();
        }
        let mut broadcast = self.shared.seek_broadcast.lock();
        broadcast.0 += 1;
        broadcast.1 = target;
    }
}

impl Drop for PlayerHandle {
    fn drop(&mut self) {
        self.shared.shutdown.store(true, Ordering::Relaxed);
        for task in self.tasks.drain(..) {
            task.abort();
        }
        if let Some(audio) = self.audio.lock().as_ref() {
            audio.device.set_user_paused(true);
        }
    }
}
