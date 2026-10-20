//! Phase 0 指标测量：2x 上屏节拍、seek 精度、切集空档、变速音高配置。
//!
//! 采样方式：独立线程按 2ms 轮询 `AVPlayerItemVideoOutput`，只在
//! `hasNewPixelBufferForItemTime:` 为真时取帧并记录显示时间戳。这测的是
//! **播放器实际产出的帧序列**（不是 GPUI 的 layout/paint），和计划里
//! 「播放器呈现计数，不看 `[Home][paint]`」同口径；`AVPlayerLayer` 的
//! 合成由窗口服务器完成，进程内看不到逐刷新呈现，故用帧间隔/帧率判定
//! 合并（详见结论的测量边界）。
//!
//! `AVPlayerItemVideoOutput` 在后台线程取帧是 Apple 允许的用法；`AVPlayer`
//! 的播放控制（play/rate/seek）也在测量线程做，主线程只负责窗口与图层。

use std::time::{Duration, Instant};

use objc2::runtime::AnyObject;
use objc2_foundation::NSArray;

use super::{avf, loader};

/// 2ms 轮询：25fps 帧间隔 40ms、2x 下 20ms，2ms 粒度足以抓到每一帧。
const POLL_INTERVAL: Duration = Duration::from_millis(2);
/// 每次变速后先跑 600ms 再统计：避开起播/解码器预热的第一两帧。
const WARMUP: Duration = Duration::from_millis(600);
/// 判定“掉帧/合并”的间隔阈值：期望帧间隔的 1.5 倍。
const DROP_FACTOR: f64 = 1.5;

struct FrameSample {
    /// 相对本次采样起点的墙钟秒数。
    wall_secs: f64,
    /// 帧的媒体显示时间（源时间轴，未乘倍速）。
    media_secs: f64,
}

struct SeekOutcome {
    target_frame: i64,
    current_secs: f64,
    shown_frame: Option<i64>,
    /// 从发起 seek 到目标帧可用（含等解码/上屏管线就绪）。
    to_frame_ms: Option<f64>,
}

pub fn run(player_addr: usize, layer_addr: usize, refresh_hz: f64) -> ! {
    // SAFETY: player 由主线程创建并泄漏，引用覆盖整个进程；测量线程退出前
    // 不会有人释放它。
    let player: &AnyObject = unsafe { &*(player_addr as *const AnyObject) };

    let asset = avf::custom_url("ep1").expect("自定义 scheme URL 构造失败");
    let asset = avf::asset_with_url(&asset);
    loader::attach(&asset);
    let item = avf::item_with_asset(&asset);
    avf::set_spectral_pitch(&item);
    let output = avf::video_output();
    avf::add_output(&item, &output);
    avf::replace_item(player, &item);
    avf::set_volume(player, 0.35);

    avf::play(player);
    if !wait_ready(&item, 8.0) {
        log::error!("[AvSpike] item 8s 内未 ready，spike 中止");
        std::process::exit(1);
    }
    let fps = f64::from(avf::nominal_fps(&asset));
    let duration = avf::duration(&asset).seconds();
    log::info!(
        "[AvSpike] item ready: {fps:.1}fps / {duration:.3}s / pitch={:?} / loader_requests={} / loader_bytes={}",
        avf::audio_pitch_algorithm(&item),
        loader::REQUESTS.load(std::sync::atomic::Ordering::Relaxed),
        loader::BYTES_SERVED.load(std::sync::atomic::Ordering::Relaxed),
    );

    for (label, rate, seconds) in [("1x", 1.0_f32, 5.0_f64), ("2x", 2.0, 8.0), ("3x", 3.0, 5.0)] {
        report_playback(player, &output, label, rate, fps, refresh_hz, seconds);
    }

    // 1x 跑完再查：首帧之前 `isReadyForDisplay` 可能还是 false，不能当作失败。
    report_layer_ready(layer_addr);

    // seek 精度：暂停后零容差 seek（精确到帧与倍速无关），逐帧目标包含 ±1 帧。
    avf::pause(player);
    let mut outcomes = Vec::new();
    for frame in [0_i64, 1, 60, 61, 299, 300, 301, 450, 499, 300] {
        outcomes.push(seek_case(player, &output, frame, fps));
    }
    // 2x 播放中 seek：只要求落点 ±2 帧（seek 期间播放仍在推进），单独记录。
    seek_while_playing(player, &output, 200, fps);
    report_seek_summary(&outcomes, fps);

    // 切集：AVQueuePlayer 预加载下一项，advanceToNextItem 接力。
    let cutover_1x = cutover(player, &asset, fps, 1.0, 5);
    let cutover_2x = cutover(player, &asset, fps, 2.0, 3);
    log::info!(
        "[AvSpike] cutover summary rate=1x n={} median={:.1}ms max={:.1}ms",
        cutover_1x.len(),
        median(&cutover_1x),
        cutover_1x.last().copied().unwrap_or(0.0)
    );
    log::info!(
        "[AvSpike] cutover summary rate=2x n={} median={:.1}ms max={:.1}ms",
        cutover_2x.len(),
        median(&cutover_2x),
        cutover_2x.last().copied().unwrap_or(0.0)
    );

    log::info!(
        "[AvSpike] pitch: algorithm=Spectral（系统频谱变速）；音高客观测量需要录音回环，本 spike 未做"
    );
    log::info!("[AvSpike] 全部场景完成，进程退出");
    std::process::exit(0);
}

