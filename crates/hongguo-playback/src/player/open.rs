//! 起播装配：`open` 三阶段编排（头部 → 音频设备 → 视频循环）、音频产物
//! 接管（`adopt_audio`）与建流尝试（`audio_attempt`）。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::time::Duration;

use hongguo_video::sample_table::TrackInfo;

use crate::audio::{AudioOut, Clock, RingFeed};
use crate::source::{ByteSource, SampleSource, open_stream};
use crate::video;

use super::Shared;
use super::feeder::audio_feeder;
use super::{OpenOptions, PlaySource, PlayerHandle};

/// 打开并起播。必须在 tokio 运行时上下文内 await（引擎任务经 `tokio::spawn`
/// 挂在调用方的运行时上）。
/// 返回 `Arc<PlayerHandle>`：GPUI 壳小窗模式下主窗与小窗两个视图共享同一
/// 个播放器（跨窗口渲染同一份帧流），句柄析构（停播）由最后一份引用触发。
pub async fn open(opts: OpenOptions) -> hongguo_common::AppResult<std::sync::Arc<PlayerHandle>> {
    // 起播链任何一段卡住，界面上都只是「一直转圈」——三个阶段各自打点，
    // 卡在哪一段一目了然（实测有过音频设备打开前静默两位数的秒数）。
    let t0 = std::time::Instant::now();
    let source = match &opts.source {
        PlaySource::Online { vid, definition } => ByteSource::Online {
            vid: vid.clone(),
            definition: *definition,
        },
        PlaySource::Local { path } => ByteSource::local(path)?,
    };
    let meta = open_stream(&source).await?;
    let video_info = meta.video;
    log::info!(
        "[Playback] 头部就绪（{}ms）：{}x{} {} 音频{}",
        t0.elapsed().as_millis(),
        video_info.width,
        video_info.height,
        match meta.video_codec {
            hongguo_media_platform::platform::video::VideoCodec::Hevc => "HEVC",
            hongguo_media_platform::platform::video::VideoCodec::H264 => "H.264",
        },
        meta.audio
            .as_ref()
            .map(|a| format!(" {}ch {}Hz", a.channels, a.sample_rate))
            .unwrap_or_else(|| "无".into())
    );

    // 时长 = 视/音两轨谁的尾巴长（elst 修剪过的 presentation 轴）
    let video_end = media_end(&video_info);
    let audio_end = meta.audio.as_ref().map(media_end).unwrap_or(0.0);
    let duration = video_end.max(audio_end);

    // 先立挂钟与共享态：音频可能迟到，视频任务不能等它（见下）。
    // 倍速在出生时就定到目标轴（clamp 与 handle.set_rate 同界）：adopt_audio
    // 经 shared.rate() 继承它，喂送侧 WSOLA 从第一包就按目标倍速伸缩——
    // 不留「起播后宿主补发 set_rate」的分级换轴窗（见 OpenOptions::rate）。
    let wall = Arc::new(Clock::wall());
    wall.reset(opts.start_secs);
    wall.set_rate(opts.rate.clamp(super::MIN_RATE, super::MAX_RATE), 0.0);
    let shared = Arc::new(Shared {
        paused: AtomicBool::new(false),
        shutdown: AtomicBool::new(false),
        clock: parking_lot::Mutex::new(Arc::clone(&wall)),
        seek_broadcast: parking_lot::Mutex::new((0, 0.0)),
        read_epoch: AtomicU64::new(0),
        buffered_ui: AtomicBool::new(false),
        volume: AtomicU32::new(1.0f32.to_bits()),
        muted: AtomicBool::new(false),
        latest: parking_lot::Mutex::new(None),
        dims: parking_lot::Mutex::new((video_info.width as usize, video_info.height as usize)),
        duration_secs: parking_lot::Mutex::new(duration),
        start_anchor: parking_lot::Mutex::new(opts.start_secs),
        hold: AtomicBool::new(false),
        clock_gate: parking_lot::Mutex::new(super::ClockGate::Waiting { audio: None }),
        audio_device: parking_lot::Mutex::new(None),
        events: opts.events,
    });
    let audio_slot: Arc<parking_lot::Mutex<Option<AudioOut>>> =
        Arc::new(parking_lot::Mutex::new(None));
    let mut tasks = Vec::new();

    // 视频循环先起：音频建流在 HAL 层可能卡若干秒（本机 USB 声卡实测
    // 3.7–5s，极端 570s），把它放在视频之前等于让整个起播等声卡——画面
    // 一直盖着封面。两条链并行，谁慢谁等：视频首帧一到就出画，音频就绪
    // 后按当前播放位置接管（见下方 spawn 的音频任务）。
    {
        let shared = Arc::clone(&shared);
        let info = video_info.clone();
        let decoder_init = video::DecoderInitOwned {
            codec: meta.video_codec,
            parameter_sets: meta.video_parameter_sets.clone(),
            sequence_header: meta.video_sequence_header.clone(),
            length_size: meta.video_length_size,
            width: info.width as usize,
            height: info.height as usize,
        };
        let source = SampleSource::new(source.clone(), Arc::new(info.samples.clone()));
        let start = opts.start_secs;
        tasks.push(tokio::spawn(async move {
            video::run(shared, source, info, decoder_init, start).await;
        }));
    }

    // 音频设备：建流（阻塞池，最多等 AUDIO_OPEN_TIMEOUT）→ 预填 → 登记进
    // 起播闸。整个过程在独立任务里，不阻塞 `open` 返回与视频出画。
    //
    // cpal/CoreAudio 的建流会在 HAL 层实测卡住（本机见过 570 秒才失败，栈停在
    // audio_unit_from_device）。等不到就先出画，留一个任务守着建流结果，
    // 设备迟到建成时接进来：换主时钟、从当前位置喂音频。
    {
        let shared = Arc::clone(&shared);
        let slot = Arc::clone(&audio_slot);
        let source = source.clone();
        let audio_info = meta.audio.clone();
        let start = opts.start_secs;
        let t_started = t0;
        tasks.push(tokio::spawn(async move {
            let Some(info) = audio_info.clone() else {
                log::info!(
                    "[Playback] 无音频轨（累计 {}ms）",
                    t_started.elapsed().as_millis()
                );
                return;
            };
            // 建流本身是阻塞调用（含 20ms 轮询与 HAL 等待），放阻塞池，
            // 别占异步 worker
            let volume = f32::from_bits(shared.volume.load(Ordering::Relaxed));
            let muted = shared.muted.load(Ordering::Relaxed);
            let attempt_info = info.clone();
            let attempt = tokio::task::spawn_blocking(move || {
                audio_attempt(&attempt_info, volume, muted)
            })
            .await
            .expect("音频建流阻塞池 join");
            let t_audio = std::time::Instant::now();
            match attempt {
                AudioAttempt::Ready(pair) => {
                    if shared.shutdown.load(Ordering::Relaxed) {
                        return;
                    }
                    // 起播闸关着（视频还没出首帧）时从**起播锚点**开始喂——
                    // 和视频从同一媒体位置出发；闸已开（视频先出了）则从当前
                    // 播放位置接上，这就是迟到接管。
                    let at = {
                        let gate = shared.clock_gate.lock();
                        match &*gate {
                            super::ClockGate::Waiting { .. } => start,
                            super::ClockGate::Started => shared.now(),
                        }
                    };
                    log::info!(
                        "[Playback] 音频设备就绪（{}ms，累计 {}ms，从 {at:.1}s 起喂）",
                        t_audio.elapsed().as_millis(),
                        t_started.elapsed().as_millis()
                    );
                    adopt_audio(&shared, &slot, *pair, source, info, at).await;
                }
                AudioAttempt::Unusable => log::info!(
                    "[Playback] 音频设备不可用（累计 {}ms，无声）",
                    t_started.elapsed().as_millis()
                ),
                AudioAttempt::Pending(rx) => {
                    log::info!("[Playback] 音频设备建流卡住，先出画；建成后自动接上");
                    let Ok(Some(pair)) = rx.await else {
                        return;
                    };
                    if shared.shutdown.load(Ordering::Relaxed) {
                        return; // 玩家已走：产物随作用域析构，设备释放
                    }
                    let Some(info) = audio_info else {
                        return;
                    };
                    // 接管点：首帧还没到（闸关着）用起播锚点，否则用当前位置
                    let at = {
                        let gate = shared.clock_gate.lock();
                        match &*gate {
                            super::ClockGate::Waiting { .. } => start,
                            super::ClockGate::Started => shared.now(),
                        }
                    };
                    log::info!("[Playback] 音频设备迟到接管（从 {at:.1}s 接上）");
                    adopt_audio(&shared, &slot, pair, source, info, at).await;
                }
            }
        }));
    }

    Ok(Arc::new(PlayerHandle {
        shared,
        audio: audio_slot,
        tasks,
    }))
}

