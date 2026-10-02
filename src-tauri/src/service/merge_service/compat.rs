//! 兼容合并：转 H.264/AAC。
//!
//! 与兼容模式共用 `media::transcode` 的同一条流水线，区别只是
//! 「一集」变「多集」——流复制拼接成全集再转，或逐集转再流复制。
//! 这里选择后者：内存占用恒定，且能复用单集转码的缓存。

use std::path::PathBuf;

use super::done_inputs;
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
/// 返回 `(输出路径, 输出大小, 参与集数)`。
pub fn compat_merge(
    state: &AppState,
    series_id: &str,
    output_name: &str,
    task: &MergeTask,
    on_progress: &ProgressSink,
) -> AppResult<(PathBuf, u64, usize)> {
    let inputs = done_inputs(state, series_id);

    if inputs.is_empty() {
        return Err(AppError::Media("没有已下载的分集".into()));
    }

    let settings = state.settings();
    let dir = settings.series_dir(output_name);
    std::fs::create_dir_all(&dir).map_err(|e| AppError::Io(e.to_string()))?;
    let output = dir.join(format!("{output_name} 合集.mp4"));

    // 逐集转码到缓存，再把缓存产物流复制拼接
    let total = inputs.len();
    let mut transcoded: Vec<PathBuf> = Vec::with_capacity(total);
    for (i, (vid_index, source)) in inputs.iter().enumerate() {
        let hit = cache::cached_path(series_id, *vid_index);
        match hit {
            Some(p) => transcoded.push(p),
            None => {
                let result = pipeline::transcode(
                    series_id,
                    *vid_index,
                    source,
                    &TranscodeOptions::default(),
                )?;
                transcoded.push(PathBuf::from(result.output_path));
            }
        }
        on_progress(i + 1, total, task);
    }

    let (size, count) = crate::media::remux::concat_copy(&transcoded, &output)?;
    Ok((output, size, count))
}
