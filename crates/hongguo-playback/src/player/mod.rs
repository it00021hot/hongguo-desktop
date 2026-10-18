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

use crate::audio::{AudioDevice, Clock};

pub use event::{OpenOptions, PlaySource, PlayerEvent};
pub use handle::{MAX_RATE, MIN_RATE, PlayerHandle};
pub use open::open;

/// 音频起播看门狗的等待上限：设备启动在 HAL 层迟迟不返回时，超过它就让
/// 画面按挂钟继续走（无声播放），绝不让界面冻在首帧。
///
/// 8 秒是「用户还能忍、但不至于误伤慢设备」的折中：正常 CoreAudio 建流在
/// 几十毫秒内完成；本机 USB 声卡卡死时栈停在 `DoIONodeConnect` 超过一分钟。
const AUDIO_START_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(8);

/// 起播闸：首帧就绪前主时钟与音频设备都不走。
///
/// 视频从**起点前的关键帧**起解，预滚到起点要付「关键帧到起点的解码时间」；
/// 这段时间若让音频设备照常推进时钟，首帧一出画就已经落后一截，而解码
/// 只够维持内容帧率、永远追不回来（1080p 续播实测首帧晚 6 秒且持续恶化）。
/// 所以时钟与设备都等「第一帧可上屏」才启动：视频循环在发首帧前调
/// [`Shared::start_clock`]，音频喂送只预填不启动设备。
pub(crate) enum ClockGate {
    /// 等首帧。音频已就绪时把（音频时钟、设备）暂存在这里，首帧一到就换轴
    /// 并起跑；无音频时就是挂钟本身，首帧一到重锚。
    Waiting {
        audio: Option<(Arc<Clock>, Arc<AudioDevice>)>,
    },
    Started,
}

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
    /// 起播锚点（媒体秒）：首帧就绪时时钟从这里起跑，预滚期间不作数。
    pub start_anchor: parking_lot::Mutex<f64>,
    /// 时钟闸：为真时 [`Shared::now`] 固定返回 [`Shared::start_anchor`]。
    ///
    /// 用在哪：起播预滚、seek、**音频迟到接管**。视频解码照常在跑（前瞻
    /// 缓冲），只是不上屏——因为锚点不动，`emit_due` 不会把帧发出去。
    /// 音频迟到时若不闸住视频，「准备音频（建流 + 预填）」这段时间画面会
    /// 自己往前走，等音频接上时它已经从更晚的位置开始，声画就永久错开
    /// 一截（倍速下这段错位还按倍速放大）。
    pub hold: AtomicBool,
    /// 起播闸（见 [`ClockGate`]）。音频就绪与视频首帧在两条任务上，
    /// 谁先到谁记；**同一把锁里完成状态迁移与时钟换轴**，避免「首帧检查
    /// 到 Waiting、音频同时存下待接管」这种两边都以为对方会动手的空档。
    pub clock_gate: parking_lot::Mutex<ClockGate>,
    /// 已接上的音频设备（没有音频时为 None）：seek 重新武装起播闸要用它
    /// 在下一帧出画前把设备停住（否则 seek 预滚期间时钟又白跑一截）。
    pub audio_device: parking_lot::Mutex<Option<Arc<AudioDevice>>>,
    /// 音频起播看门狗的截止标记（None = 没有待判定的起播）。
    pub audio_start_deadline: parking_lot::Mutex<Option<AudioStartDeadline>>,
    /// 音频初始化是否已完成（含「无音轨/设备不可用」的否定结果）。
    ///
    /// 首帧必须等它：音频设备建流 + 预填要几百毫秒，如果视频先出画并跑出
    /// 一段，音频随后才从起点接上、把主时钟 reset 回起点，`now()` 会倒退
    /// ——画面冻结到音频时钟追平（实测 2x 下 0.6s 级）。等音频就绪再开闸，
    /// 起播多等的是封面时间，播放中不再凭空卡一下。
    pub audio_ready: tokio::sync::watch::Sender<bool>,
    pub events: mpsc::Sender<PlayerEvent>,
}

/// 音频起播看门狗的记账：到 `at` 还没出声就换成挂钟，从 `anchor` 重新起步。
#[derive(Clone, Copy)]
pub(crate) struct AudioStartDeadline {
    at: std::time::Instant,
    anchor: f64,
    rate: f64,
}

impl Shared {
    /// 当前播放位置（主时钟）。视频循环与进度上报都走这里，时钟被换掉后
    /// 各处的读数自动跟上。
    pub(crate) fn now(&self) -> f64 {
        self.check_audio_start();
        if self.hold.load(Ordering::Relaxed) {
            return *self.start_anchor.lock();
        }
        let clock = Arc::clone(&*self.clock.lock());
        // 播放中才允许回调间外推：暂停/水位断流时声音不前进，时钟必须冻住。
        let smooth = !self.paused.load(Ordering::Relaxed)
            && !self
                .audio_device
                .lock()
                .as_ref()
                .is_some_and(|device| device.is_starved());
        clock.now_smooth(smooth)
    }

