//! 播放卡顿的宿主侧取证：上屏节奏与事件循环积压。
//!
//! 引擎日志只能证明「帧已经发出去了」；用户看到的卡顿发生在后面两段——
//! 事件通道的合并丢弃、NV12 转换/主线程投递、以及 render 到合成的排期。
//! 这两段此前只有 debug 日志（默认 info 级看不见），偶发一次就沉底。
//!
//! 采样口径：`PaintDiag` 只在**有新内容上屏**的 render 上打点（同一个
//! 帧重复 render 不算），因此「两次打点的墙钟间隔」就是画面内容的停顿；
//! `seen - 上次上屏的 seen` 里超出的部分 = 渲染合并跳过的帧。
//!
//! 输出策略：每 5s 一行窗口摘要（info；出现停顿/积压升级 warn），单次
//! 停顿另有即时 warn——默认 `RUST_LOG=info` 就能捞到，不需要开 debug。

use std::time::{Duration, Instant};

/// 窗口长度：与引擎/音频诊断同拍，按日志时间戳可直接对齐。
const WINDOW: Duration = Duration::from_secs(5);
/// 单次上屏间隔超过它 = 用户可感知的画面停顿（2x 下约 7 帧）。
const STALL_WARN: Duration = Duration::from_millis(120);
/// 即时告警防抖：持续卡顿时不要每帧刷屏。
const WARN_DEBOUNCE: Duration = Duration::from_secs(2);
/// 事件循环单批投递耗时超过它 = 主线程/转换被占住，升级 warn。
const LAG_WARN: Duration = Duration::from_millis(150);

/// 一次 render 采样时的播放器状态（参数收拢，避免函数参数超限）。
#[derive(Clone, Copy)]
pub(super) struct PaintState {
    pub paused: bool,
    pub active: bool,
    pub buffering: bool,
    pub switching: bool,
}

/// 上屏节奏窗口统计（由 [`crate::home::HomeView`] 持有）。
pub(super) struct PaintDiag {
    /// 累计到达视图的 Frame 事件数（单调，不随窗口清零）。
    seen: u64,
    /// 上次上屏时看到的 `seen`。
    painted_seq: u64,
    window_started: Instant,
    window_painted: u32,
    /// 窗口内因 render 合并而没画出的帧。
    window_skipped: u32,
    max_gap: Duration,
    max_gap_pts: f64,
    /// 上屏内容与主时钟（音频可闻位置）的最大偏差：真正可见的音画不同步。
    max_skew: Duration,
    /// 偏差超过 100ms（媒体秒）的帧数。
    skew_over_100ms: u32,
    over_50ms: u32,
    over_100ms: u32,
    /// 上一次有新内容上屏的时刻；暂停时清空，恢复后的首帧不误报停顿。
    last_paint: Option<Instant>,
    last_warn: Option<Instant>,
    was_paused: bool,
    /// 窗口是否前台。非前台时 GPUI 会按平台策略降频/暂停绘制，这段间隔
    /// 不是「用户看着卡了一下」——重置窗口，不参与统计与告警。
    was_active: bool,
}

impl PaintDiag {
    pub(super) fn new() -> Self {
        Self {
            seen: 0,
            painted_seq: 0,
            window_started: Instant::now(),
            window_painted: 0,
            window_skipped: 0,
            max_gap: Duration::ZERO,
            max_gap_pts: 0.0,
            max_skew: Duration::ZERO,
            skew_over_100ms: 0,
            over_50ms: 0,
            over_100ms: 0,
            last_paint: None,
            last_warn: None,
            was_paused: false,
            was_active: false,
        }
    }

    /// 帧事件到达视图（每次 Frame 调一次，render 之前）。
    pub(super) fn note_frame(&mut self) {
        self.seen += 1;
    }

    /// 换集/换引擎：上一集的节奏不能算进新一集（起播预滚本来就要等）。
    pub(super) fn reset(&mut self) {
        // 暂停/非前台/换集期间到达的帧不参与下一次停顿统计：把「已上屏」
        // 游标追平到当前 seen，否则恢复后的第一帧会把积压的几百帧算成
        // 「渲染合并跳帧」。
        self.painted_seq = self.seen;
        self.window_started = Instant::now();
        self.window_painted = 0;
        self.window_skipped = 0;
        self.max_gap = Duration::ZERO;
        self.max_gap_pts = 0.0;
        self.max_skew = Duration::ZERO;
        self.skew_over_100ms = 0;
        self.over_50ms = 0;
        self.over_100ms = 0;
        self.last_paint = None;
    }

