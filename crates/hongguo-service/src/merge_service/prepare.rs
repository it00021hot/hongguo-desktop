//! 合并前校验：把「能不能合并」一次问清楚——哪些集能参与合并、输出卷放不放得下、
//! 各集编码是否一致。结果里带的是 i18n key，由前端查资源表翻成用户文案。

use std::path::Path;

use super::done_inputs;
use crate::app_state::AppState;
use hongguo_common::AppResult;
use hongguo_common::model::MergePreflight;
use hongguo_media::{codec_probe, disk_space};

/// 收集某剧已下载的集（按集号升序），并做合并前校验。
pub fn preflight(state: &AppState, series_id: &str) -> AppResult<MergePreflight> {
    let done = done_inputs(state, series_id);

    if done.is_empty() {
        return Ok(MergePreflight {
            ok: false,
            episode_count: 0,
            estimated_size: 0,
            free_space: None,
            codec_consistent: true,
            codec_mismatch_episode: None,
            warnings: vec!["merge.noDownloads".to_string()],
        });
    }

    let estimated_size: u64 = done
        .iter()
        .map(|(_, p)| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0))
        .sum();

    let download_dir = state.settings().download_dir;
    let free_space = disk_space::free_space_at(Path::new(&download_dir));
    if free_space.is_none() {
        // 查不到不等于没空间：只记日志，不误报成空间不足
        log::warn!("查不到下载目录 {download_dir} 的剩余空间，跳过空间校验");
    }

    let codec = codec_probe::check(&done);

    Ok(MergePreflight {
        ok: true,
        episode_count: done.len(),
        estimated_size,
        free_space,
        codec_consistent: codec.consistent,
        codec_mismatch_episode: codec.mismatch_episode,
        warnings: warnings_for(estimated_size, free_space, codec.mismatch_episode),
    })
}

/// 按固定顺序拼出提示 key：先「编码不一致」（这次合并能不能做），再「空间不足」（做完放不放得下）。
fn warnings_for(
    estimated_size: u64,
    free_space: Option<u64>,
    codec_mismatch_episode: Option<u32>,
) -> Vec<String> {
    let mut warnings = Vec::new();
    if codec_mismatch_episode.is_some() {
        warnings.push("merge.codecMismatch".to_string());
    }
    match free_space {
        // 留 10% 余量：产物是边写边算的，估算与实际总有几 MB 的差，卡在边界上失败更糟。
        Some(free) if estimated_size + estimated_size / 10 > free => {
            warnings.push("merge.insufficientSpace".to_string());
        }
        _ => {}
    }
    warnings
}
