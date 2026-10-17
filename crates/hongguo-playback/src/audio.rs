//! 音频输出与主时钟。
//!
//! 时钟口径是**可闻位置**：`base + (已消费帧数 − 设备引线) × rate ÷ 采样率`。
//! 已消费是设备回调拉走的帧数；引线来自回调时间戳（`playback − callback`，
//! 即这批样本到 DAC 的提前量）——不扣的话时钟快于耳朵，画面就跟着快，
//! 这是音画不同步的系统性来源之一（蓝牙/大缓冲设备上更明显）。主流播放器
//! （Chromium/ExoPlayer）的音频时钟都取可闻位置。
//!
//! 暂停即冻结（设备不消费）；倍速直接乘在时钟上（换速时 rebase）。无音频轨
//! 或设备打不开时退化为挂钟（暂停在该模式下失效——首页场景恒有音轨）。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::time::Instant;

use parking_lot::Mutex;
use ringbuf::traits::{Consumer, Observer, Producer, Split};

use hongguo_media_platform::platform::audio::{AudioOutputStream, PlatformAudioOutput};

/// 可闻位置时钟。媒体时间域 = 样本 PTS（秒）。
pub(crate) struct Clock {
    sample_rate: u64,
    /// 音频已播帧数（每声道一个样本算一帧）。
    consumed: AtomicU64,
    /// 设备引线（帧）：这批样本从回调到 DAC 的提前量。≤ consumed。
    lead: AtomicU64,
    /// base（媒体秒）、倍速与待生效的换速：换速时 rebase，必须原子地一起动。
    state: Mutex<ClockState>,
    /// 无音频模式：起点挂钟。
    wall: Mutex<Option<Instant>>,
}

struct ClockState {
    base: f64,
    rate: f64,
    /// 分级换速：环里还压着旧速率内容时，换速**延迟到它放完**（阈值记在
    /// consumed 上）。`rate()` 在此期间返回目标值——WSOLA 与界面即刻按新
    /// 倍速走，只有时钟的推进轴等旧内容放完才换。None = 无待生效换速。
    pending: Option<PendingRate>,
}

struct PendingRate {
    at_consumed: u64,
    rate: f64,
}

/// 环内残量低于这个秒数时换速直接生效（不值得为几十毫秒 staging）。
const IMMEDIATE_RATE_DRAIN_SECS: f64 = 0.05;

impl Clock {
    pub(crate) fn audio(sample_rate: u32) -> Self {
        Self {
            sample_rate: u64::from(sample_rate),
            consumed: AtomicU64::new(0),
            lead: AtomicU64::new(0),
            state: Mutex::new(ClockState {
                base: 0.0,
                rate: 1.0,
                pending: None,
            }),
            wall: Mutex::new(None),
        }
    }

    pub(crate) fn wall() -> Self {
        Self {
            sample_rate: 0,
            consumed: AtomicU64::new(0),
            lead: AtomicU64::new(0),
            state: Mutex::new(ClockState {
                base: 0.0,
                rate: 1.0,
                pending: None,
            }),
            wall: Mutex::new(None),
        }
    }

    /// 设播放起点（媒体秒）。计数清零，now() 从 base 起走。倍速保留
    /// （用户偏好不因起播/seek 重置）；待生效的换速作废（seek 优先）。
    pub(crate) fn reset(&self, base_secs: f64) {
        let mut st = self.state.lock();
        st.base = base_secs;
        st.pending = None;
        drop(st);
        self.consumed.store(0, Ordering::Relaxed);
        self.lead.store(0, Ordering::Relaxed);
        *self.wall.lock() = (self.sample_rate == 0).then(Instant::now);
    }

    pub(crate) fn now(&self) -> f64 {
        let st = self.state.lock();
        if self.sample_rate == 0 {
            let start = self.wall.lock().expect("挂钟已设");
            return st.base + start.elapsed().as_secs_f64() * st.rate;
        }
        let consumed = self.consumed.load(Ordering::Relaxed);
        let lead = self.lead.load(Ordering::Relaxed).min(consumed);
        st.base + (consumed - lead) as f64 / self.sample_rate as f64 * st.rate
    }