/// 原生图层的 `isReadyForDisplay`：证明 AVPlayerLayer 真的挂上了内容，而不是
/// 只在视频输出侧解出帧。读属性在后台线程安全。
fn report_layer_ready(layer_addr: usize) {
    let layer: &AnyObject = unsafe { &*(layer_addr as *const AnyObject) };
    let ready: bool = unsafe { objc2::msg_send![layer, isReadyForDisplay] };
    log::info!("[AvSpike] AVPlayerLayer ready_for_display={ready}");
}

fn wait_ready(item: &AnyObject, timeout_secs: f64) -> bool {
    let deadline = Instant::now() + Duration::from_secs_f64(timeout_secs);
    while Instant::now() < deadline {
        match avf::item_status(item) {
            1 => return true,
            2 => {
                log::error!("[AvSpike] AVPlayerItem 状态 failed");
                return false;
            }
            _ => std::thread::sleep(Duration::from_millis(5)),
        }
    }
    false
}

fn report_playback(
    player: &AnyObject,
    output: &AnyObject,
    label: &str,
    rate: f32,
    content_fps: f64,
    refresh_hz: f64,
    seconds: f64,
) {
    avf::pause(player);
    avf::seek_exact(player, avf::CmTime::ZERO);
    wait_current_time(player, 0.0, Duration::from_secs(2));
    avf::set_rate(player, rate);
    std::thread::sleep(WARMUP);

    let samples = collect(output, player, seconds);
    if samples.len() < 3 {
        log::warn!("[{label}] 采样帧数不足：{}", samples.len());
        return;
    }
    let elapsed = samples.last().unwrap().wall_secs - samples[0].wall_secs;
    let presented_fps = (samples.len() - 1) as f64 / elapsed;
    let expected_fps = content_fps * f64::from(rate);
    let expected_gap = 1.0 / expected_fps;
    let mut gaps: Vec<f64> = samples
        .windows(2)
        .map(|pair| pair[1].wall_secs - pair[0].wall_secs)
        .collect();
    gaps.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let max_gap = gaps.last().copied().unwrap_or(0.0);
    let p99 = gaps[(gaps.len() as f64 * 0.99) as usize].min(max_gap);
    let drops = gaps
        .iter()
        .filter(|gap| **gap > expected_gap * DROP_FACTOR)
        .count();
    let refresh_per_frame = if presented_fps > 0.0 {
        refresh_hz / presented_fps
    } else {
        0.0
    };
    log::info!(
        "[AvSpike] {label} rate={rate:.1} frames={} presented={presented_fps:.1}fps expected={expected_fps:.1}fps \
         gap_med={:.1}ms gap_p99={:.1}ms gap_max={:.1}ms drops={drops} refresh/frame={refresh_per_frame:.2}",
        samples.len(),
        gaps[gaps.len() / 2] * 1000.0,
        p99 * 1000.0,
        max_gap * 1000.0,
    );
    log::info!(
        "[AvSpike] {label} 媒体时间 {:.3}s..{:.3}s（源时间轴）",
        samples.first().unwrap().media_secs,
        samples.last().unwrap().media_secs
    );
}

