//! 视频轨：平台逐帧解码（HEVC / H.264，VT / MF）+ 按主时钟、**按显示序**上屏。
//!
//! 解码从**起点前的最近关键帧**起步（关键帧之前的样本是必需参考帧，产物
//! 帧按 PTS 过滤掉不上屏）。节奏对主时钟：`now() >= 帧显示时间` 才发出去。
//!
//! **显示序重排**：VTDecompressionSession 按解码序吐帧，而带 B 帧的码流里
//! 显示序 ≠ 解码序（`ctts` 偏移可以为负）。解出一帧就上屏的话，画面会在
//! 相邻几秒之间来回跳（10s→8s→9s），观感正是「同样的内容重放几遍」。
//! 所以：解出的帧先按显示时间排进重排缓冲，等「已解码的 dts 水位」越过
//! 安全线（水位 - 最大负偏移）才定稿，定稿后按显示时间升序、到点才上屏。
//! 无 `ctts` 的流（绝大多数）安全线即时成立，行为与逐帧直发一致。

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use hongguo_common::AppResult;
use hongguo_media_platform::platform::video::{
    DecoderInit, VideoCodec, VideoFrame, VideoFrameDecoder,
};
use hongguo_video::sample_table::TrackInfo;

use crate::player::{PlayerEvent, Shared};
use crate::source::SampleSource;

/// 起播样本号：起点前最近的**关键帧**（全关键帧轨直接取第一个 ≥ 起点的样本）。
pub(crate) fn start_index(info: &TrackInfo, start_secs: f64) -> usize {
    let pts = info.sample_pts();
    let eps = 1e-6;
    match &info.stss {
        None => {
            // 全关键帧轨：按**解码时间**找起点。pts 在带 B 帧时非单调，
            // partition_point 会指错；dts 恒递增，越过它前先多解一帧无害
            //（显示时间早于起点的产物帧由下面的过滤丢掉）。
            let dts = info.sample_dts();
            if dts.is_empty() {
                pts.partition_point(|&p| p < start_secs - eps)
            } else {
                dts.partition_point(|&d| d < start_secs - eps)
            }
        }
        Some(sync) => {
            let mut best = 0usize;
            for &n in sync {
                let i = n as usize - 1; // stss 是 1 基
                if i < pts.len() && pts[i] <= start_secs + eps {
                    best = best.max(i);
                }
            }
            best
        }
    }
}

/// 平台解码器初始化参数的 owned 形态（视频任务跨执行器搬运；借用形态见
/// `platform::video::DecoderInit`）。
pub(crate) struct DecoderInitOwned {
    pub(crate) codec: VideoCodec,
    pub(crate) parameter_sets: Vec<Vec<u8>>,
    /// 序列头原始载荷（HEVC 的 hvcC / H.264 的 avcC；Windows MF 的形态，
    /// macOS VT 不吃）。
    pub(crate) sequence_header: Option<Vec<u8>>,
    pub(crate) length_size: usize,
    pub(crate) width: usize,
    pub(crate) height: usize,
}

impl DecoderInitOwned {
    fn open(&self) -> AppResult<VideoFrameDecoder> {
        VideoFrameDecoder::open(DecoderInit {
            codec: self.codec,
            parameter_sets: &self.parameter_sets,
            sequence_header: self.sequence_header.as_deref(),
            length_size: self.length_size,
            width: self.width,
            height: self.height,
            // FullRange 请求是 macOS VT 的语义；MF 输出恒 limited，由渲染
            // 转换端按 BT.709 limited 处理——两端观感一致
            full_range: true,
        })
    }
}

/// 重排缓冲装得下的东西：能读出显示时间即可。
///
/// 抽成 trait 是为了让重排逻辑可单测——真帧只能从解码器出来（没有公开
/// 构造器），测试里造不出真帧，但重排的正确性（显示序单调）必须能脱离
/// 解码器验证。
pub(crate) trait HasDisplayTime {
    fn display_secs(&self) -> f64;
}

impl HasDisplayTime for VideoFrame {
    fn display_secs(&self) -> f64 {
        self.pts_secs
    }
}