    /// 换倍速。
    ///
    /// 环里还压着旧速率内容（`drain_secs`，由调用方从环占有量读）时走
    /// **分级**：时钟暂按旧速率推进，阈值（`consumed + drain`）越过瞬间
    /// rebase 换新轴——旧内容按旧速率放完、新内容按新速率来，时间轴不跳、
    /// 音画不错位。立即换轴的话时钟比可闻位置快出 `drain×Δrate`（最多
    /// 1s×2.25），画面抢先声音一大截，正是「切倍速后音画不同步」的来源。
    ///
    /// `rate()` 自本调用起返回目标值：WSOLA 逐窗按新倍速伸缩（新内容进环
    /// 排在旧内容后面），界面倍速按钮即刻高亮——只有主时钟的轴在等。
    pub(crate) fn set_rate(&self, rate: f64, drain_secs: f64) {
        let mut st = self.state.lock();
        if self.sample_rate == 0 || drain_secs <= IMMEDIATE_RATE_DRAIN_SECS {
            // 立即换轴：按旧速率算出当前位并锁内 rebase（清计数），不跳
            st.base = audio_now(&st, self.sample_rate, &self.consumed, &self.lead);
            st.rate = rate;
            st.pending = None;
            drop(st);
            self.consumed.store(0, Ordering::Relaxed);
            self.lead.store(0, Ordering::Relaxed);
            return;
        }
        st.pending = Some(PendingRate {
            at_consumed: self.consumed.load(Ordering::Relaxed)
                + (drain_secs * self.sample_rate as f64) as u64,
            rate,
        });
    }

    /// 当前倍速。语义是**目标倍速**：分级换速的等待期里也返回新值——
    /// 喂送侧 WSOLA、界面显示、新引擎继承偏好都该拿目标，而不是旧轴。
    pub(crate) fn rate(&self) -> f64 {
        let st = self.state.lock();
        st.pending.as_ref().map(|p| p.rate).unwrap_or(st.rate)
    }

    /// 是否挂钟（无音频轨/设备不可用时的退化时钟）。
    pub(crate) fn is_wall(&self) -> bool {
        self.sample_rate == 0
    }

    /// 设备停流时清掉引线（见 [`AudioDevice::apply`]）：让时钟承认
    /// 「已取走但未发声」的这批样本已经过去，与下一次真正发声的内容对齐。
    pub(crate) fn drop_lead(&self) {
        self.lead.store(0, Ordering::Relaxed);
    }

    /// 输出回调记账：设备又消费了 `frames` 帧、当前引线 `lead` 帧。
    /// 帧数按增量累加（漏一次回调也不失真），引线按最新值覆盖（可闻位置
    /// 读数时再钳到 ≤ 已消费）。越过分级换速阈值时在此换轴（回调线程
    /// 串行，不会重复生效）。
    fn advance(&self, frames: u64, lead: u64) {
        let consumed = self.consumed.fetch_add(frames, Ordering::Relaxed) + frames;
        self.lead.store(lead, Ordering::Relaxed);
        // 检查与取走同一把锁：UI 线程的 set_rate 在锁外，不会插进检查与
        // 生效之间把阈值换掉
        let mut st = self.state.lock();
        let Some(p) = st.pending.take_if(|p| consumed >= p.at_consumed) else {
            return;
        };
        // base 推进到阈值处的可闻位（引线用当前值，误差 ≤ 一根引线），
        // 计数清零从阈值重新起算——之后 now() 就是新轴
        st.base += (p
            .at_consumed
            .saturating_sub(self.lead.load(Ordering::Relaxed).min(p.at_consumed)))
            as f64
            / self.sample_rate as f64
            * st.rate;
        st.rate = p.rate;
        drop(st);
        self.consumed.store(0, Ordering::Relaxed);
        self.lead.store(0, Ordering::Relaxed);
    }
}

/// now() 的音频支路（set_rate 锁内复用）。
fn audio_now(st: &ClockState, sample_rate: u64, consumed: &AtomicU64, lead: &AtomicU64) -> f64 {
    if sample_rate == 0 {
        return st.base;
    }
    let c = consumed.load(Ordering::Relaxed);
    let l = lead.load(Ordering::Relaxed).min(c);
    st.base + (c - l) as f64 / sample_rate as f64 * st.rate
}