fn collect(output: &AnyObject, player: &AnyObject, seconds: f64) -> Vec<FrameSample> {
    let started = Instant::now();
    let mut samples = Vec::new();
    let mut last_media: Option<avf::CmTime> = None;
    while started.elapsed().as_secs_f64() < seconds {
        let now = avf::current_time(player);
        if avf::has_new_frame(output, now)
            && let Some(display) = avf::frame_display_time(output, now)
            && Some(display) != last_media
        {
            last_media = Some(display);
            samples.push(FrameSample {
                wall_secs: started.elapsed().as_secs_f64(),
                media_secs: display.seconds(),
            });
        }
        std::thread::sleep(POLL_INTERVAL);
    }
    samples
}

fn seek_case(player: &AnyObject, output: &AnyObject, frame: i64, fps: f64) -> SeekOutcome {
    avf::set_rate(player, 0.0);
    let target_secs = frame as f64 / fps;
    let started = Instant::now();
    avf::seek_exact(player, avf::CmTime::secs(target_secs));
    let settle_deadline = started + Duration::from_secs(2);
    let mut settled = false;
    while Instant::now() < settle_deadline {
        if (avf::current_time(player).seconds() - target_secs).abs() <= 0.001 {
            settled = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    let settle_ms = started.elapsed().as_secs_f64() * 1000.0;

    let shown_frame = wait_shown_frame(output, player, frame, fps, Duration::from_secs(1));
    let to_frame_ms = shown_frame.map(|_| started.elapsed().as_secs_f64() * 1000.0);
    let current_secs = avf::current_time(player).seconds();
    log::info!(
        "[AvSpike] seek target={frame} ({target_secs:.3}s) current={current_secs:.3}s \
         settled={settled} shown={shown_frame:?} err={} settle={settle_ms:.1}ms to_frame={}",
        shown_frame.map_or(999, |got| (got - frame).abs()),
        to_frame_ms.map_or("timeout".to_owned(), |ms| format!("{ms:.1}ms")),
    );
    SeekOutcome {
        target_frame: frame,
        current_secs,
        shown_frame,
        to_frame_ms,
    }
}

fn seek_while_playing(player: &AnyObject, output: &AnyObject, frame: i64, fps: f64) {
    let target_secs = frame as f64 / fps;
    avf::set_rate(player, 2.0);
    std::thread::sleep(Duration::from_millis(300));
    let started = Instant::now();
    avf::seek_exact(player, avf::CmTime::secs(target_secs));
    let shown = wait_shown_frame(output, player, frame, fps, Duration::from_secs(2));
    avf::pause(player);
    log::info!(
        "[AvSpike] seek@2x target={frame} ({target_secs:.3}s) current={:.3}s shown={shown:?} \
         err={} total={:.1}ms",
        avf::current_time(player).seconds(),
        shown.map_or(999, |got| (got - frame).abs()),
        started.elapsed().as_secs_f64() * 1000.0
    );
}

fn wait_shown_frame(
    output: &AnyObject,
    player: &AnyObject,
    target_frame: i64,
    fps: f64,
    timeout: Duration,
) -> Option<i64> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        let current = avf::current_time(player);
        if avf::has_new_frame(output, current)
            && let Some(display) = avf::frame_display_time(output, current)
        {
            let shown = (display.seconds() * fps).round() as i64;
            if (shown - target_frame).abs() <= 2 {
                return Some(shown);
            }
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    None
}

fn report_seek_summary(outcomes: &[SeekOutcome], fps: f64) {
    let max_err = outcomes
        .iter()
        .map(|outcome| {
            outcome
                .shown_frame
                .map_or(999, |shown| (shown - outcome.target_frame).abs())
        })
        .max()
        .unwrap_or(999);
    let mut to_frames: Vec<f64> = outcomes
        .iter()
        .filter_map(|outcome| outcome.to_frame_ms)
        .collect();
    to_frames.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let max_current_err = outcomes
        .iter()
        .map(|outcome| (outcome.current_secs - outcome.target_frame as f64 / fps).abs())
        .fold(0.0_f64, f64::max);
    log::info!(
        "[AvSpike] seek summary: n={} max_frame_err={max_err} median_to_frame={:.1}ms \
         max_to_frame={:.1}ms max_current_err={:.1}ms",
        outcomes.len(),
        to_frames[to_frames.len() / 2],
        to_frames.last().copied().unwrap_or(0.0),
        max_current_err * 1000.0,
    );
}

fn cutover(player: &AnyObject, asset_a: &AnyObject, fps: f64, rate: f32, runs: usize) -> Vec<f64> {
    avf::pause(player);
    let asset_b_url = avf::custom_url("ep2").expect("ep2 URL");
    let asset_b = avf::asset_with_url(&asset_b_url);
    loader::attach(&asset_b);
    let mut gaps = Vec::new();

    for run in 0..runs {
        let item_a = avf::item_with_asset(asset_a);
        let output_a = avf::video_output();
        avf::add_output(&item_a, &output_a);
        let item_b = avf::item_with_asset(&asset_b);
        avf::set_spectral_pitch(&item_b);
        let output_b = avf::video_output();
        avf::add_output(&item_b, &output_b);
        let items = NSArray::from_slice(&[&*item_a, &*item_b]);
        let queue = avf::queue_player(&items);
        if !wait_ready(&item_a, 5.0) {
            log::warn!("[AvSpike] cutover 第 {} 轮 item A 未 ready", run + 1);
            continue;
        }
        avf::set_volume(&queue, 0.2);
        avf::play(&queue);
        if rate != 1.0 {
            avf::set_rate(&queue, rate);
        }
        std::thread::sleep(Duration::from_millis(1200));

        // 队列只预加载字节，下一项 status 仍是 unknown——这正是要记录的事实。
        let status_before = avf::item_status(&item_b);
        let started = Instant::now();
        avf::advance_to_next_item(&queue);
        let first = wait_first_frame(&output_b, &queue, Duration::from_secs(2));
        let gap_ms = started.elapsed().as_secs_f64() * 1000.0;
        gaps.push(gap_ms);
        log::info!(
            "[AvSpike] cutover #{} rate={rate:.1} first_frame={:?} gap={gap_ms:.1}ms \
             itemB_status_before={status_before}",
            run + 1,
            first.map(|display| (display.seconds() * fps).round() as i64),
        );
        avf::pause(&queue);
        drop(queue);
        std::thread::sleep(Duration::from_millis(250));
    }

    gaps.sort_by(|a, b| a.partial_cmp(b).unwrap());
    gaps
}

fn wait_first_frame(
    output: &AnyObject,
    queue: &AnyObject,
    timeout: Duration,
) -> Option<avf::CmTime> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        let current = avf::current_time(queue);
        if avf::has_new_frame(output, current)
            && let Some(display) = avf::frame_display_time(output, current)
        {
            return Some(display);
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    None
}

fn wait_current_time(player: &AnyObject, target: f64, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if (avf::current_time(player).seconds() - target).abs() <= 0.001 {
            return;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn median(sorted: &[f64]) -> f64 {
    if sorted.is_empty() {
        0.0
    } else {
        sorted[sorted.len() / 2]
    }
}