/// 把一对音频产物接进播放：起喂送 → 预填 → 启动设备 → 换主时钟。
///
/// 立即就绪与迟到接管共用这一段——两者唯一的差别是 `start_secs`：立即就绪
/// 时是用户要的续播点，迟到接管时是**当前已经播到的位置**（视频先走了）。
///
/// 顺序很重要：**先喂满一截再启动设备**。反过来的话设备开局先消费空缓冲，
/// 放出去的是静音；而时钟只计真实帧（见 `audio.rs` 回调），这段静音虽然
/// 不再推进时钟，但会白白拉长起播——设备在响、画面不动，读起来也是「卡」。
async fn adopt_audio(
    shared: &Arc<Shared>,
    slot: &Arc<parking_lot::Mutex<Option<AudioOut>>>,
    pair: (AudioOut, RingFeed),
    source: ByteSource,
    info: TrackInfo,
    start_secs: f64,
) {
    let (out, feed) = pair;
    let clock = Arc::clone(&out.clock_src);
    // 迟到接管（视频已经在播）时先把画面闸在接管点：接下来要建喂送、等
    // 预填，这段时间若让视频继续走，音频却从「准备开始时」的位置起播，
    // 声画就永久错开一整段准备时长（倍速下还按倍速放大）。
    let late_takeover = !matches!(
        &*shared.clock_gate.lock(),
        super::ClockGate::Waiting { .. }
    );
    if late_takeover {
        shared.hold_at(start_secs);
        log::info!("[Playback] 音频接管中：视频暂挂在 {start_secs:.2}s 等音频就绪");
    }
    // 倍速/音量/静音是用户偏好：新时钟与设备初值都要继承（新时钟环空，
    // drain=0 立即生效）
    clock.set_rate(shared.rate(), 0.0);
    clock.reset(start_secs);
    out.set_volume(f32::from_bits(shared.volume.load(Ordering::Relaxed)));
    out.set_muted(shared.muted.load(Ordering::Relaxed));
    // 暂停状态要带给刚接上的设备（起播时若已是暂停态）
    out.device
        .set_user_paused(shared.paused.load(Ordering::Relaxed));

    let (prime_tx, prime_rx) = tokio::sync::oneshot::channel();
    let feeder = {
        let shared = Arc::clone(shared);
        let source = SampleSource::new(source.clone(), Arc::new(info.samples.clone()));
        let start = start_secs;
        // 喂送任务挂在调用方的运行时上（`open` 的契约）；接管路径同样已在
        // tokio 上下文内（引擎任务都是），直接 spawn
        tokio::spawn(async move {
            audio_feeder(shared, feed, source, info, start, Some(prime_tx)).await;
        })
    };

    // 等预填（超时就把设备先启动：时钟只计真实帧，短期欠载不跑偏）
    if tokio::time::timeout(AUDIO_PRIME_TIMEOUT, prime_rx)
        .await
        .is_err()
    {
        log::info!(
            "[Playback] 音频预填 {}s 未就绪（字节还没供上来），先启动设备",
            AUDIO_PRIME_TIMEOUT.as_secs()
        );
    }
    // 设备与时钟**都等首帧**再起跑（见 `Shared::start_clock`）：视频从
    // 起点前的关键帧起解，预滚期间让时钟走就等于给首帧预支一截落后量。
    //
    // 两条路径分开处理——迟到接管**不能**把 gate 改回 `Waiting`：那时视频
    // 已被 `hold_at` 闸住，`now()` 恒等于锚点，队列里的帧永远不到期，
    // `present()` 不会被调用，也就没人再调 `start_clock` 来放行（实测死锁：
    // 画面停在接管点、音频设备也起不来）。所以迟到接管就地起跑。
    if late_takeover {
        *shared.clock.lock() = Arc::clone(&clock);
        out.device.activate();
        *shared.audio_device.lock() = Some(Arc::clone(&out.device));
        *slot.lock() = Some(out);
        shared.release_hold();
        log::info!("[Playback] 音频接管完成：设备起跑、画面放行（{start_secs:.2}s）");
        drop(feeder);
        return;
    }
    {
        let mut gate = shared.clock_gate.lock();
        match &mut *gate {
            super::ClockGate::Waiting { audio } => {
                *audio = Some((clock, Arc::clone(&out.device)));
            }
            super::ClockGate::Started => {
                // 窗口极窄：首帧恰好在这把锁之外先到（音频建流慢于首帧
                // 解码，且 gate 已由 start_clock 翻开）。就地起跑。
                *shared.clock.lock() = Arc::clone(&clock);
                out.device.activate();
                log::info!("[Playback] 音频设备迟到就绪：立即接管时钟");
            }
        }
    }
    *shared.audio_device.lock() = Some(Arc::clone(&out.device));
    *slot.lock() = Some(out);
    // 喂送任务已 spawn（detach），停机由 shared.shutdown 传递——
    // push 循环每 4ms 检查一次，句柄不需要留在 tasks 里
    drop(feeder);
}