/// 平台输出流（已带 Send，具体类型由平台 crate 保证）。
///
/// macOS 是 AudioToolbox 默认输出单元（output-only），Windows 是 cpal 的
/// WASAPI 流。引擎只认 [`AudioOutputStream`] 契约——**不含任何平台分支**。
type Stream = <hongguo_media_platform::platform::audio::PlatformAudioOutput as
    hongguo_media_platform::platform::audio::AudioOutputHost>::Stream;

/// 设备级播放开关的仲裁：**用户暂停**、**缓冲断流暂停**与**起播闸**各占
/// 一位，三者都放行时设备才运转。三个来源来自不同任务（控制面、喂送循环、
/// 视频首帧），必须单点合成，否则「缓冲恢复」会把用户暂停悄悄解除。
///
/// **设备调用跑在专属线程上**：CoreAudio 的 `AudioQueueStart`（以及此前的
/// AudioUnit 初始化）在 HAL 层可能阻塞数秒——本机 USB 声卡实测 4–5 秒，
/// 栈停在 `AudioQueueObject::Prime → DoIONodeConnect`。这些调用若发生在
/// 播放任务里，整条链路（视频上屏、喂送、seek）一起冻住。这里只投一条
/// 命令（非阻塞），由控制线程串行执行：命令有序、失败只记日志。
pub(crate) struct AudioDevice {
    /// 控制线程的命令口；线程随 sender 关闭退出，流在其作用域内释放。
    cmd: std::sync::mpsc::Sender<bool>,
    user_paused: AtomicBool,
    starved: AtomicBool,
    /// 主时钟（同一设备域）。设备停下的瞬间要把「引线」清零——见 `apply`。
    clock: Arc<Clock>,
    /// 起播闸：首帧出画前设备不转。视频从起点前的关键帧起解，预滚期间
    /// 时钟若随设备推进，首帧一出画就已经落后一截，之后解码只够维持
    /// 内容帧率、永远追不回（实测 1080p 续播首帧晚 6 秒）。所以设备与
    /// 时钟都等首帧出画才启动（见 `player::Shared::start_clock`）。
    activated: AtomicBool,
}

impl AudioDevice {
    /// 起控制线程并接管流的生命周期。线程名固定，排障时一眼能认出来。
    fn new(stream: Stream, clock: Arc<Clock>) -> Option<Self> {
        let (tx, rx) = std::sync::mpsc::channel::<bool>();
        let spawned = std::thread::Builder::new()
            .name("hg-audio-ctl".into())
            .spawn(move || {
                let mut stream = Some(stream);
                // 幂等记账：状态未变不发命令（发送端已在 apply 里做过，
                // 这里兜重复投递）
                let mut playing = false;
                while let Ok(want) = rx.recv() {
                    if want == playing {
                        continue;
                    }
                    let Some(s) = stream.as_ref() else {
                        break;
                    };
                    if let Err(e) = s.set_playing(want) {
                        log::warn!(
                            "[Playback] 音频设备 {}失败: {e}",
                            if want { "启动" } else { "暂停" }
                        );
                    } else {
                        playing = want;
                    }
                }
                // sender 关闭（设备析构）：流在这里释放。AudioQueue 的
                // Stop/Dispose 同样可能卡在 HAL，放在本线程不影响播放。
                drop(stream.take());
            });
        if let Err(e) = spawned {
            log::warn!("[Playback] 音频控制线程起不来（{e}），本集无声");
            return None;
        }
        Some(Self {
            cmd: tx,
            user_paused: AtomicBool::new(false),
            starved: AtomicBool::new(false),
            clock,
            activated: AtomicBool::new(false),
        })
    }

    fn apply(&self) {
        let activated = self.activated.load(Ordering::Relaxed);
        let user_paused = self.user_paused.load(Ordering::Relaxed);
        let starved = self.starved.load(Ordering::Relaxed);
        let should_play = activated && !user_paused && !starved;
        if !should_play {
            // 设备停下的瞬间，已经交给 HAL、但不会真正发声的那一批（引线）
            // 不能再算作「已播」：这些样本从环形缓冲里被取走了、停流时被
            // 丢弃，而时钟的 consumed 已经把它们计进去。不清零引线的话，
            // 每次缓冲暂停都会让时钟比声音多走一个引线（十几毫秒），
            // 反复缓冲后累积成可闻的音画不同步（声音越来越落后画面）。
            self.clock.drop_lead();
        }
        // 设备起停留痕：用户暂停是唯一没有别处记录的位（starved 翻转有
        // 水位日志），只在它参与时升到 info，纯水位翻转让给水位日志
        log::log!(
            if user_paused || !activated {
                log::Level::Info
            } else {
                log::Level::Debug
            },
            "[Playback] 音频设备 {}（activated={activated} user_paused={user_paused} starved={starved}）",
            if should_play { "启动" } else { "暂停" }
        );
        // 非阻塞投递：真正的设备调用在控制线程里做（见类型注释）。
        if let Err(e) = self.cmd.send(should_play) {
            log::warn!("[Playback] 音频控制线程已退出，命令丢弃: {e}");
        }
    }

