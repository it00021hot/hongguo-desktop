//! 兼容合并：转 H.264/AAC。
//!
//! 与兼容模式共用 `media::transcode` 的同一条流水线，区别只是
//! 「一集」变「多集」——流复制拼接成全集再转，或逐集转再流复制。
//! 这里选择后者：内存占用恒定，且能复用单集转码的缓存。

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

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

    // **并行度按后端定**，两条路的瓶颈完全不同。
    let threads = merge_threads(inputs.len());
    log::info!("[Merge] {output_name}：{total} 集，并行度 {threads}");

    // 每格存结果或错误。用 `Mutex<Vec<…>>` 而不是逐个切分切片：
    // 切片得靠 `split_at_mut` 一路拆引用，十几行下来比一把锁还难读；
    // 而这里的写入频率是「每集一次」，对比真正的热路径（每帧）可以忽略。
    let slots: Mutex<Vec<Option<Result<PathBuf, AppError>>>> =
        Mutex::new((0..total).map(|_| None).collect());

    // 已完成集数，供进度上报做**累计**。
    //
    // 这里原先报的是 `on_progress(1, total, task)` —— done 恒为 1，于是无论转完
    // 几集前端收到的都是 1/total，只有收尾那次 `total/total` 才是真进度，
    // 表现就是进度条卡在开头不动。多个工作线程并发时还要 `fetch_add` 才不会
    // 互相覆盖。
    let done = AtomicUsize::new(0);
    // 正在转码的集数，用来把「集内」进度折成整体百分比
    let running = AtomicUsize::new(0);

    // 硬编会话超订的自适应收敛（背景见 [`merge_threads`]）：
    // 生效并行度从 threads 起步，一观察到「有硬编却走了软解」就收到 2。
    // 收敛不影响已完成的集——软解产物同样是能播的 H.264，只是慢，不值得重做。
    let live_cap = AtomicUsize::new(threads);
    let active = AtomicUsize::new(0);
    let hw_expected = crate::media::ffmpeg::h264_encoder().is_some_and(|e| e.hardware);

    std::thread::scope(|scope| {
        // 每个线程循环领下一集：谁先空出来谁接下一集。
        // 不用「起 threads 个线程各跑固定那几集」——集数少于核数时会漏，
        // 集数多于核数时又只能并发 threads 集。
        for _ in 0..threads {
            scope.spawn(|| loop {
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
                let result = match cache::cached_path(series_id, *vid_index) {
                    Some(p) => Ok(p),
                    None => {
                        running.fetch_add(1, Ordering::Relaxed);
                        let on_eps = episode_progress(
                            on_progress,
                            done.load(Ordering::Relaxed),
                            running.load(Ordering::Relaxed),
                            total,
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
                        // 会话超订的信号：明明探测到硬编、这集却走了软解。
                        // 收敛并行度，让后续集不再超订。个别集因偶发错误回落
                        // 也会触发（多收敛一次，代价只是后面保守些），可接受。
                        if hw_expected && r.as_ref().is_ok_and(|t| t.decoder.contains("rusty"))
                            && live_cap.fetch_min(2, Ordering::Relaxed) > 2
                        {
                            log::warn!(
                                "[Merge] 硬编会话疑似超订（本集回落软解），并行度收敛到 2"
                            );
                        }
                        running.fetch_sub(1, Ordering::Relaxed);
                        r.map(|t| PathBuf::from(t.output_path))
                    }
                };
                slots.lock().unwrap_or_else(|p| p.into_inner())[next] = Some(result);
                active.fetch_sub(1, Ordering::Relaxed);
                let finished = done.fetch_add(1, Ordering::Relaxed) + 1;
                on_progress(finished, total, task);
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
    on_progress(total, total, task);

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

/// 并行度按后端定：两条路的瓶颈完全不同。
///
/// - 纯 Rust 软解：瓶颈是**单线程**的 HEVC 解码（`rusty_h265` 没有并行原语），
///   它吃的是内存带宽。实测 i5-13400 / 16 逻辑核、1080p 单集 54s：
///   串行 216s、4 路 177s、16 路 169s——加线程只是抢带宽，所以压在 4。
/// - ffmpeg 硬编（nvenc 等）：编码在 GPU 上，CPU 只剩解码，铺 6 路吞吐最好
///   （受核数约束）。**老 NVIDIA 驱动限制并发会话数（3~8 路不等）**，超限的
///   那路开不了编码器、静默回落软解——所以线程数只是上限，真正生效的是
///   [`live_cap`]：一观察到「有硬编却走了软解」就收敛，后续集不再超订。
/// - ffmpeg 软编（libx264）：编码器自己多线程，瓶颈变成 CPU 总量。实测
///   libx264 单集 7.1s，4 路并发时单集劣化到约 25s，但吞吐从 9.3s/集提到
///   约 6.3s/集。**并发度高会拉长单集耗时**，进度条停得更久，所以不铺满。
///   ⚠️ 不要给并行实例加 `-threads N` 限线程——真机实测（i5-13400、真实
///   剧集 4 路并行）默认线程 38.5s，限 `-threads 4` 反而 48.5s（慢 26%）：
///   x264 默认线程数已经调得很好，限线程只会饿着每个编码器。
fn merge_threads(episodes: usize) -> usize {
    let cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    let cap = match crate::media::ffmpeg::h264_encoder() {
        Some(e) if e.hardware => cores.clamp(1, 6),
        Some(_) => (cores / 2).clamp(1, 4),
        None => 4,
    };
    cap.min(episodes.max(1))
}

/// 集内进度：把 ffmpeg 报的「已编码秒数」折成整部合并的完成比例。
///
/// 多集并发时每集各报一次，整体比例 = (已完成集数 + 本集比例) / 总集数。
fn episode_progress(
    on_progress: &ProgressSink,
    finished: usize,
    running: usize,
    total: usize,
    task: &MergeTask,
) -> impl Fn(f64) + Send + Sync + 'static {
    let task = task.clone();
    let sink = on_progress.clone();
    move |ratio: f64| {
        if !(0.0..=1.0).contains(&ratio) {
            return;
        }
        let _ = running;
        let done = finished as f64 + ratio;
        on_fraction(&sink, done, total as f64, &task);
    }
}

/// 把「已完成 n.x 集」折成 `ProgressSink` 的整数口径，并夹在 `0..total-1`。
///
/// 夹上界而不是报满：`total/total` 在合并里表示「转码全部完成、进入拼接」，
/// 集内进度抢先报满会让进度条在还在转码时就显示 100%。
fn on_fraction(sink: &ProgressSink, done: f64, total: f64, task: &MergeTask) {
    if total <= 0.0 {
        return;
    }
    let done = done.clamp(0.0, total);
    let n = done.round() as usize;
    sink(
        n.min(total.round() as usize - 1),
        total.round() as usize,
        task,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::model::{DownloadTask, MergeMode, MergeTask};
    use parking_lot::Mutex;
    use std::sync::Arc;

    fn task() -> MergeTask {
        MergeTask::new("s", "剧", "out", MergeMode::Compat)
    }

    #[test]
    fn fraction_is_clamped_just_below_complete() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink: ProgressSink = {
            let seen = seen.clone();
            Arc::new(move |d, t, _| seen.lock().push((d, t)))
        };
        let task = task();
        on_fraction(&sink, 3.5, 10.0, &task);
        on_fraction(&sink, 9.9, 10.0, &task);
        on_fraction(&sink, 12.0, 10.0, &task);
        let got = seen.lock().clone();
        assert_eq!(got.len(), 3);
        assert!(
            got.iter().all(|(d, t)| *d < *t),
            "集内进度不能报满：满进度表示「开始拼接」了，实际: {got:?}"
        );
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