/// 按显示时间把帧插进升序缓冲（同刻保持到达序，`partition_point` 是上界）。
pub(crate) fn insert_by_pts<F: HasDisplayTime>(pending: &mut Vec<F>, frame: F) {
    let at = pending.partition_point(|f| f.display_secs() <= frame.display_secs());
    pending.insert(at, frame);
}

/// 定稿：显示时间不晚于水位（`已解码 dts - 最大负偏移`）的帧，不可能再有
/// 更早的帧解出来，可以进上屏队列。保持显示序升序。
///
/// 返回本批条数（0 = 还没到定稿线）。
pub(crate) fn commit_ready<F: HasDisplayTime>(
    pending: &mut Vec<F>,
    watermark: f64,
    ready: &mut VecDeque<F>,
) -> usize {
    let n = pending.partition_point(|f| f.display_secs() <= watermark);
    ready.extend(pending.drain(..n));
    n
}

/// 上屏一帧（进帧槽 + 发事件）。`first_sent` 只在第一次翻开。
async fn present(
    frame: VideoFrame,
    shared: &Shared,
    first_sent: &mut bool,
    shown_frames: &AtomicU64,
) {
    let (w, h) = frame.dims;
    // 帧槽留给「随手取最新帧」的调用方；事件里带一份给事件驱动的宿主
    //（克隆只是引用计数 +1，不搬数据：macOS CVPixelBuffer / Windows Arc）
    *shared.latest.lock() = Some(frame.clone());
    if !*first_sent {
        *first_sent = true;
        log::info!(
            "[Playback] 首帧 {w}x{h}（显示时间 {:.2}s，时长 {:.1}s）",
            frame.pts_secs,
            *shared.duration_secs.lock()
        );
        shared
            .emit(PlayerEvent::FirstFrame {
                width: w,
                height: h,
            })
            .await;
    }
    let shown = shown_frames.fetch_add(1, Ordering::Relaxed);
    if shown.is_multiple_of(100) {
        // skew = 这一帧的显示时间 − 上屏瞬间的时钟：
        //   负 = 帧迟到（画面内容落后于声音），正 = 超跑。
        // **这是音画是否对齐的唯一可读指标**——光看「帧在动」分不出对齐。
        let skew = frame.pts_secs - shared.now();
        if skew.abs() > 0.15 {
            log::warn!(
                "[Playback][sync] 第 {shown} 帧偏差 {skew:+.3}s（画面{}）",
                if skew < 0.0 { "落后" } else { "超跑" }
            );
        } else {
            log::debug!(
                "[Playback][video] 已上屏 {shown} 帧（{:.2}s），clock={:.2}，偏差 {skew:+.3}s",
                frame.pts_secs,
                shared.now()
            );
        }
    }
    shared.emit(PlayerEvent::Frame(frame)).await;
}

/// 把已到期（显示时间 ≤ 时钟）的帧发出去，**一帧都不丢**。
/// 返回 `false` = 收到停机信号。
///
/// 解码追不上时的策略（明确约定：不丢内容，声音视频都一样）：**慢下来播**。
/// 时钟由音频回调推进（声音不能跳帧），视频追不上就晚点显示——两者仍按
/// 同一条时间轴对齐，只是这一段播放变慢（像直播进缓冲），绝不跳过内容。
/// 真实瓶颈（解码 46.5ms/帧 > 33ms 实时线）另行解决，慢播只是兜底。
async fn emit_due(
    ready: &mut VecDeque<VideoFrame>,
    shared: &Shared,
    first_sent: &mut bool,
    shown_frames: &AtomicU64,
) -> bool {
    loop {
        if shared.shutdown.load(Ordering::Relaxed) {
            return false;
        }
        let Some(front) = ready.front() else {
            return true;
        };
        if front.pts_secs > shared.now() {
            return true;
        }
        let frame = ready.pop_front().expect("front 刚看过");
        present(frame, shared, first_sent, shown_frames).await;
    }
}