    pub(crate) fn set_user_paused(&self, paused: bool) {
        self.user_paused.store(paused, Ordering::Relaxed);
        self.apply();
    }

    /// 缓冲水位控制（rebuffer 状态机）。返回值 = 状态是否发生翻转
    /// （翻转沿是发 Buffering/Ready 事件的时机）。
    pub(crate) fn set_starved(&self, starved: bool) -> bool {
        if self.starved.swap(starved, Ordering::Relaxed) == starved {
            return false;
        }
        self.apply();
        true
    }

    /// 首帧出画：打开起播闸。返回是否为首次激活（迟到接管的设备已经
    /// 激活过，此时不得再重锚时间轴）。
    pub(crate) fn activate(&self) -> bool {
        if self.activated.swap(true, Ordering::Relaxed) {
            return false;
        }
        self.apply();
        true
    }

    /// seek 重新武装起播闸：先把设备停住，下一帧出画再由 [`Self::activate`]
    /// 放行。设备停着时回调不来，seek 的冲流收口会等到设备重新起跑——
    /// 这是预期顺序（旧声音绝不能在预滚期间漏出去）。
    pub(crate) fn deactivate(&self) {
        if !self.activated.swap(false, Ordering::Relaxed) {
            return;
        }
        self.apply();
    }
}

/// 音频输出与主时钟的控制块（设备 + 回调里的可变量）。
pub(crate) struct AudioOut {
    pub(crate) device: Arc<AudioDevice>,
    pub(crate) clock_src: Arc<Clock>,
    /// 音量（f32 位型存储；1.0 = 全量）。回调内乘 PCM——零延迟生效。
    volume: Arc<AtomicU32>,
    /// 静音独立于音量（对齐 Tauri 版 playback-prefs 语义）。
    muted: Arc<AtomicBool>,
    /// 环形缓冲占有量（设备域帧）：投放端与回调各刷一份真值。分级换速
    /// 靠它知道「环里还压着多少旧速率内容」（见 [`Clock::set_rate`]）。
    ring_frames: Arc<AtomicU64>,
    /// 设备域采样率（占有量帧数换算秒用）。
    device_rate: u32,
    /// 冲流中：回调丢弃环形缓冲里的所有旧数据、不推进时钟（seek 用）。
    discarding: Arc<AtomicBool>,
    /// 冲流收口凭证（见 [`RingFeed::end_flush_if_seen`]）。
    discard_seen: Arc<AtomicBool>,
    /// 欠载静音累计（诊断）：持续增长 = 喂送跟不上；接近 0 = 健康。
    #[expect(dead_code, reason = "诊断字段：与 [cb] 日志配套，排障时按需读取")]
    pub(crate) silence_frames: Arc<AtomicU64>,
}