    /// 音频起播看门狗（见 [`Self::arm_audio_start_watchdog`]）：到点仍没出声
    /// 就换成挂钟。由 [`Self::now`] 顺带执行——视频循环每 3ms 一轮，粒度足够。
    fn check_audio_start(&self) {
        let deadline = *self.audio_start_deadline.lock();
        let Some(deadline) = deadline else {
            return;
        };
        let clock = Arc::clone(&*self.clock.lock());
        // 已经出声，或主时钟已被换走（音频迟到接管/音频播完切挂钟）：
        // 都说明这条看门狗不再适用。
        if clock.has_advanced() || clock.is_wall() {
            *self.audio_start_deadline.lock() = None;
            return;
        }
        if std::time::Instant::now() < deadline.at {
            return;
        }
        let wall = Arc::new(Clock::wall());
        wall.set_rate(deadline.rate, 0.0);
        wall.reset(deadline.anchor);
        *self.clock.lock() = wall;
        *self.audio_start_deadline.lock() = None;
        log::warn!(
            "[Playback] 音频设备 {}s 内未出声（HAL 卡住），回退挂钟继续无声播放",
            AUDIO_START_TIMEOUT.as_secs()
        );
    }

    /// 把时钟闸在 `secs`：视频停在这一帧等音频/seek 重定位完成。
    pub(crate) fn hold_at(&self, secs: f64) {
        *self.start_anchor.lock() = secs;
        self.hold.store(true, Ordering::Relaxed);
    }

    /// 放行时钟闸（音频已接上 / 首帧已出）。
    pub(crate) fn release_hold(&self) {
        self.hold.store(false, Ordering::Relaxed);
    }

    /// 换主时钟（音频迟到接管/音频播完切挂钟时用）。
    pub(crate) fn set_clock(&self, clock: Arc<Clock>) {
        *self.clock.lock() = clock;
    }

    /// 首帧可上屏：打开起播闸（时钟起跑、音频设备启动）。幂等。
    ///
    /// 无音频/设备迟到时主时钟还是挂钟，重锚到起点——预滚期间它已经走了的
    /// 时间不算数。音频已就绪时换成音频时钟并 reset 到同一起点：环里预填的
    /// 正是从起点开始的 PCM，设备一起跑，声画就对齐。持有 `clock_gate`
    /// 完成全部动作（同步锁，不跨 await）。
    ///
    /// **音频起不来不能冻住画面**：CoreAudio 的设备启动在 HAL 层可能长时间
    /// 不返回（本机 USB 声卡实测 `AudioQueueStart` 卡在 `DoIONodeConnect`
    /// 超过一分钟）。所以换到音频时钟后起一条看门狗：`AUDIO_START_TIMEOUT`
    /// 内没收到带真实帧的回调，就退回挂钟——无声也要能正常看。
    pub(crate) fn start_clock(&self) {
        // 首帧出画即放行（音频已注册时下面同时起跑设备）
        self.hold.store(false, Ordering::Relaxed);
        let mut gate = self.clock_gate.lock();
        let ClockGate::Waiting { audio } = &mut *gate else {
            return;
        };
        let audio = audio.take();
        *gate = ClockGate::Started;
        let anchor = *self.start_anchor.lock();
        match audio {
            Some((clock, device)) => {
                clock.reset(anchor);
                *self.clock.lock() = clock;
                device.activate();
                log::info!("[Playback] 首帧就绪：音频时钟起跑（锚点 {anchor:.2}s）");
                self.arm_audio_start_watchdog(anchor);
            }
            None => {
                // 无音频（或设备迟到）：主时钟重锚到起点。迟到接管自己的
                // 路径会看到 Started 并直接换轴（见 open::adopt_audio），这里
                // 只负责把挂钟的预滚时间抹掉。
                self.clock.lock().reset(anchor);
                log::info!("[Playback] 首帧就绪：挂钟重锚（{anchor:.2}s）");
            }
        }
    }

    /// 音频起播看门狗：设备迟迟不出声就回退挂钟（见 [`Self::start_clock`]）。
    ///
    /// 做法是给主时钟贴一个「超时时刻」标记，由**视频循环**（本来每 3ms
    /// 一轮）在 [`Self::now`] 里判定——不另起线程、不多一把锁：
    ///
    /// - 音频时钟在标记到点前收到过真实帧 → 标记清掉，正常走音频时钟；
    /// - 到点仍一帧未推进 → 主时钟换成挂钟，从锚点继续（无声播放）。
    fn arm_audio_start_watchdog(&self, anchor: f64) {
        let rate = self.rate();
        *self.audio_start_deadline.lock() = Some(AudioStartDeadline {
            at: std::time::Instant::now() + AUDIO_START_TIMEOUT,
            anchor,
            rate,
        });
    }

    /// seek 后重新武装起播闸：下一帧出画前时钟与设备都不走。
    ///
    /// seek 与起播同一个问题——视频要从目标前的关键帧解码预滚到目标，
    /// 这段时间时钟若在走，首帧（seek 后的画面）一出就已经落后一截。
    /// 锁顺序与 [`Self::start_clock`] 一致（gate → anchor → clock → device）。
    pub(crate) fn rearm_gate(&self, target: f64) {
        let mut gate = self.clock_gate.lock();
        // seek 预滚期间闸住时钟：视频从目标前的关键帧解到目标，这段解码
        // 时间不能算进播放时间（否则首帧一出画就已落后）。
        *self.start_anchor.lock() = target;
        self.hold.store(true, Ordering::Relaxed);
        let clock = Arc::clone(&self.clock.lock());
        let device = self.audio_device.lock().clone();
        if let Some(device) = &device {
            // 设备先停：预滚期间不能有任何声音推进时钟（旧缓冲由喂送侧的
            // 冲流协议在设备重新起跑后丢弃）
            device.deactivate();
        }
        let audio = if clock.is_wall() {
            None
        } else {
            device.map(|device| (clock, device))
        };
        *gate = ClockGate::Waiting { audio };
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
