//! 快速合并：流复制拼接。

use std::path::PathBuf;

use tauri::State;

use super::done_inputs;
use super::progress::ProgressSink;
use crate::app_state::AppState;
use crate::domain::model::MergeTask;
use crate::error::{AppError, AppResult};

/// 快速合并某剧已下载的分集。
///
/// 流复制没有「逐集」的天然边界——`concat_copy` 一次拼完，所以这里只在
/// 开始前与结束后各报一次进度。
pub fn quick_merge(
    state: &State<'_, AppState>,
    series_id: &str,
    output_name: &str,
    task: &MergeTask,
    on_progress: &ProgressSink,
) -> AppResult<(PathBuf, u64, usize)> {
    let inputs = done_inputs(state, series_id);

    // 闸门放在执行点上，不放在 UI 上：绕过界面直接提交的命令请求同样要拦住，
    // 否则产出的就是一份索引都过不去的文件。
    ensure_codec_consistent(&inputs)?;

    let settings = state.settings();
    let dir = settings.series_dir(output_name);
    let output = dir.join(format!("{output_name} 合集.mp4"));

    let total = inputs.len();
    on_progress(0, total, task);

    let sorted: Vec<PathBuf> = inputs.into_iter().map(|(_, p)| p).collect();
    let (size, count) = crate::media::remux::concat_copy(&sorted, &output)?;
    on_progress(total, total, task);
    Ok((output, size, count))
}

/// 编码不一致就拒绝，不产出文件。
///
/// [`crate::media::remux::concat_copy`] 是整文件字节级顺序拼接：各集的容器
/// 结构或编码参数对不上时，产物连索引都过不去。
fn ensure_codec_consistent(inputs: &[(u32, PathBuf)]) -> AppResult<()> {
    if let Some(episode) = crate::media::codec_probe::check(inputs).mismatch_episode {
        return Err(AppError::Media(format!(
            "第 {episode} 集的编码与第 1 集不一致，快速合并会产出损坏文件"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::mp4::fixtures::{mp4, TrackSpec};

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("hg-quick-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn episode(dir: &std::path::Path, vid_index: u32, spec: &TrackSpec) -> (u32, PathBuf) {
        let path = dir.join(format!("{vid_index}.mp4"));
        std::fs::write(&path, mp4(std::slice::from_ref(spec))).unwrap();
        (vid_index, path)
    }

    #[test]
    fn mismatching_encodings_are_refused() {
        // UI 把快速合并置灰只是提示；真正的保证必须落在执行点上
        let dir = temp_dir("mismatch");
        let inputs = vec![
            episode(&dir, 1, &TrackSpec::video(1, b"hvc1", 1920, 1080)),
            episode(&dir, 2, &TrackSpec::video(1, b"hvc1", 1280, 720)),
        ];

        let err = ensure_codec_consistent(&inputs).unwrap_err();
        let text = err.to_string();
        assert!(text.contains('2'), "错误要指到出问题的集号: {text}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_single_unparsable_episode_is_refused() {
        let dir = temp_dir("garbage");
        let bad = dir.join("2.mp4");
        std::fs::write(&bad, [0xffu8; 512]).unwrap();
        let inputs = vec![
            episode(&dir, 1, &TrackSpec::video(1, b"hvc1", 1920, 1080)),
            (2, bad),
        ];

        assert!(ensure_codec_consistent(&inputs).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn matching_encodings_pass_the_gate() {
        let dir = temp_dir("match");
        let inputs = vec![
            episode(&dir, 1, &TrackSpec::video(1, b"hvc1", 1920, 1080)),
            episode(&dir, 2, &TrackSpec::video(1, b"hvc1", 1920, 1080)),
        ];

        assert!(ensure_codec_consistent(&inputs).is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