/// 等到队首帧到期并发出去（解码跑太快时的节拍闸门）。
/// 返回 `false` = 收到停机信号。
async fn wait_front_due(
    ready: &mut VecDeque<VideoFrame>,
    shared: &Shared,
    first_sent: &mut bool,
    shown_frames: &AtomicU64,
) -> bool {
    loop {
        if shared.shutdown.load(Ordering::Relaxed) {
            return false;
        }
        let Some(front) = ready.front() else {
            return true;
        };
        if front.pts_secs <= shared.now() {
            return emit_due(ready, shared, first_sent, shown_frames).await;
        }
        // 时钟经 shared 取：音频设备迟到接管时会换主时钟，这里自动跟上
        tokio::time::sleep(std::time::Duration::from_millis(3)).await;
    }
}

/// 视频主循环。返回自然播完（中途退出由停机/事件错误控制）。
pub(crate) async fn run(
    shared: Arc<Shared>,
    mut source: SampleSource,
    info: TrackInfo,
    init: DecoderInitOwned,
    start_secs: f64,
) {
    let mut start_secs = start_secs;
    let mut decoder = match init.open() {
        Ok(d) => d,
        Err(e) => {
            shared.fail(format!("解码器初始化失败: {e}")).await;
            return;
        }
    };

    *shared.dims.lock() = (info.width as usize, info.height as usize);

    let pts = info.sample_pts();
    let dts = info.sample_dts();
    let pts = if pts.is_empty() {
        let fps = info.average_framerate().unwrap_or(25.0);
        (0..info.samples.len()).map(|i| i as f64 / fps).collect()
    } else {
        pts
    };
    let frame_dur = 1.0 / info.average_framerate().unwrap_or(25.0);
    // B 帧重排的安全线：显示时间早于「已解码 dts 水位 - 本值」的帧，
    // 不可能再有更早的帧解出来，可以定稿。无 ctts 时为 0。
    let reorder_secs = info.max_reorder_secs();
    if reorder_secs > 0.0 {
        log::info!(
            "[Playback] 流带 B 帧重排（最大负偏移 {:.3}s），按显示序上屏",
            reorder_secs
        );
    }
    let mut first_sent = false;
    let shown_frames = AtomicU64::new(0);
    // 重排缓冲（显示序升序）与已定稿待上屏队列（显示序升序）
    let mut pending: Vec<VideoFrame> = Vec::new();
    let mut ready: VecDeque<VideoFrame> = VecDeque::new();
    // 解码前瞻：`ready` 里备着这么多秒的画面就停手等时钟。
    //
    // 不能「等到该显示才开始解码」——那样每帧都要先付解码耗时（1080p 数
    // 毫秒）加上取样本的等待，帧永远迟到，画面对音频就整体落后一截。
    // 前瞻让解码跑在显示前面，上屏只等时钟。
    const LOOKAHEAD_SECS: f64 = 0.4;
    // 解码耗时观测（排障：画面跟不上音频时，先看是解码慢还是取样本慢）
    let mut diag_decode = std::time::Duration::ZERO;
    let mut diag_read = std::time::Duration::ZERO;
    let mut diag_iters = 0u64;

    let total = pts.len().min(source.len());
    let mut idx = start_index(&info, start_secs);
    // seek 广播的记账位（独立于音频喂送，见 Shared::seek_broadcast）
    let mut seek_gen = 0u64;
    while idx < total {
        // seek：冲重排与上屏队列、重建解码会话、从目标前的关键帧续播。
        // 时钟已被句柄侧即时换轴，这里的重定位让解码供给跟上新时间轴。
        if let Some(target) = shared.poll_seek(&mut seek_gen) {
            let _ = decoder.finish();
            decoder = match init.open() {
                Ok(d) => d,
                Err(e) => {
                    shared.fail(format!("seek 重建解码器失败: {e}")).await;
                    return;
                }
            };
            pending.clear();
            ready.clear();
            start_secs = target;
            idx = start_index(&info, target);
            log::info!("[Playback] 视频 seek 到 {target:.1}s（关键帧样本 {idx}）");
            continue;
        }
        if shared.shutdown.load(Ordering::Relaxed) {
            return;
        }
        // 节拍闸门：**超前**到前瞻上限就等时钟，落后就全速追。
        //
        // 基准必须是「最新已解帧相对时钟的超前量」。若拿「队首是否到期」当
        // 闸门，等队首（0.4s 前解出来的那帧）到期等于每轮只放一帧、把解码
        // 挤成刚够实时——解码一慢就再也追不回来（实测画面掉到 2/3 速、
        // 音画偏差滚到 -5s）。落后时这里不设闸，解码全速把差距吃掉。
        loop {
            if shared.shutdown.load(Ordering::Relaxed) || shared.seek_pending_since(seek_gen) {
                break; // seek/停机要及时让路到外层
            }
            let ahead_secs = ready
                .back()
                .map(|f| f.pts_secs - shared.now())
                .unwrap_or(f64::NEG_INFINITY);
            if ahead_secs < LOOKAHEAD_SECS {
                break;
            }
            if !emit_due(&mut ready, &shared, &mut first_sent, &shown_frames).await {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(3)).await;
        }
        let t_read = std::time::Instant::now();
        // 读样本带读取代次：seek 会抬代次，堵在旧位置上的 serve 请求尽早
        // 退场并注销读者登记（填充调度器立刻转向新位置）
        let my_epoch = shared.read_epoch.load(Ordering::Relaxed);
        let shared_for_read = Arc::clone(&shared);
        let sample = match source
            .read_cancellable(idx, move || {
                shared_for_read.read_epoch.load(Ordering::Relaxed) != my_epoch
            })
            .await
        {
            Ok(s) => s,
            Err(e) => {
                if shared.seek_pending_since(seek_gen) {
                    continue; // seek 引起的退场：循环顶上重定位
                }
                shared.fail(format!("取视频样本 {idx} 失败: {e}")).await;
                return;
            }
        };
        diag_read += t_read.elapsed();
        let t_dec = std::time::Instant::now();
        let frames = match decoder.decode(&sample, pts[idx], frame_dur) {
            Ok(frames) => frames,
            Err(e) => {
                shared.fail(format!("视频样本 {idx} 解码失败: {e}")).await;
                return;
            }
        };
        diag_decode += t_dec.elapsed();
        diag_iters += 1;
        if diag_iters.is_multiple_of(500) {
            log::debug!(
                "[Playback][video] 解码 {diag_iters} 样本：取样本均 {:.1}ms、解码均 {:.1}ms，超前 {:.2}s",
                diag_read.as_secs_f64() * 1000.0 / diag_iters as f64,
                diag_decode.as_secs_f64() * 1000.0 / diag_iters as f64,
                ready
                    .back()
                    .map(|f| f.pts_secs - shared.now())
                    .unwrap_or(0.0)
            );
        }
        // 起点前的参考帧不入槽（起点对齐由关键帧起步 + 这条过滤共同完成）
        for frame in frames {
            if frame.pts_secs < start_secs - frame_dur {
                continue;
            }
            insert_by_pts(&mut pending, frame);
        }
        // 定稿：水位 = 本样本解码时间 - 最大负偏移（无 stts 时退化为该样本
        // 显示时间，即逐帧直发）
        let watermark = dts.get(idx).copied().unwrap_or(pts[idx]) - reorder_secs;
        commit_ready(&mut pending, watermark, &mut ready);
        if !emit_due(&mut ready, &shared, &mut first_sent, &shown_frames).await {
            return;
        }
        idx += 1;
    }
    match decoder.finish() {
        Ok(tail) => {
            for frame in tail {
                if frame.pts_secs < start_secs - frame_dur {
                    continue;
                }
                insert_by_pts(&mut pending, frame);
            }
        }
        Err(e) => log::warn!("[Playback] 解码收尾失败: {e}"),
    }
    // 流没有更多帧了：剩余全部定稿，按显示序到点发完（时钟还没走完的
    // 尾帧仍按各自的显示时间排队，不抢跑）
    ready.extend(pending.drain(..));
    if !wait_front_due(&mut ready, &shared, &mut first_sent, &shown_frames).await {
        return;
    }

    // 等时钟走到结尾再宣布结束（音频尾音放完）
    let end = *shared.duration_secs.lock();
    while shared.now() < end && !shared.shutdown.load(Ordering::Relaxed) {
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    shared.emit(PlayerEvent::Ended).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 轻量帧载体：真帧只能从解码器出来，重排逻辑本身不需要它。
    #[derive(Debug, Clone, PartialEq)]
    struct Fake(f64);
    impl HasDisplayTime for Fake {
        fn display_secs(&self) -> f64 {
            self.0
        }
    }

    /// B 帧流的核心不变量：**无论解码序如何乱，出口必须是显示序升序**。
    /// 这正是「同样的内容来回重放」的根源——解出一帧就发的话，
    /// 显示时间是 [0, 2d, d]，画面就在 0→2d→d 之间跳。
    #[test]
    fn reorder_emits_display_order_not_decode_order() {
        let d = 1.0 / 30.0;
        // 解码序（dts 递增）：I(dts 0) P(dts 2d) B(dts d)
        // 对应显示时间：I=0, P=2d, B=d —— 非单调，就是 B 帧
        let decode_order = [Fake(0.0), Fake(2.0 * d), Fake(d)];
        let dtss = [0.0, 2.0 * d, d];
        let reorder = d; // 最大负偏移 = 1 帧

        let mut pending: Vec<Fake> = Vec::new();
        let mut ready: VecDeque<Fake> = VecDeque::new();
        let mut emitted: Vec<f64> = Vec::new();

        for (frame, dts) in decode_order.into_iter().zip(dtss) {
            insert_by_pts(&mut pending, frame);
            commit_ready(&mut pending, dts - reorder, &mut ready);
            while let Some(f) = ready.pop_front() {
                emitted.push(f.0);
            }
        }
        emitted.extend(ready.drain(..).map(|f| f.0));
        emitted.extend(pending.drain(..).map(|f| f.0));

        assert!(
            emitted.windows(2).all(|w| w[1] > w[0]),
            "上屏顺序必须是显示序升序，实际 {emitted:?}"
        );
        assert_eq!(emitted.len(), 3, "一帧都不许丢");
    }

    /// 没有 B 帧（无 ctts → 水位即时成立）的流必须逐帧立刻定稿，
    /// 不能因为引入重排就让画面迟一帧。
    #[test]
    fn without_ctts_each_frame_commits_immediately() {
        let mut pending: Vec<Fake> = Vec::new();
        let mut ready: VecDeque<Fake> = VecDeque::new();
        for i in 0..5 {
            let t = i as f64 / 30.0;
            insert_by_pts(&mut pending, Fake(t));
            // reorder_secs = 0：水位就是本样本的 dts，等于显示时间
            let n = commit_ready(&mut pending, t, &mut ready);
            assert_eq!(n, 1, "第 {i} 帧应立刻定稿");
            assert!(pending.is_empty());
        }
        assert_eq!(ready.len(), 5);
    }

    /// 同刻帧保持到达序：乱序插入不能改变同一显示时间的相对顺序。
    #[test]
    fn insert_is_stable_for_equal_timestamps() {
        let mut pending: Vec<Fake> = Vec::new();
        insert_by_pts(&mut pending, Fake(1.0));
        insert_by_pts(&mut pending, Fake(0.0));
        insert_by_pts(&mut pending, Fake(1.0));
        insert_by_pts(&mut pending, Fake(0.5));

        let times: Vec<f64> = pending.iter().map(|f| f.0).collect();
        assert_eq!(times, vec![0.0, 0.5, 1.0, 1.0]);
    }

    /// 水位不到就不许定稿——提前放行等于把还没到的帧排到前面去。
    #[test]
    fn commit_waits_for_the_watermark() {
        let d = 1.0 / 30.0;
        let mut pending: Vec<Fake> = Vec::new();
        let mut ready: VecDeque<Fake> = VecDeque::new();

        insert_by_pts(&mut pending, Fake(2.0 * d));
        insert_by_pts(&mut pending, Fake(0.0));
        // 解码到 dts=d，最大负偏移 1 帧 → 水位 0：只该放行显示时间 0 的那帧
        assert_eq!(commit_ready(&mut pending, d - d, &mut ready), 1);
        assert_eq!(ready.pop_front().map(|f| f.0), Some(0.0));
        assert_eq!(pending.len(), 1, "后一帧还没到定稿线");
    }
}
