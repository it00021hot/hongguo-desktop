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
    // **并行度按实测定，不按理论定**（i5-13400 / 16 逻辑核，1080p 单集 54s）：
    //
    // | 并行度 | 4 集耗时 | 相对串行 216s |
    // |--------|----------|----------------|
    // | 1（串行） | 216s | 基准 |
    // | 4       | 177s | 1.22× |
    // | 16（满核） | 169s | 1.28× |
    //
    // 收益远小于预期，因为瓶颈是**单线程的 HEVC 解码**（`rusty_h265` 无任何
    // 并行原语），而它吃的是内存带宽：4 路同时解就把带宽打满了，再加线程
    // 也没用。`RUSTY_THREADS=1` 限制编码器内部线程后只快 5%，进一步印证
    // 编码不是瓶颈。
    //
    // 这里仍取一个小并发（而不是 1）：串行时任一时刻只有一路在解，
    // 解码与编码的访存可以重叠一点；4 路以上纯属抢带宽。
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .min(inputs.len())
        .clamp(1, 4);

    log::info!("[Merge] {output_name}：{total} 集，并行度 {threads}",);

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

    std::thread::scope(|scope| {
        // 每个线程循环领下一集：谁先空出来谁接下一集。
        // 不用「起 threads 个线程各跑固定那几集」——集数少于核数时会漏，
        // 集数多于核数时又只能并发 threads 集。
        for _ in 0..threads {
            scope.spawn(|| loop {
                if slot.is_cancelled() {
                    return;
                }
                // 认领第一格还没被占的。认领与占位在**同一次持锁**里完成，
                // 否则两个线程会同时看到同一格是空的，转同一集两遍。
                let next = {
                    let mut g = slots.lock().expect("结果锁中毒");
                    match g.iter().position(|s| s.is_none()) {
                        Some(i) => {
                            g[i] = Some(Ok(PathBuf::new())); // 占位
                            Some(i)
                        }
                        None => None,
                    }
                };
                let Some(next) = next else { return };

                let (vid_index, source) = &inputs[next];
                let result = match cache::cached_path(series_id, *vid_index) {
                    Some(p) => Ok(p),
                    None => pipeline::transcode(
                        series_id,
                        *vid_index,
                        source,
                        &TranscodeOptions::default(),
                    )
                    .map(|r| PathBuf::from(r.output_path)),
                };
                slots.lock().expect("结果锁中毒")[next] = Some(result);
                let finished = done.fetch_add(1, Ordering::Relaxed) + 1;
                on_progress(finished, total, task);
            });
        }
    });

    // 收集：按集号顺序取回，任一集失败就整体失败。
    // 取消要在拼接**之前**拦下来，否则会拼出一份不完整的「全集」。
    let slots = slots.into_inner().expect("结果锁中毒");
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
    Ok((output, size, count))
}