    /// 每次 render 调用一次。只有 `seen` 前进了才说明这一帧是新内容。
    pub(super) fn observe_render(
        &mut self,
        pts: Option<f64>,
        clock_secs: Option<f64>,
        rate: f64,
        state: PaintState,
    ) {
        let PaintState {
            paused,
            active,
            buffering,
            switching,
        } = state;
        if !active {
            // 非前台：平台可能降频/暂停绘制，间隔没有诊断意义
            self.was_active = false;
            self.reset();
            return;
        }
        if !self.was_active {
            self.was_active = true;
            self.reset();
        }
        if paused {
            // 暂停期间没有新内容，恢复后的首帧与暂停前不构成「停顿」
            if !self.was_paused {
                self.reset();
            }
            self.was_paused = true;
            return;
        }
        if self.was_paused {
            self.was_paused = false;
            // 暂停时长不算进上屏窗口：恢复后从新窗口重新计
            self.reset();
        }
        if self.seen != self.painted_seq {
            let now = Instant::now();
            let new_frames = self.seen - self.painted_seq;
            self.window_painted += 1;
            self.window_skipped += (new_frames - 1) as u32;
            // 当前上屏内容与主时钟（音频可闻位置）的偏差：这才是用户能看到/
            // 听到的不同步，不只是引擎发出时刻的偏差。
            if !switching && let (Some(clock), Some(pts)) = (clock_secs, pts) {
                let skew = (clock - pts).abs();
                if skew > 0.0 {
                    self.max_skew = self.max_skew.max(Duration::from_secs_f64(skew));
                }
                if skew > 0.1 {
                    self.skew_over_100ms += 1;
                }
            }
            if let Some(previous) = self.last_paint {
                let gap = now.saturating_duration_since(previous);
                if gap >= Duration::from_millis(50) {
                    self.over_50ms += 1;
                }
                if gap >= Duration::from_millis(100) {
                    self.over_100ms += 1;
                }
                if gap > self.max_gap {
                    self.max_gap = gap;
                    self.max_gap_pts = pts.unwrap_or(0.0);
                }
                if gap >= STALL_WARN
                    && self
                        .last_warn
                        .is_none_or(|at| now.duration_since(at) >= WARN_DEBOUNCE)
                {
                    self.last_warn = Some(now);
                    log::warn!(
                        "[Home][paint] 画面停顿 {}ms（上屏帧 {}，媒体 {:.1}s，{:.2}x，\
                         buffering={} switching={}）",
                        gap.as_millis(),
                        self.seen,
                        pts.unwrap_or(0.0),
                        rate,
                        buffering,
                        switching,
                    );
                }
            }
            self.last_paint = Some(now);
            self.painted_seq = self.seen;
        }
        self.report_if_due(rate, buffering, switching);
    }

    /// 到点输出窗口摘要。上屏帧率与「合并跳帧」是判断宿主是否跟得上的
    /// 直接指标：2x 下内容帧率应约 60fps，明显低于它或跳帧数抬头就是卡。
    fn report_if_due(&mut self, rate: f64, buffering: bool, switching: bool) {
        let elapsed = self.window_started.elapsed();
        if elapsed < WINDOW {
            return;
        }
        let fps = self.window_painted as f64 / elapsed.as_secs_f64();
        let level = if self.max_gap >= STALL_WARN
            || self.over_100ms > 0
            || self.max_skew >= Duration::from_millis(200)
        {
            log::Level::Warn
        } else {
            log::Level::Info
        };
        log::log!(
            level,
            "[Home][paint] {:.1}s 窗口：上屏 {} 帧（{:.1}fps，{:.2}x），\
             渲染合并跳 {} 帧，最大间隔 {}ms@{:.1}s（>50ms {} 次，>100ms {} 次），\
             音画偏差最大 {}ms（>100ms {} 帧），buffering={} switching={}",
            elapsed.as_secs_f64(),
            self.window_painted,
            fps,
            rate,
            self.window_skipped,
            self.max_gap.as_millis(),
            self.max_gap_pts,
            self.over_50ms,
            self.over_100ms,
            self.max_skew.as_millis(),
            self.skew_over_100ms,
            buffering,
            switching,
        );
        self.window_started = Instant::now();
        self.window_painted = 0;
        self.window_skipped = 0;
        self.max_gap = Duration::ZERO;
        self.max_gap_pts = 0.0;
        self.max_skew = Duration::ZERO;
        self.skew_over_100ms = 0;
        self.over_50ms = 0;
        self.over_100ms = 0;
    }
}

