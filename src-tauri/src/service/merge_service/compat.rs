//! 兼容合并：转 H.264/AAC。
//!
//! 与兼容模式共用 `media::transcode` 的同一条流水线，区别只是
//! 「一集」变「多集」——流复制拼接成全集再转，或逐集转再流复制。
//! 这里选择后者：内存占用恒定，且能复用单集转码的缓存。

use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};

use super::done_inputs;
use super::guard::RunningMerge;
use super::progress::ProgressSink;
use crate::app_state::AppState;
use crate::domain::model::MergeTask;
use crate::error::{AppError, AppResult};
use crate::media::transcode::TranscodeOptions;
use crate::service::transcode_service::{cache, pipeline};

/// 兼容合并：把每集转成 H.264 后流复制拼接。
///
/// 逐集转码，每完成一集报一次进度——这是唯一能看出「卡在哪一集」的地方。
///
/// `slot` 是运行中任务的占用凭证：用户在界面上删掉这条「合并中」时，
/// 取消标志会被置位，这里在**每集之间**检查并提前退出。检查点放在集与集
/// 之间而不是转码内部：一集软转要一分多钟，插到解码循环里反而拖慢它，
/// 而「最多再浪费一集的时间」是删除操作能接受的代价。
///
/// 返回 `(输出路径, 输出大小, 参与集数)`。
pub fn compat_merge(
    state: &AppState,
    series_id: &str,
    output_name: &str,
    task: &MergeTask,
    on_progress: &ProgressSink,
    slot: &RunningMerge,
) -> AppResult<(PathBuf, u64, usize)> {
    let inputs = done_inputs(state, series_id);

    if inputs.is_empty() {
        return Err(AppError::Media("没有已下载的分集".into()));
    }

    let settings = state.settings();
    let dir = settings.series_dir(output_name);
    std::fs::create_dir_all(&dir).map_err(|e| AppError::Io(e.to_string()))?;
    let output = dir.join(format!("{output_name} 合集.mp4"));
    let total = inputs.len();

    // 逐集转码到缓存，再把缓存产物流复制拼接。
    //
    // **统一到第 1 集的分辨率**：短剧各集由平台**分别**编码，同一部剧里混着
    // 1080p 与 720p 是常态。逐集转码但不缩放的话，产物依然规格不一，拼接那一步
    // 照样过不去——实测一部 11 集的剧（10 集 1080p、1 集 720p）走到拼接才失败，
    // 前面十几分钟的转码全白做。
    let scale_to = pipeline::resolution_of(&inputs[0].1);
    let mut odd: Vec<u32> = Vec::new();
    if let Some((w, h)) = scale_to {
        odd = inputs
            .iter()
            .filter(|(_, p)| pipeline::resolution_of(p).is_some_and(|(rw, rh)| rw != w || rh != h))
            .map(|(i, _)| *i + 1)
            .collect();
        if !odd.is_empty() {
            log::info!(
                "[Merge] 第 {} 集分辨率与首集不同，将统一缩放到 {w}x{h}",
                odd.iter()
                    .map(|i| i.to_string())
                    .collect::<Vec<_>>()
                    .join("、")
            );
        }
    }

    // 缩放缺口快速失败：没有缩放能力的后端（纯 Rust 软解）遇上混合分辨率，
    // 各集各转各的分辨率，最后死在拼接的宽高一致校验上——实测一部 11 集的
    // 剧（10 集 1080p、1 集 720p）在软解上白转十几分钟才失败。把失败提到
    // 转码之前，并告诉用户哪条路能解决。
    if !odd.is_empty() && !crate::media::capability::scaling_available() {
        return Err(AppError::Media(format!(
            "第 {} 集分辨率与首集不一致，当前转码后端（纯 Rust 软解）不支持缩放，无法合并；\
             安装 ffmpeg 或在支持硬件编码的机器上重试",
            odd.iter()
                .map(|i| i.to_string())
                .collect::<Vec<_>>()
                .join("、")
        )));
    }

    // **并行度按后端定**，两条路的瓶颈完全不同。
    let threads = merge_threads(inputs.len());
    log::info!("[Merge] {output_name}：{total} 集，并行度 {threads}");

    // 每格存结果或错误。用 `Mutex<Vec<…>>` 而不是逐个切分切片：
    // 切片得靠 `split_at_mut` 一路拆引用，十几行下来比一把锁还难读；
    // 而这里的写入频率是「每集一次」，对比真正的热路径（每帧）可以忽略。
    let slots: Mutex<Vec<Option<Result<PathBuf, AppError>>>> =
        Mutex::new((0..total).map(|_| None).collect());

    // 已完成集数与集内比例的全局账本，供进度上报做**小数累计**。
    //
    // 这里原先只报整数集数，单集合并的进度条从头卡到尾；且多线程并发时
    // 各自报「done + 本集比例」会互相覆盖、进度来回跳，所以收进一个
    // 原子账本统一折算（见 [`LiveProgress`]）。
    let live = LiveProgress::new();

    // 硬编会话超订的自适应收敛（背景见 [`merge_threads`]）：
    // 生效并行度从 threads 起步，一观察到「预期硬编却走了软路」就收到 2。
    // 收敛不影响已完成的集——软解产物同样是能播的 H.264，只是慢，不值得重做。
    let live_cap = AtomicUsize::new(threads);
    let active = AtomicUsize::new(0);
    let hw_expected = crate::media::capability::selected_backend().is_hardware();

    std::thread::scope(|scope| {
        // 每个线程循环领下一集：谁先空出来谁接下一集。
        // 不用「起 threads 个线程各跑固定那几集」——集数少于核数时会漏，
        // 集数多于核数时又只能并发 threads 集。
        for _ in 0..threads {
            scope.spawn(|| {
                loop {
                    if slot.is_cancelled() {
                        return;
                    }
                    // 拿活跃名额：live_cap 收敛后，多出来的线程在这里等而不是抢活。
                    // CAS 保证名额不超发；取消检查让等待线程能及时退出。
                    loop {
                        if slot.is_cancelled() {
                            return;
                        }
                        let a = active.load(Ordering::Relaxed);
                        if a < live_cap.load(Ordering::Relaxed)
                            && active
                                .compare_exchange(a, a + 1, Ordering::Relaxed, Ordering::Relaxed)
                                .is_ok()
                        {
                            break;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(100));
                    }
                    // 认领第一格还没被占的。认领与占位在**同一次持锁**里完成，
                    // 否则两个线程会同时看到同一格是空的，转同一集两遍。
                    // 锁中毒只说明有线程 panic 过：持锁内只有整格读写，其余
                    // 格位仍然可信，恢复使用（空格在收集阶段自会报错）。
                    let next = {
                        let mut g = slots.lock().unwrap_or_else(|p| p.into_inner());
                        match g.iter().position(|s| s.is_none()) {
                            Some(i) => {
                                g[i] = Some(Ok(PathBuf::new())); // 占位
                                Some(i)
                            }
                            None => None,
                        }
                    };
                    let Some(next) = next else {
                        active.fetch_sub(1, Ordering::Relaxed);
                        return;
                    };

                    let (vid_index, source) = &inputs[next];
                    // 本集的集内记账（毫秒拆分的最近比例），完成时结算回账本
                    let last_ms = AtomicI64::new(0);
                    let result = match cache::cached_path(series_id, *vid_index) {
                        Some(p) => Ok(p),
                        None => {
                            let on_eps = episode_progress(
                                &live,
                                &last_ms,
                                pipeline::episode_seconds(source),
                                total,
                                on_progress,
                                task,
                            );
                            let cb: &(dyn Fn(f64) + Send + Sync) = &on_eps;
                            let r = pipeline::transcode(
                                series_id,
                                *vid_index,
                                source,
                                &TranscodeOptions::default(),
                                scale_to,
                                Some(cb),
                            );
                            // 会话超订的信号：预期硬编、这集却落在软路上。
                            // 收敛并行度，让后续集不再超订。个别集因偶发错误回落
                            // 也会触发（多收敛一次，代价只是后面保守些），可接受。
                            if hw_expected
                                && r.as_ref().is_ok_and(|t| !t.backend.is_hardware())
                                && live_cap.fetch_min(2, Ordering::Relaxed) > 2
                            {
                                log::warn!(
                                    "[Merge] 硬编会话疑似超订（本集回落软路），并行度收敛到 2"
                                );
                            }
                            r.map(|t| PathBuf::from(t.output_path))
                        }
                    };
                    slots.lock().unwrap_or_else(|p| p.into_inner())[next] = Some(result);
                    active.fetch_sub(1, Ordering::Relaxed);
                    let finished = live.finish(last_ms.load(Ordering::Relaxed));
                    on_progress(finished as f64, total, task);
                }
            });
        }
    });

    // 收集：按集号顺序取回，任一集失败就整体失败。
    // 取消要在拼接**之前**拦下来，否则会拼出一份不完整的「全集」。
    let slots = slots.into_inner().unwrap_or_else(|p| p.into_inner());
    let mut transcoded: Vec<PathBuf> = Vec::with_capacity(total);
    for slot in slots {
        match slot {
            Some(Ok(p)) => transcoded.push(p),
            Some(Err(AppError::Cancelled)) => {
                log::info!("[Merge] {output_name} 已被取消，放弃拼接");
                return Err(AppError::Cancelled);
            }
            Some(Err(e)) => return Err(e),
            // 线程 panic 时该格留空：报出来，而不是当成成功拼出半截文件
            None => return Err(AppError::Media("转码线程异常退出".into())),
        }
    }
    on_progress(total as f64, total, task);

    // 拼接前再确认一次：最后几集转完到真正开写之间还有窗口
    if slot.is_cancelled() {
        log::info!("[Merge] {output_name} 已被取消，放弃拼接");
        return Err(AppError::Cancelled);
    }

    let (size, count) = crate::media::remux::concat_copy(&transcoded, &output)?;

    // 产物必须自己解得开才叫成功。流复制拼接对输入的一致性要求极高，
    // 漏检一个条件就会得到一份「有 moov、能打开、但只播得动前几秒」的文件——
    // 那种坏法用户看不出来，只会觉得「合并功能有问题」。
    let verified = crate::media::demux::demux_file(&output).map_err(|e| {
        let _ = std::fs::remove_file(&output);
        AppError::Media(format!("合并产物校验失败，已删除该文件: {e}"))
    })?;
    if !verified
        .video_track()
        .is_some_and(|t| !t.info.samples.is_empty())
    {
        let _ = std::fs::remove_file(&output);
        return Err(AppError::Media(
            "合并产物里没有可用的视频样本，已删除该文件".into(),
        ));
    }
    let frames: usize = verified
        .tracks
        .iter()
        .filter(|t| t.info.is_video)
        .map(|t| t.info.samples.len())
        .sum();
    log::info!(
        "[Merge] {output_name} 完成：{count} 集 / {frames} 个视频样本 / {:.1}MB",
        size as f64 / 1048576.0
    );
    Ok((output, size, count))
}

