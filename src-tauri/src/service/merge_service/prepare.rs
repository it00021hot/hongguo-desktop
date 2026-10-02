//! 合并前校验：把「能不能合并」一次问清楚。
//!
//! 三件事：哪些集能参与合并、输出卷还放不放得下、各集的编码是否一致。
//! 结果里带的是 i18n key，由前端查资源表翻成用户看得懂的文案。

use std::path::Path;

use super::done_inputs;
use crate::app_state::AppState;
use crate::domain::model::MergePreflight;
use crate::error::AppResult;
use crate::media::{codec_probe, disk_space};

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

/// 按固定顺序拼出提示 key。
///
/// 顺序固定：先说「为什么不能快速合并」，再说「还有没有位置放」——
/// 编码不一致是这次合并能不能做的问题，空间只是做完之后放不放得下。
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
        Some(free) if estimated_size > free => {
            warnings.push("merge.insufficientSpace".to_string());
        }
        // 余量够、或压根查不到余量，都没有要提示的事
        _ => {}
    }
    warnings
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::model::{DownloadTask, Settings};
    use crate::domain::mp4::fixtures::{mp4, TrackSpec};
    use std::path::PathBuf;
    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("hg-preflight-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 一份下载目录指向 `dir` 的状态。
    fn state_with_dir(dir: &Path) -> AppState {
        let state = AppState::default();
        state.replace_settings(Settings {
            download_dir: dir.to_string_lossy().to_string(),
            ..Settings::default()
        });
        state
    }

    /// 造一集已下载的分集：落盘 + 入队。
    fn add_episode(
        state: &AppState,
        dir: &Path,
        series_id: &str,
        vid_index: u32,
        spec: &TrackSpec,
    ) {
        let path = dir.join(format!("{vid_index}.mp4"));
        std::fs::write(&path, mp4(std::slice::from_ref(spec))).unwrap();
        let mut task = DownloadTask::new(series_id, "剧", vid_index, "v", "");
        task.mark_completed(&path.to_string_lossy(), 1);
        state.queue().enqueue(task);
    }

    #[test]
    fn nothing_downloaded_short_circuits() {
        let state = AppState::default();
        let p = preflight(&state, "1").unwrap();

        assert!(!p.ok);
        assert_eq!(p.episode_count, 0);
        assert_eq!(p.estimated_size, 0);
        assert_eq!(p.warnings, vec!["merge.noDownloads".to_string()]);
        assert_eq!(p.codec_mismatch_episode, None);
    }

    #[test]
    fn matching_episodes_pass_the_codec_gate() {
        let dir = temp_dir("match");
        let state = state_with_dir(&dir);
        add_episode(
            &state,
            &dir,
            "1",
            1,
            &TrackSpec::video(1, b"hvc1", 1920, 1080),
        );
        add_episode(
            &state,
            &dir,
            "1",
            2,
            &TrackSpec::video(1, b"hvc1", 1920, 1080),
        );

        let p = preflight(&state, "1").unwrap();

        assert!(p.ok);
        assert_eq!(p.episode_count, 2);
        assert!(p.estimated_size > 0, "预计大小应等于各集文件大小之和");
        assert!(p.codec_consistent, "两集编码相同不该报不一致");
        assert_eq!(p.codec_mismatch_episode, None);
        assert!(
            !p.warnings.contains(&"merge.codecMismatch".to_string()),
            "不该带编码不一致的提示: {:?}",
            p.warnings
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn mismatching_episodes_are_reported_with_their_episode_number() {
        let dir = temp_dir("mismatch");
        let state = state_with_dir(&dir);
        add_episode(
            &state,
            &dir,
            "1",
            1,
            &TrackSpec::video(1, b"hvc1", 1920, 1080),
        );
        add_episode(
            &state,
            &dir,
            "1",
            2,
            &TrackSpec::video(1, b"hvc1", 1280, 720),
        );

        let p = preflight(&state, "1").unwrap();

        assert!(!p.codec_consistent);
        assert_eq!(p.codec_mismatch_episode, Some(2));
        assert!(
            p.warnings.contains(&"merge.codecMismatch".to_string()),
            "应提示编码不一致: {:?}",
            p.warnings
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_small_merge_never_reports_insufficient_space() {
        // 余量查得到且远大于预计大小时，不该出现空间不足的提示
        let dir = temp_dir("fits");
        let state = state_with_dir(&dir);
        add_episode(
            &state,
            &dir,
            "1",
            1,
            &TrackSpec::video(1, b"hvc1", 1920, 1080),
        );

        let p = preflight(&state, "1").unwrap();

        assert!(!p.warnings.contains(&"merge.insufficientSpace".to_string()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn warnings_are_ordered_codec_first() {
        assert_eq!(
            warnings_for(200, Some(100), Some(2)),
            vec![
                "merge.codecMismatch".to_string(),
                "merge.insufficientSpace".to_string()
            ]
        );
    }

    #[test]
    fn warnings_stay_empty_when_everything_checks_out() {
        assert!(warnings_for(200, Some(1000), None).is_empty());
        assert!(
            warnings_for(200, None, None).is_empty(),
            "查不到余量时不产提示"
        );
    }

    #[test]
    fn equal_size_and_free_space_fits() {
        // 刚好放下不算不足，否则「差一个字节就报」纯属噪声
        assert!(warnings_for(100, Some(100), None).is_empty());
        assert_eq!(
            warnings_for(101, Some(100), None),
            vec!["merge.insufficientSpace".to_string()]
        );
    }
}