/// 轨道的媒体时长（秒，presentation 轴：mdhd 时长 − elst 起点）。
fn media_end(info: &TrackInfo) -> f64 {
    if info.media_timescale > 0 && info.media_duration > 0 {
        let end = info.media_duration as f64 / f64::from(info.media_timescale);
        return (end - info.edit_media_start_secs).max(0.0);
    }
    info.sample_pts().last().copied().unwrap_or(0.0)
}

/// 建音频设备的上界等待。见 [`open`] 里该段的说明。
const AUDIO_OPEN_TIMEOUT: Duration = Duration::from_secs(5);

/// 连续两次尝试之间至少要隔的时间。
///
/// 卡住的建流线程永远回不来（CoreAudio 的 HAL 调用没有超时语义），放任每集
/// 都起一个新的就会叠出一堆僵尸线程、各自占着 HAL 连接。所以两次尝试之间
/// 拉开这个间隔——但**不是永久放弃**：设备重新插拔/系统恢复后，下一次
/// 尝试还能把声音找回来。
const AUDIO_RETRY_COOLDOWN: Duration = Duration::from_secs(30);

/// 建流**超时**（HAL 卡住而非快速失败）之后的冷却：HAL 卡死期间每 30s 叠
/// 一个僵尸线程只会更糟——实测卡死窗口里整个进程的定时器唤醒都被拖慢
/// （换集起播的 50ms 轮询被拉长到 20s，观感即「每次换集卡 20-40 秒」），
/// 拉长冷却让恢复期的风暴小一个量级。设备真恢复时最多等 5 分钟找回声音。
const AUDIO_TIMEOUT_COOLDOWN: Duration = Duration::from_secs(5 * 60);