/// 并行度按后端定，几条路的瓶颈完全不同。
///
/// - 平台层（VideoToolbox 等）：有硬编时编码在 GPU 上，CPU 只剩解封装/拷贝，
///   会话数同样有限（Apple 平台硬编会话有上限），铺 4 路保守起步，
///   超了由 [`live_cap`] 收敛兜住；macOS 无硬编时是 Apple 软编会话——
///   编码器自己就是多线程 CPU 大户，压到 2 路保交互。
/// - ffmpeg 硬编（nvenc 等）：同是 GPU 会话受限，铺 6 路吞吐最好
///   （受核数约束）。**老 NVIDIA 驱动限制并发会话数（3~8 路不等）**，超限的
///   那路开不了编码器、静默回落软解——所以线程数只是上限，真正生效的是
///   [`live_cap`]：一观察到「预期硬编却走了软路」就收敛，后续集不再超订。
/// - ffmpeg 软编（libx264）：编码器自己多线程，瓶颈变成 CPU 总量。实测
///   libx264 单集 7.1s，4 路并发时单集劣化到约 25s，但吞吐从 9.3s/集提到
///   约 6.3s/集。**并发度高会拉长单集耗时**，进度条停得更久，所以不铺满。
///   ⚠️ 不要给并行实例加 `-threads N` 限线程——真机实测（i5-13400、真实
///   剧集 4 路并行）默认线程 38.5s，限 `-threads 4` 反而 48.5s（慢 26%）：
///   x264 默认线程数已经调得很好，限线程只会饿着每个编码器。
/// - 纯 Rust 软解：瓶颈是**单线程**的 HEVC 解码（`rusty_h265` 没有并行原语），
///   它吃的是内存带宽。实测 i5-13400 / 16 逻辑核、1080p 单集 54s：
///   串行 216s、4 路 177s、16 路 169s——加线程只是抢带宽，所以压在 4。
fn merge_threads(episodes: usize) -> usize {
    let cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    let cap = match crate::media::capability::selected_backend() {
        crate::media::Backend::Platform => {
            if crate::media::platform::h264_hw_encoder_available() {
                cores.clamp(1, 4)
            } else {
                // Apple 软编会话：编码吃满 CPU，2 路封顶
                cores.clamp(1, 2)
            }
        }
        crate::media::Backend::FfmpegHw => cores.clamp(1, 6),
        crate::media::Backend::FfmpegSw => (cores / 2).clamp(1, 4),
        crate::media::Backend::Rust => 4,
    };
    cap.min(episodes.max(1))
}