impl AudioOut {
    /// 建输出流。`channels`/`sample_rate` 是**解码侧**格式；音量/静音取当前值。
    /// 流按**设备混音格式**开（Windows 共享模式不转换任意采样率——解码
    /// 44100Hz 打到只收 48000Hz 的设备直接建流失败），喂送侧经
    /// [`PcmAdapter`] 适配；环形缓冲/回调/时钟/音量全部统一在设备域。
    /// 不在这里启动设备：调用方先预填（见 player::adopt_audio）。
    pub(crate) fn open(
        channels: u16,
        sample_rate: u32,
        volume: f32,
        muted: bool,
    ) -> Option<(Self, RingFeed)> {
        use hongguo_media_platform::platform::audio::{AudioOutputHost, OutputFormat};

        // 设备混音格式与输出流都走平台契约：macOS 是 AudioToolbox 默认输出
        // 单元（output-only，**不触发麦克风授权**），Windows 是 cpal/WASAPI。
        // 引擎这一层没有任何平台分支。
        let format = match <PlatformAudioOutput as AudioOutputHost>::default_output_format() {
            Ok(f) => f,
            Err(e) => {
                log::warn!("[Playback] 读音频设备混音格式失败: {e}");
                return None;
            }
        };
        let (dst_rate, dst_channels) = (format.sample_rate, format.channels);
        let adapter =
            crate::resample::PcmAdapter::new(channels, sample_rate, dst_channels, dst_rate);
        // 2 秒（设备域）：1 秒环在倍速下只有 0.5 秒媒体余量，喂送端一次
        // 网络抖动就会撞到低水位并触发「缓冲中」。2 秒给足吸收空间
        // （48kHz 立体声 ≈ 768KB，可忽略）。
        let ring = ringbuf::HeapRb::<f32>::new(usize::from(dst_channels) * dst_rate as usize * 2);
        let (prod, mut cons) = ring.split();

        // 时钟计数设备域帧：设备帧数 ÷ 设备采样率 = 媒体秒（重采样保持时长）
        let clock = Arc::new(Clock::audio(dst_rate));
        let cb_clock = Arc::clone(&clock);
        // 环占有量（设备域帧）：投放端与回调各刷一份真值，分级换速读它
        let ring_frames = Arc::new(AtomicU64::new(0));
        let cb_ring_w = Arc::clone(&ring_frames);
        let cb_calls = Arc::new(AtomicU64::new(0));
        // 回调节奏实测起点：clock 每秒推进量必须与墙钟一致（设备回调
        // 间隔 × 每拍帧数 ÷ 采样率）；不一致就是设备域口径错了，音画会
        // 系统性漂移（实测过一次 2 倍速推进）。
        let cb_started = std::time::Instant::now();
        // 欠载（缓冲空、只能出静音）累计帧数。
        //
        // **静音不推进时钟**：回调里只把真实取出的帧计入 consumed。设备在
        // 解码器还没喂上来的那几十毫秒里空转、或中途追不上喂送时，都会往
        // 喇叭送静音——那些帧要是也计时，音频时钟就跑到真实声音前面去。
        let cb_silence = Arc::new(AtomicU64::new(0));
        let cb_silence_w = Arc::clone(&cb_silence);
        let cb_volume = Arc::new(AtomicU32::new(volume.to_bits()));
        let cb_volume_w = Arc::clone(&cb_volume);
        let cb_muted = Arc::new(AtomicBool::new(muted));
        let cb_muted_w = Arc::clone(&cb_muted);
        let cb_discard = Arc::new(AtomicBool::new(false));
        let cb_discard_w = Arc::clone(&cb_discard);
        let cb_discard_feed = Arc::clone(&cb_discard);
        // 冲流的收口凭证：回调真跑过至少一个丢弃周期（设备暂停时回调不来，
        // 拿不到凭证——宁可等恢复，也别让旧声音漏出去）
        let cb_seen = Arc::new(AtomicBool::new(false));
        let cb_seen_w = Arc::clone(&cb_seen);
        let cb_seen_out = Arc::clone(&cb_seen);
        let channels_cb = usize::from(dst_channels);
        // 累计已消费帧数与引线都在时钟里，回调只管报数
        let stream = <PlatformAudioOutput as AudioOutputHost>::open(
            OutputFormat {
                sample_rate: dst_rate,
                channels: dst_channels,
            },
            move |data: &mut [f32], lead_frames: u32| {
                    if cb_discard.load(Ordering::Relaxed) {
                        // 冲流：整环丢弃（一次回调清空），出静音、不计时
                        cb_seen_w.store(true, Ordering::Relaxed);
                        let mut scratch = [0f32; 64];
                        loop {
                            if cons.pop_slice(&mut scratch) < scratch.len() {
                                break;
                            }
                        }
                        cb_ring_w.store(0, Ordering::Relaxed);
                        data.fill(0.0);
                        return;
                    }
                    let filled = cons.pop_slice(data); // 样本数
                    cb_ring_w.store(
                        (cons.occupied_len() / channels_cb) as u64,
                        Ordering::Relaxed,
                    );
                    // 设备引线：这批数据到 DAC 的提前量（帧），平台实现给出
                    let lead = f64::from(lead_frames);
                    let frames = filled / channels_cb;
                    if cb_muted.load(Ordering::Relaxed) {
                        // 静音不是暂停：照常推进时钟，只是听不见
                        data.fill(0.0);
                    } else {
                        let vol = f32::from_bits(cb_volume.load(Ordering::Relaxed));
                        if vol != 1.0 {
                            for s in &mut data[..filled] {
                                *s *= vol;
                            }
                        }
                        // 欠载的尾巴补静音
                        for slot in &mut data[filled..] {
                            *slot = 0.0;
                        }
                    }
                    let silent_frames = ((data.len() - filled) / channels_cb) as u64;
                    let silence = cb_silence_w.fetch_add(silent_frames, Ordering::Relaxed);
                    // 只计真实音频：时钟 =「已经送进设备的真实声音」的可闻位置
                    cb_clock.advance(frames as u64, lead as u64);
                    let n = cb_calls.fetch_add(1, Ordering::Relaxed);
                    if n.is_multiple_of(200) {
                        log::debug!(
                            "[Playback][cb] 第 {n} 次回调，本拍 {frames} 帧，clock={:.2}s，墙钟 {:.2}s，引线 {lead:.0} 帧，欠载 {:.1}s（累计）",
                            cb_clock.now(),
                            cb_started.elapsed().as_secs_f64(),
                            (silence + silent_frames) as f64 / f64::from(dst_rate)
                        );
                    }
            },
        )
        .map_err(|e| {
            log::warn!(
                "[Playback] 音频建流失败（解码 {channels}ch {sample_rate}Hz → 设备 {dst_channels}ch {dst_rate}Hz）: {e}"
            );
            e
        })
        .ok()?;
        if channels != dst_channels || sample_rate != dst_rate {
            log::info!(
                "[Playback] 音频设备已建（{dst_channels}ch {dst_rate}Hz，解码侧 {channels}ch {sample_rate}Hz 已适配）"
            );
        } else {
            log::info!("[Playback] 音频设备已建（{channels}ch {sample_rate}Hz）");
        }
        let device = Arc::new(AudioDevice::new(stream, Arc::clone(&clock))?);
        Some((
            Self {
                device: Arc::clone(&device),
                clock_src: clock,
                volume: cb_volume_w,
                muted: cb_muted_w,
                ring_frames: Arc::clone(&ring_frames),
                device_rate: dst_rate,
                discarding: cb_discard_w,
                discard_seen: cb_seen_out,
                silence_frames: cb_silence,
            },
            RingFeed {
                producer: prod,
                pending: Vec::new(),
                ring_frames,
                discarding: cb_discard_feed,
                discard_seen: cb_seen,
                device,
                adapter,
            },
        ))
    }

