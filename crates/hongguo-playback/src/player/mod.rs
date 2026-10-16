//! 引擎编排与对外句柄。
//!
//! [`open`] 做完所有贵的前置（读样本表、开解码器、起音频设备）才返回句柄；
//! 随后的播放由两个任务自治：音频喂送（[`audio_feeder`](feeder::audio_feeder)）与视频循环
//! （[`video::run`](crate::video::run)）。宿主只拿 [`PlayerHandle`]：取帧、暂停/继续、倍速/
//! 音量/静音、seek、读进度、等事件。
//!
//! 控制面语义与 Tauri 版一致：倍速 0.25–4（音频 WSOLA 变速不变调，时钟乘
//! rate）、音量 0–1 与静音独立、seek 冲解码器与设备缓冲后从关键帧续播。

mod event;
mod feeder;
mod handle;
mod open;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use tokio::sync::mpsc;

use crate::audio::Clock;

pub use event::{OpenOptions, PlaySource, PlayerEvent};
pub use handle::{MAX_RATE, MIN_RATE, PlayerHandle};
pub use open::open;

/// 内部共享态：任务族与句柄的共同视图。
pub(crate) struct Shared {
    pub paused: AtomicBool,
    pub shutdown: AtomicBool,
    /// 主时钟，**可替换**：音频设备迟到接管、音频播完切挂钟，都会换它，
    /// 画面与进度读数自动跟上。
    pub clock: parking_lot::Mutex<Arc<Clock>>,
    /// seek 广播：(代次, 目标媒体秒)。代次单调递增；每个消费者（视频循环/
    /// 音频喂送）各自记住处理到的代次。**不能是单槽 take**——先到的消费者
    /// 会把请求「抢走」，另一位永远不知道：音频不重定位、它名下的冲流也
    /// 永远收不了口（回调一直丢弃 → 环刚填满就被清空 → 水位无限抖动、
    /// 时钟冻结，用户看到的就是「拖进度一直缓冲中」）。
    pub seek_broadcast: parking_lot::Mutex<(u64, f64)>,
    /// 读取代次：seek 时 +1，堵在旧位置上的 serve 读取据此尽早退场
    /// （注销读者登记，让填充调度器立刻转向新位置）。
    pub read_epoch: AtomicU64,
    /// 缓冲徽标的 UI 态（Buffering/Ready 事件只按它翻转发）。与设备的
    /// starved 位解耦：seek 会先解除 starved 让回调跑起来收冲流的口，
    /// 若靠 starved 翻转发事件，这条路径上 Ready 会永远丢。
    pub buffered_ui: AtomicBool,
    /// 音量（f32 位型存储，0–1）与静音：喂给设备回调，同时是音频设备迟到
    /// 接管时的初值来源。
    pub volume: AtomicU32,
    pub muted: AtomicBool,
    pub latest: parking_lot::Mutex<Option<hongguo_media_platform::platform::video::VideoFrame>>,
    pub dims: parking_lot::Mutex<(usize, usize)>,
    pub duration_secs: parking_lot::Mutex<f64>,
    pub events: mpsc::Sender<PlayerEvent>,
}

impl Shared {
    /// 当前播放位置（主时钟）。视频循环与进度上报都走这里，时钟被换掉后
    /// 各处的读数自动跟上。
    pub(crate) fn now(&self) -> f64 {
        self.clock.lock().now()
    }

    /// 换主时钟（音频迟到接管/音频播完切挂钟时用）。
    pub(crate) fn set_clock(&self, clock: Arc<Clock>) {
        *self.clock.lock() = clock;
    }

    /// 消费者视角取新 seek：代次比 `last_gen` 新则返回目标并就地记账。
    /// 每个消费者独立记账（单槽 take 会被先到者抢走，见 seek_broadcast）。
    pub(crate) fn poll_seek(&self, last_gen: &mut u64) -> Option<f64> {
        let (current, target) = *self.seek_broadcast.lock();
        if current != *last_gen {
            *last_gen = current;
            Some(target)
        } else {
            None
        }
    }

    /// 是否有比 `last_gen` 新的 seek 未处理（视频前瞻闸门用它及时让路）。
    pub(crate) fn seek_pending_since(&self, last_gen: u64) -> bool {
        self.seek_broadcast.lock().0 != last_gen
    }
    pub(crate) fn rate(&self) -> f64 {
        self.clock.lock().rate()
    }

    /// 发事件。宿主弃了（通道关闭）返回 Err，调用方应顺势退出。
    pub(crate) async fn emit(&self, event: PlayerEvent) {
        let _ = self.events.send(event).await;
    }

    /// 致命失败：通知宿主并自停。
    pub(crate) async fn fail(&self, msg: String) {
        log::warn!("[Playback] {msg}");
        self.shutdown.store(true, Ordering::Relaxed);
        self.emit(PlayerEvent::Failed(msg)).await;
    }
}
