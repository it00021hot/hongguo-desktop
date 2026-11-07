//! 兼容合并：转 H.264/AAC。与兼容播放共用 `transcode_service::pipeline` 同一条流水线，
//! 区别只是「一集」变「多集」——这里选逐集转再流复制：内存占用恒定，且能复用单集转码的缓存。

use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};

use super::done_inputs;
use super::guard::RunningMerge;
use super::progress::ProgressSink;
use crate::app_state::AppState;
use crate::transcode_service::{cache, pipeline};
use hongguo_common::model::MergeTask;
use hongguo_common::{AppError, AppResult};

/// 兼容合并：把每集转成 H.264 后流复制拼接，逐集转码、每完成一集报一次进度；返回 `(输出路径, 输出大小, 参与集数)`。
/// `slot` 是运行中任务的占用凭证——取消标志在**每集之间**检查（转码内部不插检查点，最多再浪费一集的时间）。
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
    let output = super::output_file(&settings, output_name);
    hongguo_store::paths::qa_create_dir_all(output.parent().expect("产物必有父目录"))
        .map_err(|e| AppError::Io(e.to_string()))?;
    let total = inputs.len();

    // 逐集转码到缓存再流复制拼接；统一到第 1 集的分辨率——各集由平台分别编码，
    // 同一部剧混 1080p 与 720p 是常态，不缩放的话拼接那一步照样过不去。
    let scale_to = pipeline::resolution_of(&inputs[0].1);
    if let Some((w, h)) = scale_to {
        let odd: Vec<u32> = inputs
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

    // 没有可用后端就别开工：每一集转码都会立刻失败，早失败省下用户的时间。
    let Some(backend) = hongguo_media_platform::capability::selected_backend() else {
        return Err(AppError::Media(
            "当前机器没有可用的转码后端：系统编码器不可用，且未安装 ffmpeg。\
             安装 ffmpeg 后重试。"
                .into(),
        ));
    };

    // **并行度按后端定**，两条路的瓶颈完全不同。
    let threads = merge_threads(backend, inputs.len());
    log::info!("[Merge] {output_name}：{total} 集，并行度 {threads}");

    // 每格存结果或错误；一把锁按格认领，写入频率只有「每集一次」。
    let slots: Mutex<Vec<Option<Result<PathBuf, AppError>>>> =
        Mutex::new((0..total).map(|_| None).collect());

    // 已完成集数与集内比例的全局账本，进度上报做小数累计
    //（多线程并发各自报会互相覆盖、进度来回跳，见 [`LiveProgress`]）。
    let live = LiveProgress::new();

    // 硬编会话超订的自适应收敛（见 [`merge_threads`]）：生效并行度从 threads 起步，
    // 一观察到「预期硬编却走了软路」就收到 2；不影响已完成的集。
    let live_cap = AtomicUsize::new(threads);
    let active = AtomicUsize::new(0);
    let hw_expected = backend.is_hardware();

    std::thread::scope(|scope| {
        // 每个线程循环领下一集：谁先空出来谁接下一集（集数与核数不必对齐）。
        for _ in 0..threads {
            scope.spawn(|| {
                loop {
                    if slot.is_cancelled() {
                        return;
                    }
                    // 拿活跃名额：CAS 保证名额不超发；取消检查让等待线程能及时退出。
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
                    // 认领与占位在同一次持锁里完成，否则两个线程会同时看到同一格是空的、转同一集两遍；
                    // 锁中毒只说明有线程 panic 过，其余格位仍可信，恢复使用（空格在收集阶段自会报错）。
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
                                scale_to,
                                hongguo_media_platform::RateProfile::Archive,
                                Some(cb),
                                pipeline::TranscodeHooks::default(),
                            );
                            // 会话超订的信号：预期硬编却落在软路——收敛并行度，
                            // 后续集不再超订（偶发回落多收敛一次，代价只是保守些）。
                            if hw_expected
                                && r.as_ref()
                                    .is_ok_and(|t| !t.backend.is_some_and(|b| b.is_hardware()))
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

    let (size, count) = hongguo_media::remux::concat_copy(&transcoded, &output)?;

    // 产物必须自己解得开才叫成功：流复制拼接漏检一个条件就会得到
    // 「能打开、但只播得动前几秒」的文件，用户只会觉得「合并功能有问题」。
    let verified = hongguo_media::demux::demux_file(&output).map_err(|e| {
        let _ = hongguo_store::paths::qa_remove_file(&output);
        AppError::Media(format!("合并产物校验失败，已删除该文件: {e}"))
    })?;
    if !verified
        .video_track()
        .is_some_and(|t| !t.info.samples.is_empty())
    {
        let _ = hongguo_store::paths::qa_remove_file(&output);
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
    // 拼接已消费全部转码产物：兼容缓存是播放侧的 3 集滚动窗口，合并的瞬时多集
    // 文件就地删除，不等下一轮 trim（校验失败的分支不删——重试还能命中缓存）。
    let mut cleaned = 0usize;
    for p in &transcoded {
        if hongguo_store::paths::qa_remove_file(p).is_ok() {
            cleaned += 1;
        }
    }
    log::info!(
        "[Merge] {output_name} 完成：{count} 集 / {frames} 个视频样本 / {:.1}MB，清理 {cleaned} 个转码产物",
        size as f64 / 1048576.0
    );
    Ok((output, size, count))
}

/// 并行度按后端定：GPU 会话受限的硬编铺 4/6 路，CPU 大户的软编压到 2/4 路；线程数只是上限，
/// 硬编会话超订由 `live_cap` 收敛兜住。不要给并行 ffmpeg 实例加 `-threads N` 限线程——
/// x264 默认线程数已经调得很好，限线程只会饿着每个编码器、反而更慢。
fn merge_threads(backend: hongguo_media::Backend, episodes: usize) -> usize {
    let cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    let cap = match backend {
        hongguo_media::Backend::Platform => {
            if hongguo_media_platform::platform::h264_hw_encoder_available() {
                cores.clamp(1, 4)
            } else {
                // Apple 软编会话：编码吃满 CPU，2 路封顶
                cores.clamp(1, 2)
            }
        }
        hongguo_media::Backend::FfmpegHw => cores.clamp(1, 6),
        hongguo_media::Backend::FfmpegSw => (cores / 2).clamp(1, 4),
    };
    cap.min(episodes.max(1))
}

/// 兼容合并的全局进度账本：「已完成整集数 + 在转各集的比例和」。
/// 各集只把自己的比例增量（毫单位，1000 = 一集）汇入同一本原子账，读取时统一折算，
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

/// 集内进度：管线回调给「已编码秒数」，除以本集总时长折成比例，经 [`LiveProgress`] 汇成整体小数集数上报。
/// 时长读不出来的集退化为按集粒度推进（完成一集跳一格），不再假装有集内进度。
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
    use hongguo_common::model::DownloadTask;

    #[test]
    fn single_episode_progress_climbs_fractionally() {
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
        let live = LiveProgress::new();
        let last = AtomicI64::new(0);
        assert_eq!(live.update(&last, f64::NAN), 0.0);
        assert_eq!(live.update(&last, 42.0), 1.0);
        assert_eq!(live.update(&last, -1.0), 0.0, "回落到夹紧值 0，账随实报");
    }

    #[test]
    fn threads_never_exceed_the_episode_count() {
        for n in [1usize, 2, 3, 10, 100] {
            assert!(
                merge_threads(hongguo_media::Backend::FfmpegHw, n) <= n,
                "{n} 集时并行度不应超过集数"
            );
            assert!(
                merge_threads(hongguo_media::Backend::FfmpegHw, n) >= 1,
                "并行度至少为 1"
            );
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
        assert_eq!(merge_threads(hongguo_media::Backend::FfmpegHw, 1), 1);
    }
}