    pub(crate) fn set_volume(&self, volume: f32) {
        self.volume.store(volume.to_bits(), Ordering::Relaxed);
    }

    pub(crate) fn set_muted(&self, muted: bool) {
        self.muted.store(muted, Ordering::Relaxed);
    }

    /// 环形缓冲滞留时长（秒，设备域）。换倍速时它就是「还压在环里的旧速率
    /// 内容」的播放时间——分级时钟的换轴延迟就取这个值。
    pub(crate) fn buffered_secs(&self) -> f64 {
        self.ring_frames.load(Ordering::Relaxed) as f64 / f64::from(self.device_rate.max(1))
    }

    /// 开始冲流（seek 跳点即调）：回调进入丢弃态并清零收口凭证，
    /// 收口由喂送端重新预填后经 [`RingFeed::end_flush_if_seen`] 完成。
    pub(crate) fn begin_flush(&self) {
        self.discard_seen.store(false, Ordering::Relaxed);
        self.discarding.store(true, Ordering::Relaxed);
    }
}

impl Drop for AudioOut {
    fn drop(&mut self) {
        // 停设备（命令投给控制线程，非阻塞），流与控制线程随命令口关闭退出：
        // 设备释放可能卡在 HAL，不能放在播放任务的析构路径上。
        self.device.set_user_paused(true);
    }
}