/// 兼容合并的全局进度账本：「已完成整集数 + 在转各集的比例和」。
///
/// 集内回调只看得到自己那一集，多线程并发时若各自报「done + 本集比例」，
/// 后完成的前一集会把整体比例往回拽，进度条来回跳。所以各集只把**自己的**
/// 比例增量汇入同一本原子账（毫秒拆分，1000 = 一集），读取时统一折算，
/// 任何交错顺序下整体值都单调不减。
struct LiveProgress {
    /// 已完成的整集数
    done: AtomicUsize,
    /// 在转各集的比例和（毫单位；一集转完即从这里结转进 `done`）
    inflight_milli: AtomicI64,
}

impl LiveProgress {
    fn new() -> Self {
        Self {
            done: AtomicUsize::new(0),
            inflight_milli: AtomicI64::new(0),
        }
    }

    /// 一条集内上报：把该集最新比例汇入账本，返回整体「已完成集数」小数口径。
    /// `last` 是这一集的记账格（记它上次汇入的毫单位数，增量才算得出来）。
    fn update(&self, last: &AtomicI64, ratio: f64) -> f64 {
        let milli = (ratio.clamp(0.0, 1.0) * 1000.0).round() as i64;
        self.inflight_milli
            .fetch_add(milli - last.load(Ordering::Relaxed), Ordering::Relaxed);
        last.store(milli, Ordering::Relaxed);
        self.snapshot()
    }

