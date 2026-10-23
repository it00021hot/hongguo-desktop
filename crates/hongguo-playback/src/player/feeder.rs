//! 音频馈送循环：样本 → 平台 AAC 解码 → WSOLA 倍速 → 环形缓冲，含 seek
//! 重定位与缓冲水位状态机。

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use hongguo_video::sample_table::TrackInfo;

use crate::audio::{Clock, RingFeed};
use crate::source::SampleSource;
use crate::wsola::Wsola;

use super::{PlayerEvent, Shared};

/// ASC 的 audioObjectType（诊断：2=AAC-LC，5=HE-AAC v1/SBR，29=HE-AAC v2/PS）。
fn asc_aot(asc: &[u8]) -> Option<u32> {
    let b0 = *asc.first()?;
    let b1 = *asc.get(1)?;
    let aot = u32::from((b0 >> 3) & 0x1f);
    if aot == 31 {
        // 转义：再取 6 位，实际 profile = 32 + ext
        let ext = u32::from(((b0 & 0x07) << 3) | (b1 >> 5));
        Some(32 + ext)
    } else {
        Some(aot)
    }
}

/// 起播预填的包数下限：小环/短流时也别等太久（见 [`AUDIO_PRIME_SECS`]）。
const AUDIO_PRIME_PACKETS: usize = 8;

/// 预填包数上限：环一直填不满（采样率错配、流损坏）时不能无限等。
const AUDIO_PRIME_MAX_PACKETS: usize = 512;

/// 缓冲低水位：环形缓冲低于这个秒数 → 暂停设备 + Buffering（设备绝不
/// 空转放静音——主流一致，宁停不响空洞）。
pub(super) const BUFFER_LOW_SECS: f64 = 0.25;

/// 缓冲高水位：回填到这个秒数才恢复播放（滞回避免抖动）。
const BUFFER_HIGH_SECS: f64 = 0.6;

/// 起播预填目标（秒，设备域）：**填到高水位才启动设备**。
///
/// 以前按固定 8 包（AAC 一包 23ms 媒体，倍速下更短）就放行设备——环里
/// 只有几十毫秒，设备一启动就低于低水位，立刻触发「缓冲中 → 回填 →
/// 恢复」的假事件。每次起播/换集都会闪一下，倍速越高越短、越必现。
/// 预填到高水位后，设备起步就有滞回区间可用，不再有启动瞬态。
const AUDIO_PRIME_SECS: f64 = BUFFER_HIGH_SECS;

/// 音频侧诊断窗口：与视频/宿主同拍，日志时间戳可直接对齐。
const DIAG_WINDOW: Duration = Duration::from_secs(5);
/// 窗口内设备欠载超过它（毫秒）——用户能听到的断音，升级 warn。
const UNDERRUN_WARN_MS: f64 = 50.0;
/// 单次取样本等待超过它（毫秒）——供给卡顿，升级 warn。
const READ_WARN_MS: f64 = 500.0;

/// 音频侧 5s 窗口诊断：环水位、欠载、断流次数与各段耗时。
///
/// 倍速下「偶发卡顿」的音频侧表现是环被抽干：设备回调补静音（欠载）或
/// 水位状态机暂停设备（断流）。这两件事都必须有量化记录，否则日志里只有
/// 「播放中」和一片安静，分不出是没喂上还是设备没消费。
struct AudioDiag {
    window_started: Instant,
    /// 窗口内环水位的上下沿（秒，设备域）；跳过预填/冲流阶段。
    ring_min: f64,
    ring_max: f64,
    /// 水位暂停次数与累计时长。
    starve_count: u32,
    starve_started: Option<Instant>,
    starve_total: Duration,
    /// 设备回调欠载（毫秒）：窗口增量 + 窗口起点基线。
    silence_ms: f64,
    last_silence_ms: f64,
    read_max: Duration,
    decode_max: Duration,
    wsola_max: Duration,
    push_max: Duration,
    in_frames: u64,
    out_frames: u64,
    dropped_frames: u64,
    decode_errors: u32,
}

impl AudioDiag {
    fn new(silence_ms: f64) -> Self {
        Self {
            window_started: Instant::now(),
            ring_min: f64::INFINITY,
            ring_max: 0.0,
            starve_count: 0,
            starve_started: None,
            starve_total: Duration::ZERO,
            silence_ms: 0.0,
            last_silence_ms: silence_ms,
            read_max: Duration::ZERO,
            decode_max: Duration::ZERO,
            wsola_max: Duration::ZERO,
            push_max: Duration::ZERO,
            in_frames: 0,
            out_frames: 0,
            dropped_frames: 0,
            decode_errors: 0,
        }
    }