/// 音频解码循环往环形缓冲投放的把手，兼带冲流协议的收口端。投放前先把
/// 解码域 PCM 适配成设备混音格式（[`crate::resample::PcmAdapter`]）。
pub(crate) struct RingFeed {
    producer: ringbuf::HeapProd<f32>,
    /// 上次投放的未写完余量（设备域）。用户暂停中断投放时留这里，恢复后
    /// 的下一次调用先续投它——旧包必须排在新包前面（保序）。
    pending: Vec<f32>,
    /// 环占有量（设备域帧，与 [`AudioOut::ring_frames`] 同源）。投放端
    /// 每次写入后刷新。
    ring_frames: Arc<AtomicU64>,
    discarding: Arc<AtomicBool>,
    discard_seen: Arc<AtomicBool>,
    device: Arc<AudioDevice>,
    adapter: crate::resample::PcmAdapter,
}

impl RingFeed {
    /// 投放一段交错 PCM（解码域）；缓冲满时让出任务等设备消费（消费速率
    /// 由硬件决定）。返回 `false` = 停机。
    ///
    /// **用户暂停（`paused` 命中）时把已适配余量留在 [`Self::pending`]
    /// 立即返回 `true`**：环满 + 设备暂停时没人消费，原地等下去会让喂送
    /// 任务对 seek 全部失聪——它该回暂停等待循环去，那里才轮得到 poll。
    /// 恢复后的下一次调用先续投余量再处理新输入（保序）。
    pub(crate) async fn push(
        &mut self,
        samples: &[f32],
        shutdown: &AtomicBool,
        paused: &AtomicBool,
    ) -> bool {
        if self.pending.is_empty() {
            self.pending = self.adapter.process(samples).to_vec();
        }
        let (dst_ch, _) = self.adapter.dst_format();
        let mut done = 0usize;
        while done < self.pending.len() {
            if shutdown.load(Ordering::Relaxed) {
                return false;
            }
            if paused.load(Ordering::Relaxed) {
                self.pending.drain(..done);
                return true;
            }
            done += self.producer.push_slice(&self.pending[done..]);
            self.ring_frames.store(
                (self.producer.occupied_len() / usize::from(dst_ch.max(1))) as u64,
                Ordering::Relaxed,
            );
            if done < self.pending.len() {
                tokio::time::sleep(std::time::Duration::from_millis(4)).await;
            }
        }
        self.pending.clear();
        true
    }

    /// 环空位时长（秒，设备域）。冲流期的投放决策用它：放不下就丢包，
    /// 绝不等在 push 上（设备暂停时没人消费，等 = 卡死喂送任务）。
    pub(crate) fn vacant_secs(&self) -> f64 {
        let (dst_ch, dst_rate) = self.adapter.dst_format();
        let frames = self.producer.vacant_len() / usize::from(dst_ch.max(1));
        frames as f64 / f64::from(dst_rate.max(1))
    }

    /// 解除缓冲断流暂停（转发给设备；seek 冲流收口依赖回调跑起来）。
    pub(crate) fn unstarve(&self) {
        self.device.set_starved(false);
    }

    /// 开始冲流（与 [`AudioOut::begin_flush`] 同一协议，喂送端视角）。
    /// 适配器相位一并清（新旧位置不连续）；未投放的余量作废（旧位置的
    /// 声音不该再进环）。
    pub(crate) fn begin_flush(&mut self) {
        self.discard_seen.store(false, Ordering::Relaxed);
        self.discarding.store(true, Ordering::Relaxed);
        self.adapter.reset();
        self.pending.clear();
    }

    /// 结束冲流：**仅当**回调真跑过丢弃周期才放行（设备暂停时回调不来，
    /// 凭证拿不到就等——否则旧数据会在恢复后漏出去）。返回是否已收口。
    pub(crate) fn end_flush_if_seen(&self) -> bool {
        if self.discard_seen.load(Ordering::Relaxed) {
            self.discarding.store(false, Ordering::Relaxed);
            true
        } else {
            false
        }
    }

    /// 环内滞留时长（秒，设备域——环里是适配后的设备样本）。
    pub(crate) fn buffered_secs(&self) -> f64 {
        let (dst_ch, dst_rate) = self.adapter.dst_format();
        let frames = self.producer.occupied_len() / usize::from(dst_ch.max(1));
        frames as f64 / f64::from(dst_rate.max(1))
    }