    /// 一集完成：把它在途的份额结转为整集，返回新的完成集数。
    /// `last_milli` 传这一集记账格的终值；缓存命中的集没进过账，传 0。
    fn finish(&self, last_milli: i64) -> usize {
        if last_milli > 0 {
            self.inflight_milli.fetch_sub(last_milli, Ordering::Relaxed);
        }
        self.done.fetch_add(1, Ordering::Relaxed) + 1
    }

    fn snapshot(&self) -> f64 {
        self.done.load(Ordering::Relaxed) as f64
            + self.inflight_milli.load(Ordering::Relaxed) as f64 / 1000.0
    }
}

/// 集内进度：管线回调给「已编码秒数」，除以本集总时长折成比例，
/// 再经 [`LiveProgress`] 汇成整体小数集数上报。时长读不出来的集
/// 退化为按集粒度推进（完成一集跳一格），不再假装有集内进度。
fn episode_progress<'a>(
    live: &'a LiveProgress,
    last: &'a AtomicI64,
    episode_seconds: Option<f64>,
    total: usize,
    on_progress: &'a ProgressSink,
    task: &'a MergeTask,
) -> impl Fn(f64) + Send + Sync + 'a {
    move |secs: f64| {
        let Some(dur) = episode_seconds.filter(|d| *d > 0.0) else {
            return;
        };
        let done = live.update(last, secs / dur);
        on_progress(done, total, task);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::model::DownloadTask;

    #[test]
    fn single_episode_progress_climbs_fractionally() {
        // 原实现把进度量化成整集，单集合并的进度条从头卡到尾——
        // 集内比例必须能推进小数口径，完成时精确归一
        let live = LiveProgress::new();
        let last = AtomicI64::new(0);
        assert_eq!(live.update(&last, 0.3), 0.3);
        assert_eq!(live.update(&last, 0.9), 0.9);
        assert_eq!(live.finish(last.load(Ordering::Relaxed)), 1);
        assert_eq!(live.snapshot(), 1.0);
    }

    #[test]
    fn parallel_episodes_keep_progress_monotonic() {
        let live = LiveProgress::new();
        let a = AtomicI64::new(0);
        let b = AtomicI64::new(0);
        let mut prev = 0.0f64;
        for step in [0.2, 0.5, 0.8] {
            for last in [&a, &b] {
                let snap = live.update(last, step);
                assert!(
                    snap >= prev,
                    "并发上报的交错顺序不能让整体进度回退: {prev} -> {snap}"
                );
                prev = snap;
            }
        }
        let done = live.finish(a.load(Ordering::Relaxed));
        assert_eq!(done, 1, "A 集完成应记 1 集");
        assert!(live.snapshot() >= prev, "结转不能让进度回退");
        let _ = live.finish(b.load(Ordering::Relaxed));
        assert_eq!(live.snapshot(), 2.0, "全部完成后应精确等于总集数");
    }

    #[test]
    fn ratio_update_ignores_out_of_range_values() {
        // 越界的比例按夹紧口径记账：每集记「当前比例」的绝对值，
        // 上一笔报多了，下一笔自然把账减回去——账本始终等于各集当前比例之和
        let live = LiveProgress::new();
        let last = AtomicI64::new(0);
        assert_eq!(live.update(&last, f64::NAN), 0.0);
        assert_eq!(live.update(&last, 42.0), 1.0);
        assert_eq!(live.update(&last, -1.0), 0.0, "回落到夹紧值 0，账随实报");
    }

    #[test]
    fn threads_never_exceed_the_episode_count() {
        for n in [1usize, 2, 3, 10, 100] {
            assert!(merge_threads(n) <= n, "{n} 集时并行度不应超过集数");
            assert!(merge_threads(n) >= 1, "并行度至少为 1");
        }
    }

    #[test]
    fn a_single_episode_merge_runs_serially() {
        let d = std::env::temp_dir().join(format!("hg-compat-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let mut t = DownloadTask::new("s", "剧", 1, "v", "");
        let p = d.join("1.mp4");
        std::fs::write(&p, b"x").unwrap();
        t.mark_completed(p.to_string_lossy().as_ref(), 1);
        let _ = std::fs::remove_dir_all(&d);
        assert_eq!(merge_threads(1), 1);
    }
}