    fn observe_ring(&mut self, ring_secs: f64) {
        self.ring_min = self.ring_min.min(ring_secs);
        self.ring_max = self.ring_max.max(ring_secs);
    }

    fn note_silence(&mut self, current_ms: f64) {
        self.silence_ms += (current_ms - self.last_silence_ms).max(0.0);
        self.last_silence_ms = current_ms;
    }

    fn starve_started(&mut self) {
        self.starve_count += 1;
        self.starve_started = Some(Instant::now());
    }

    fn starve_ended(&mut self) -> Duration {
        let Some(started) = self.starve_started.take() else {
            return Duration::ZERO;
        };
        let duration = started.elapsed();
        self.starve_total += duration;
        duration
    }

    /// seek/换集后重置窗口：重定位造成的预填与冲流不是播放卡顿。
    fn reset(&mut self) {
        self.window_started = Instant::now();
        self.ring_min = f64::INFINITY;
        self.ring_max = 0.0;
        self.starve_count = 0;
        self.starve_started = None;
        self.starve_total = Duration::ZERO;
        self.silence_ms = 0.0;
        self.read_max = Duration::ZERO;
        self.decode_max = Duration::ZERO;
        self.wsola_max = Duration::ZERO;
        self.push_max = Duration::ZERO;
        self.in_frames = 0;
        self.out_frames = 0;
        self.dropped_frames = 0;
        self.decode_errors = 0;
    }

    /// 到点输出一行窗口摘要。断流/欠载/取流等待任一超阈值就升级 warn——
    /// 用户报「卡了一下」时，这行直接给出卡在音频侧的证据。
    fn report(&mut self, rate: f64) {
        if self.window_started.elapsed() < DIAG_WINDOW {
            return;
        }
        let elapsed = self.window_started.elapsed();
        let ring_min = if self.ring_min.is_finite() {
            self.ring_min
        } else {
            0.0
        };
        let level = if self.silence_ms >= UNDERRUN_WARN_MS
            || self.starve_count > 0
            || self.read_max.as_secs_f64() * 1000.0 >= READ_WARN_MS
        {
            log::Level::Warn
        } else {
            log::Level::Info
        };
        log::log!(
            level,
            "[Playback][audio] {:.1}s 窗口：环 {:.2}–{:.2}s，设备欠载 {:.0}ms，\
             水位暂停 {} 次（累计 {:.0}ms），读样本最大 {}ms/解码最大 {}ms/\
             WSOLA 最大 {}ms/推送等待最大 {}ms，入 {} 帧/出 {} 帧，\
             冲流丢弃 {} 帧，解码失败 {} 包（{:.2}x）",
            elapsed.as_secs_f64(),
            ring_min,
            self.ring_max,
            self.silence_ms,
            self.starve_count,
            self.starve_total.as_secs_f64() * 1000.0,
            self.read_max.as_millis(),
            self.decode_max.as_millis(),
            self.wsola_max.as_millis(),
            self.push_max.as_millis(),
            self.in_frames,
            self.out_frames,
            self.dropped_frames,
            self.decode_errors,
            rate,
        );
        self.reset();
    }
}

/// 喂送循环的游标账本：冲流收口状态（重预填包计数 + 是否在冲流中）与
/// 样本游标。seek 重定位一次改写全部三样，收拢一处免得散参乱序。
struct FeederCursor {
    idx: usize,
    pushed: usize,
    flush_pending: bool,
}

/// 喂送侧的 seek 重定位：冲解码器与适配器相位、开冲流、登记重预填。
///
/// 附带两件联动：徽标置缓冲态并发 Buffering（seek 后必然要重新缓冲）；
/// 解除水位暂停——冲流的丢弃收口要靠设备回调跑起来，暂停着的设备回调
/// 不来，凭证永远拿不到，冲流收不了口，水位重判又被冲流闸着，三方互等
/// 就是「拖进度一直缓冲中」的死局。用户暂停位不受影响（该静着还是静着）。
async fn feeder_relocate(
    shared: &Arc<Shared>,
    feed: &mut RingFeed,
    decoder: &mut hongguo_media_platform::platform::audio::AacDecoder,
    pts: &[f64],
    target: f64,
    cursor: &mut FeederCursor,
) {
    decoder.reset();
    feed.begin_flush();
    cursor.flush_pending = true;
    cursor.pushed = 0;
    cursor.idx = pts.partition_point(|&p| p < target - 1e-6);
    // 徽标只在**播放中**置缓冲态：暂停里 seek 的是静止画面（Tauri 同款
    // 语义——新位置的首帧立即可见），没有「在缓冲」这回事；恢复播放后
    // 水位状态机会按实际情况补发边沿。
    if !shared.paused.load(Ordering::Relaxed) {
        shared.buffered_ui.store(true, Ordering::Relaxed);
        shared.emit(PlayerEvent::Buffering).await;
    }
    feed.unstarve();
    log::info!(
        "[Playback] 音频 seek 重定位到 {target:.1}s（样本 {}/{}，冲流收口待重预填）",
        cursor.idx,
        pts.len()
    );
}