    /// 缓冲断流暂停（见 AudioDevice::set_starved）。
    pub(crate) fn set_starved(&self, starved: bool) -> bool {
        self.device.set_starved(starved)
    }
}

/// 冲流协议 + 水位状态机的单元测试（不碰真设备，纯逻辑部分）。
#[cfg(test)]
mod tests {
    use super::*;

    /// 时钟数学：可闻位置 = base + (消费 − 引线) × rate ÷ 采样率。
    #[test]
    fn clock_counts_audible_position_not_pulled() {
        let clock = Clock::audio(48_000);
        clock.reset(10.0);
        // 拉了 1 秒、引线 480 帧（10ms）：可闻位置 10s + 0.99s
        clock.advance(48_000, 480);
        assert!((clock.now() - 10.99).abs() < 1e-9);
    }

    /// 引线不可能超过已消费量（起播瞬间）：钳到 0，不出负数时间。
    #[test]
    fn clock_clamps_lead_to_consumed() {
        let clock = Clock::audio(48_000);
        clock.reset(0.0);
        clock.advance(100, 9_999);
        assert!(clock.now() >= 0.0);
    }

    /// 换倍速 rebase：时间轴连续，换速后按新速率推进。
    #[test]
    fn clock_rebases_on_rate_change() {
        let clock = Clock::audio(48_000);
        clock.reset(0.0);
        clock.advance(48_000, 0); // 1s
        clock.set_rate(2.0, 0.0); // 环空，立即换轴
        let at_switch = clock.now();
        assert!((at_switch - 1.0).abs() < 1e-9, "换速瞬间不跳");
        clock.advance(48_000, 0); // 再消费 1s 设备时间 = 媒体 2s
        assert!((clock.now() - 3.0).abs() < 1e-9);
    }

    /// 分级换速：环里压着旧速率内容时，时钟按旧轴走到阈值再换新轴，
    /// 全程连续；`rate()` 从换速调用起就返回目标值（WSOLA/界面语义）。
    #[test]
    fn clock_stages_rate_change_until_ring_drains() {
        let clock = Clock::audio(48_000);
        clock.reset(0.0);
        clock.advance(24_000, 0); // 可闻 0.5s；环里还有 1s 旧内容
        clock.set_rate(2.0, 1.0);
        assert_eq!(clock.rate(), 2.0, "目标倍速即刻生效（WSOLA/界面语义）");
        assert!((clock.now() - 0.5).abs() < 1e-9, "换速调用不跳轴");
        // 旧内容还没放完：继续旧轴
        clock.advance(24_000, 0);
        assert!((clock.now() - 1.0).abs() < 1e-9, "阈值前按旧速率推进");
        // 越过阈值（consumed 24000+24000+48000=96000 ≥ 72000）：rebase 换轴。
        // 越线的那一小撮帧（≤ 一次回调 ~10ms）随清计数归零，可闻位停驻阈值处
        clock.advance(48_000, 0);
        assert!((clock.now() - 1.5).abs() < 1e-9, "阈值处可闻位 1.5s");
        clock.advance(24_000, 0);
        assert!((clock.now() - 2.5).abs() < 1e-9, "阈值后按新速率推进");
        clock.advance(24_000, 0);
        assert!((clock.now() - 3.5).abs() < 1e-9);
        // 换轴后 pending 清掉，rate 回到状态值
        assert_eq!(clock.rate(), 2.0);
    }

    /// 分级等待期间 seek：reset 作废待生效换速，从新起点按原倍速走。
    #[test]
    fn seek_cancels_pending_rate() {
        let clock = Clock::audio(48_000);
        clock.reset(0.0);
        clock.advance(24_000, 0);
        clock.set_rate(2.0, 1.0);
        clock.reset(10.0);
        assert_eq!(clock.rate(), 1.0, "reset 作废待生效换速（保留当前轴）");
        clock.advance(48_000, 0);
        assert!((clock.now() - 11.0).abs() < 1e-9);
    }

    /// 倍速对挂钟模式同样生效（无音频退化路径）。
    #[test]
    fn wall_clock_scales_with_rate() {
        let clock = Clock::wall();
        clock.set_rate(2.0, 0.0);
        clock.reset(5.0);
        assert!((clock.now() - 5.0).abs() < 0.05);
    }
}