/// 像素转换统计。只有 Windows 走 NV12→RGBA（macOS 是 CVPixelBuffer 零拷贝，
/// 转换段在事件循环里整个不存在），所以随平台一起消失——mac 的诊断行不印
/// 恒为 0 的「转换耗时」。
#[cfg(target_os = "windows")]
#[derive(Default)]
struct ConvertDiag {
    count: u32,
    sum: Duration,
    max: Duration,
}

/// 事件循环窗口统计：合并丢弃、单批最大条数、投递耗时（Windows 含像素转换）。
pub(super) struct EventDiag {
    window_started: Instant,
    events: u32,
    frames: u32,
    skipped: u32,
    max_batch: usize,
    batches: u32,
    sum_lag: Duration,
    max_lag: Duration,
    #[cfg(target_os = "windows")]
    convert: ConvertDiag,
    last_warn: Option<Instant>,
}

impl EventDiag {
    pub(super) fn new() -> Self {
        Self {
            window_started: Instant::now(),
            events: 0,
            frames: 0,
            skipped: 0,
            max_batch: 0,
            batches: 0,
            sum_lag: Duration::ZERO,
            max_lag: Duration::ZERO,
            #[cfg(target_os = "windows")]
            convert: ConvertDiag::default(),
            last_warn: None,
        }
    }

    pub(super) fn note_batch(&mut self, batch_len: usize, frames: u32, skipped: u32) {
        self.events += batch_len as u32;
        self.frames += frames;
        self.skipped += skipped;
        self.max_batch = self.max_batch.max(batch_len);
        self.batches += 1;
    }

    /// 像素转换耗时（仅 Windows；NV12→RGBA，见 home/mod.rs 事件循环）。
    #[cfg(target_os = "windows")]
    pub(super) fn note_convert(&mut self, elapsed: Duration) {
        self.convert.count += 1;
        self.convert.sum += elapsed;
        self.convert.max = self.convert.max.max(elapsed);
    }

    /// 一批事件从出通道到全部投递进视图（含 Windows 的 NV12→RGBA 转换）。
    pub(super) fn note_lag(&mut self, elapsed: Duration) {
        self.sum_lag += elapsed;
        self.max_lag = self.max_lag.max(elapsed);
        if elapsed >= LAG_WARN
            && self
                .last_warn
                .is_none_or(|at| Instant::now().duration_since(at) >= WARN_DEBOUNCE)
        {
            self.last_warn = Some(Instant::now());
            log::warn!(
                "[Home][events] 事件投递耗时 {}ms（单批 {} 事件；主线程/像素转换被占住）",
                elapsed.as_millis(),
                self.max_batch,
            );
        }
    }

    pub(super) fn report(&mut self) {
        let elapsed = self.window_started.elapsed();
        if elapsed < WINDOW {
            return;
        }
        let level = if self.max_lag >= LAG_WARN || self.skipped > 3 {
            log::Level::Warn
        } else {
            log::Level::Info
        };
        let avg_lag = self.sum_lag.as_secs_f64() * 1000.0 / f64::from(self.batches.max(1));
        #[cfg(target_os = "windows")]
        let convert_seg = format!(
            "，转换均 {:.1}ms/最大 {}ms",
            self.convert.sum.as_secs_f64() * 1000.0 / f64::from(self.convert.count.max(1)),
            self.convert.max.as_millis(),
        );
        #[cfg(not(target_os = "windows"))]
        let convert_seg = String::new();
        log::log!(
            level,
            "[Home][events] {:.1}s 窗口：收 {} 事件/{} 帧，合并丢弃 {} 帧，\
             单批最大 {}，投递均 {:.1}ms/最大 {}ms{convert_seg}",
            elapsed.as_secs_f64(),
            self.events,
            self.frames,
            self.skipped,
            self.max_batch,
            avg_lag,
            self.max_lag.as_millis(),
        );
        self.window_started = Instant::now();
        self.events = 0;
        self.frames = 0;
        self.skipped = 0;
        self.max_batch = 0;
        self.batches = 0;
        self.sum_lag = Duration::ZERO;
        self.max_lag = Duration::ZERO;
        #[cfg(target_os = "windows")]
        {
            self.convert = ConvertDiag::default();
        }
    }
}