/// 音频喂送循环：样本 → 平台 AAC 解码 → WSOLA 倍速 → 环形缓冲。
///
/// 播放位置不在这里记账——时钟由输出回调按设备消费推进，喂送端只管把解码
/// 好的 PCM 源源不断放进缓冲。附带三件事：seek 重定位（冲流协议）、rebuffer
/// 水位状态机、播完后的挂钟接管。
///
/// `primed`：预填到 [`AUDIO_PRIME_SECS`]（高水位）时发一记信号（调用方
/// 据此启动设备，保证设备一响就有真实数据、且不会立刻触发低水位）。
pub(super) async fn audio_feeder(
    shared: Arc<Shared>,
    mut feed: RingFeed,
    mut source: SampleSource,
    info: TrackInfo,
    start_secs: f64,
    primed: Option<tokio::sync::oneshot::Sender<()>>,
) {
    let asc = info.decoder_config.clone();
    let (channels, sample_rate) = (info.channels, info.sample_rate);
    if channels == 0 || sample_rate == 0 || asc.is_empty() {
        shared
            .fail(format!(
                "音频轨信息不全（ch={channels} sr={sample_rate} asc={}B）",
                asc.len()
            ))
            .await;
        return;
    }
    // profile 留证：LC 还是 HE，一行日志钉死，后续排障不用猜
    match asc_aot(&asc) {
        Some(2) => log::info!("[Playback] 音频 profile：AOT 2（AAC-LC）"),
        Some(5) => log::info!("[Playback] 音频 profile：AOT 5（HE-AAC v1 / SBR）"),
        Some(29) => log::info!("[Playback] 音频 profile：AOT 29（HE-AAC v2 / PS）"),
        Some(other) => log::info!("[Playback] 音频 profile：AOT {other}"),
        None => log::warn!("[Playback] 音频 profile 解析不出（asc 首字节缺失）"),
    }

    let mut decoder = match hongguo_media_platform::platform::audio::AacDecoder::open(
        &asc,
        channels,
        sample_rate,
    ) {
        Ok(d) => d,
        Err(e) => {
            shared.fail(format!("AAC 解码器初始化失败: {e}")).await;
            return;
        }
    };
    let mut wsola = Wsola::new(sample_rate, channels);
    let ch = usize::from(channels);

    let mut pts = info.sample_pts();
    if pts.is_empty() {
        // 没有时间表：按每样本 1024 帧摊
        pts = (0..info.samples.len())
            .map(|i| i as f64 * 1024.0 / f64::from(sample_rate))
            .collect();
    }
    // 起点对齐：第一个 PTS ≥ 起点的样本（elst 修剪后，priming 样本已在 0 前）
    let mut cursor = FeederCursor {
        idx: pts.partition_point(|&p| p < start_secs - 1e-6),
        pushed: 0,
        flush_pending: false,
    };
    let total = pts.len().min(source.len());

    let mut primed = primed;
    // 预填阶段：水位状态机不判（环低于低水位是正常中间态，判了就是每次
    // 起播都发一次假 Buffering）。
    let mut priming = primed.is_some();
    // seek 广播的记账位（独立于视频循环，见 Shared::seek_broadcast）
    let mut seek_gen = 0u64;
    let mut diag = AudioDiag::new(feed.silence_ms());

    while cursor.idx < total {
        if shared.shutdown.load(Ordering::Relaxed) {
            return;
        }
        // 暂停 = 停喂（设备也停消费，时钟冻结）；seek 仍然处理（暂停中拖
        // 进度条：恢复后必须从新位置走）
        while shared.paused.load(Ordering::Relaxed) && !shared.shutdown.load(Ordering::Relaxed) {
            if let Some(t) = shared.poll_seek(&mut seek_gen) {
                feeder_relocate(
                    &shared,
                    &mut feed,
                    &mut decoder,
                    pts.as_slice(),
                    t,
                    &mut cursor,
                )
                .await;
                diag.reset();
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        if shared.shutdown.load(Ordering::Relaxed) {
            return;
        }
        if let Some(t) = shared.poll_seek(&mut seek_gen) {
            feeder_relocate(
                &shared,
                &mut feed,
                &mut decoder,
                pts.as_slice(),
                t,
                &mut cursor,
            )
            .await;
            diag.reset();
            continue;
        }
        // 读样本带读取代次：seek 会抬代次，堵在旧位置上的 serve 请求尽早
        // 退场（注销读者登记，填充调度器立刻转向新位置）——干等旧数据的话
        // seek 的就绪被白白拖后一个区间。
        let my_epoch = shared.read_epoch.load(Ordering::Relaxed);
        let shared_for_read = Arc::clone(&shared);
        let t_read = Instant::now();
        let sample = match source
            .read_cancellable(cursor.idx, move || {
                shared_for_read.read_epoch.load(Ordering::Relaxed) != my_epoch
            })
            .await
        {
            Ok(s) => s,
            Err(e) => {
                if shared.seek_pending_since(seek_gen) {
                    continue; // seek 引起的退场：循环顶上重定位
                }
                shared
                    .fail(format!("取音频样本 {} 失败: {e}", cursor.idx))
                    .await;
                return;
            }
        };
        diag.read_max = diag.read_max.max(t_read.elapsed());
        let t_decode = Instant::now();
        let decoded = match decoder.decode(&sample) {
            Ok(d) => d,
            Err(e) => {
                // 单包解不出来不该整场静音，但也**不能跳过**：跳过等于把这段
                // 时间从音频里抹掉，音轨比画面短一截——每跳一包就多攒一份
                // 音画差。补一包等长静音，时间轴严丝合缝。
                diag.decode_errors += 1;
                log::warn!("[Playback] 音频样本 {} 解码失败（补静音）: {e}", cursor.idx);
                vec![0f32; 1024 * ch]
            }
        };
        diag.decode_max = diag.decode_max.max(t_decode.elapsed());
        // 倍速伸缩（rate 变化逐窗生效）
        let rate = shared.rate();
        let mut stretched = Vec::with_capacity(decoded.len() / 2);
        let t_wsola = Instant::now();
        wsola.process(&decoded, rate, &mut stretched);
        diag.wsola_max = diag.wsola_max.max(t_wsola.elapsed());
        diag.in_frames += (decoded.len() / ch) as u64;
        diag.out_frames += (stretched.len() / ch) as u64;
        if !stretched.is_empty() {
            // 冲流期环放不下就丢这包：老数据还没被回调丢完、设备又暂停着时
            //（用户暂停/水位暂停），等在 push 上会让喂送任务连同冲流收口、
            // 水位重判一起卡死——「拖进度一直缓冲中」的根因。冲流边界本来
            // 就是要丢的内容；时间轴的连续由时钟换轴负责，不靠这包撑着。
            let need_secs = stretched.len() as f64 / ch as f64 / f64::from(sample_rate);
            if cursor.flush_pending && feed.vacant_secs() < need_secs {
                diag.dropped_frames += (stretched.len() / ch) as u64;
                log::debug!("[Playback] 冲流期环满，丢一包等回调腾位");
            } else {
                let t_push = Instant::now();
                if !feed
                    .push(&stretched, &shared.shutdown, &shared.paused)
                    .await
                {
                    return;
                }
                diag.push_max = diag.push_max.max(t_push.elapsed());
                cursor.pushed += 1;
            }
        }
        // 冲流收口：预填够 + 设备确实跑过丢弃周期（设备暂停时回调不来，
        // 宁可等恢复也别让旧声音漏出去）
        if cursor.flush_pending && cursor.pushed >= AUDIO_PRIME_PACKETS && feed.end_flush_if_seen()
        {
            cursor.flush_pending = false;
        }
        // 预填够高水位就放行设备（见 AUDIO_PRIME_SECS）
        if priming {
            let enough = feed.buffered_secs() >= AUDIO_PRIME_SECS
                || cursor.pushed >= AUDIO_PRIME_MAX_PACKETS;
            if enough {
                priming = false;
                if let Some(tx) = primed.take() {
                    log::info!(
                        "[Playback] 音频预填完成（{:.2}s，{} 包），设备可以起跑",
                        feed.buffered_secs(),
                        cursor.pushed
                    );
                    let _ = tx.send(());
                }
            }
        }
        // 水位状态机（冲流期间不判：环里混着待丢弃的旧数据；预填期间不判：
        // 还没起播，环浅是正常中间态）
        if !cursor.flush_pending && !priming {
            let ring_secs = feed.buffered_secs();
            diag.observe_ring(ring_secs);
            if ring_secs <= BUFFER_LOW_SECS && feed.set_starved(true) {
                diag.starve_started();
                log::info!("[Playback] 缓冲低水位（{ring_secs:.2}s），暂停设备等填充");
            } else if ring_secs >= BUFFER_HIGH_SECS && feed.set_starved(false) {
                let starved = diag.starve_ended();
                log::info!(
                    "[Playback] 缓冲回填（{ring_secs:.2}s，断流 {:.0}ms），恢复播放",
                    starved.as_secs_f64() * 1000.0
                );
            }
            // UI 徽标边沿只按 buffered_ui 翻转：seek 路径上 starved 被提前
            // 解除（收冲流的口），靠它的翻转沿发事件会把 Ready 永远丢掉。
            if ring_secs <= BUFFER_LOW_SECS && !shared.buffered_ui.swap(true, Ordering::Relaxed) {
                shared.emit(PlayerEvent::Buffering).await;
            } else if ring_secs >= BUFFER_HIGH_SECS
                && shared.buffered_ui.swap(false, Ordering::Relaxed)
            {
                shared.emit(PlayerEvent::Ready).await;
            }
        }
        diag.note_silence(feed.silence_ms());
        if diag.window_started.elapsed() >= DIAG_WINDOW {
            diag.report(shared.rate());
        }
        cursor.idx += 1;
        // 喂送心跳（debug）：喂到哪、环多满、各状态位——排「喂送卡死/失聪」
        // 类问题的第一现场
        if cursor.idx.is_multiple_of(200) {
            log::debug!(
                "[Playback][feeder] idx {}/{} 环 {:.2}s paused={} flush={} gen={} 入{} 出{} 丢{}",
                cursor.idx,
                total,
                feed.buffered_secs(),
                shared.paused.load(Ordering::Relaxed),
                cursor.flush_pending,
                seek_gen,
                diag.in_frames,
                diag.out_frames,
                diag.dropped_frames
            );
        }
    }
    // 流很短或一直没喂满：别让调用方等满超时，直接放行
    if let Some(tx) = primed.take() {
        let _ = tx.send(());
    }

    // 喂完：水位暂停就此作废——后面不会再有数据，「等填充」永远等不到，
    // 设备必须跑起来把环里最后一段放干，下面的放干等待才有尽头（否则
    // 尾段一次低水位就能把设备暂停在残量上：环放不干、挂钟接不了、
    // 时钟冻结、徽标常亮——seek 到音频轨末尾尤其必现）。
    feed.unstarve();
    // 徽标若还亮着就此收口：不会再有回填沿了
    if shared.buffered_ui.swap(false, Ordering::Relaxed) {
        shared.emit(PlayerEvent::Ready).await;
    }

    // 喂完：等设备把环形缓冲放干，再把主时钟切到挂钟——音频时钟在缓冲
    // 放干后不再推进，视频尾巴（比音轨长时）会被冻住，Ended 也到不了。
    while feed.buffered_secs() > 0.0 && !shared.shutdown.load(Ordering::Relaxed) {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let at = shared.now();
    let rate = shared.rate();
    let wall = Arc::new(Clock::wall());
    // 挂钟无环，drain=0 立即换轴（rate() 是目标值，此刻旧的分级等待已无意义）
    wall.set_rate(rate, 0.0);
    wall.reset(at);
    shared.set_clock(wall);
    log::info!("[Playback] 音频喂送结束（{at:.1}s 起切挂钟收尾）");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ASC 的 AOT 解析：LC 与 HE 两族都要读对（诊断日志的地基）。
    /// 位序：首 5 位 = AOT；31 时再取 6 位（实际 profile = 32 + ext）。
    #[test]
    fn asc_aot_parses_lc_and_he() {
        // AOT=2（LC）：00010 → 0b0001_0xxx
        assert_eq!(asc_aot(&[0x10, 0x00]), Some(2));
        // AOT=5（HE v1）：00101 → 0b0010_1xxx
        assert_eq!(asc_aot(&[0x29, 0x00]), Some(5));
        // AOT=29（HE v2）：11101 → 0xE8 高 5 位
        assert_eq!(asc_aot(&[0xE8, 0x00]), Some(29));
        // AOT=31 转义：11111 + 000000 → 32
        assert_eq!(asc_aot(&[0xF8, 0x00]), Some(32));
        assert_eq!(asc_aot(&[]), None);
    }
}