/// 上一次建流尝试的时刻（Unix 毫秒；0 = 从未试过）。
static AUDIO_LAST_ATTEMPT_MS: AtomicU64 = AtomicU64::new(0);
/// 上一次尝试是以「超时」收场的（HAL 卡住的强信号）。
static AUDIO_LAST_TIMED_OUT: AtomicBool = AtomicBool::new(false);

/// 现在的 Unix 毫秒。
fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// 音频设备尝试的结果。
enum AudioAttempt {
    /// 在超时内建好：直接接上（Box 抹平与 Pending 的尺寸差，别让这个
    /// 枚举在栈上按最大变体展开）。
    Ready(Box<(AudioOut, RingFeed)>),
    /// 设备/音轨不可用（没有音轨、设备打不开、上次尝试还在冷却）。
    Unusable,
    /// 还没回来：视频先走，这个接收端等结果，到了就接管。
    Pending(tokio::sync::oneshot::Receiver<Option<(AudioOut, RingFeed)>>),
}

/// 建音频输出，最多等 [`AUDIO_OPEN_TIMEOUT`]。
///
/// 纯阻塞函数（`AUDIO_OPEN_TIMEOUT` 内按 20ms 轮询 HAL 结果），调用方放
/// 阻塞池——它可能卡住数秒，不能占异步 worker。
fn audio_attempt(audio: &TrackInfo, volume: f32, muted: bool) -> AudioAttempt {
    let (channels, sample_rate) = (audio.channels, audio.sample_rate);
    if channels == 0 || sample_rate == 0 {
        return AudioAttempt::Unusable;
    }
    let now = now_millis();
    let last = AUDIO_LAST_ATTEMPT_MS.load(Ordering::Acquire);
    let cooldown = if AUDIO_LAST_TIMED_OUT.load(Ordering::Acquire) {
        AUDIO_TIMEOUT_COOLDOWN
    } else {
        AUDIO_RETRY_COOLDOWN
    };
    if last != 0 && now.saturating_sub(last) < cooldown.as_millis() as u64 {
        log::info!(
            "[Playback] 距上次音频建流尝试不足 {}s，本次直接无声起播",
            cooldown.as_secs()
        );
        return AudioAttempt::Unusable;
    }
    AUDIO_LAST_ATTEMPT_MS.store(now, Ordering::Release);

    let (tx, rx) = tokio::sync::oneshot::channel();
    let spawned = std::thread::Builder::new()
        .name("hg-audio-open".into())
        .spawn(move || {
            // 接收端已弃（超时且玩家已走）时 send 失败即丢弃产物：设备释放
            let _ = tx.send(AudioOut::open(channels, sample_rate, volume, muted));
        });
    if let Err(e) = spawned {
        log::warn!("[Playback] 音频建流线程起不来（{e}），无声播放");
        return AudioAttempt::Unusable;
    }

    // 超时后仍要拿回接收端（接管任务等它），所以按 &mut 轮询而不是把
    // future 整个交给 timeout（超时会把 future 一起丢掉）
    let mut rx = rx;
    let deadline = std::time::Instant::now() + AUDIO_OPEN_TIMEOUT;
    loop {
        match rx.try_recv() {
            Ok(Some(pair)) => {
                // 建流成功：冷却重置，后续集数立刻可用
                AUDIO_LAST_ATTEMPT_MS.store(0, Ordering::Release);
                AUDIO_LAST_TIMED_OUT.store(false, Ordering::Release);
                return AudioAttempt::Ready(Box::new(pair));
            }
            Ok(None) => {
                log::warn!("[Playback] 音频设备不可用，退化为无声播放（挂钟）");
                AUDIO_LAST_TIMED_OUT.store(false, Ordering::Release);
                return AudioAttempt::Unusable;
            }
            Err(tokio::sync::oneshot::error::TryRecvError::Closed) => {
                log::warn!("[Playback] 音频建流线程异常退出，无声播放");
                AUDIO_LAST_TIMED_OUT.store(false, Ordering::Release);
                return AudioAttempt::Unusable;
            }
            Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {}
        }
        if std::time::Instant::now() >= deadline {
            log::warn!(
                "[Playback] 音频设备建流超过 {}s 未返回（CoreAudio 卡住），先出画、等它迟到接管",
                AUDIO_OPEN_TIMEOUT.as_secs()
            );
            // HAL 卡住的强信号：拉长冷却（见 AUDIO_TIMEOUT_COOLDOWN），别让
            // 僵尸线程每 30s 叠一个
            AUDIO_LAST_TIMED_OUT.store(true, Ordering::Release);
            return AudioAttempt::Pending(rx);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// 预填的等待上界：超时（网络还没把音频字节供上来）就先启动设备——
/// 时钟只计真实帧，短期欠载不会跑偏，只是画面会等音频一下。
const AUDIO_PRIME_TIMEOUT: Duration = Duration::from_secs(2);

#[cfg(test)]
mod tests {
    use super::*;

    /// media_end 在 elst 修剪下扣掉 edit 起点（priming 不该算进时长）。
    #[test]
    fn media_end_subtracts_edit_start() {
        let mut info = TrackInfo {
            media_timescale: 48_000,
            media_duration: 48_000 * 100,
            edit_media_start_secs: 2112.0 / 48_000.0,
            ..Default::default()
        };
        assert!((media_end(&info) - (100.0 - 2112.0 / 48_000.0)).abs() < 1e-9);
        info.media_timescale = 0;
        info.media_duration = 0;
        assert_eq!(media_end(&info), 0.0);
    }
}
