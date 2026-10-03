//! 从磁盘补回下载任务记录。
//!
//! 用于任务记录丢失/被清空、或早期版本直接下载没登记的情况——
//! 只要磁盘上有文件且能对上集号，就重新登记为「已完成」。启动时自动跑一遍
//! （[`crate::bootstrap::rescan`]），下载管理页也有手动入口（`rescan_downloads`）。
//!
//! 认集号两步走：
//! 1. **精确寻址**：按当前命名模板把档案里每集的文件名算出来，存在即登记。
//!    「剧名 007 标题」这种结尾不是数字的模板只有这条路能对上。
//! 2. **尾数扫描兜底**：模板改过、档案里没有的孤儿文件，按文件名末尾数字认集号
//!    （与合并收集、现版的行为一致）。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::app_state::AppState;
use crate::domain::model::{DownloadTask, Series, Settings};
use crate::error::AppResult;

/// 判定「磁盘上这个文件是一条有效分集」的最小体积。
///
/// 与合并收集（[`crate::service::merge_service`]）的口径一致：小于它的多半是
/// 上次中断留下的残缺文件，登记成已完成只会让播放时打不开。
const MIN_EPISODE_BYTES: u64 = 100 * 1024;

/// 尾数扫描认的集号最长位数。四位数封顶，避免把「2023 合集」这类年份当集号。
const MAX_INDEX_DIGITS: usize = 4;

/// 一次扫描的结果。
pub struct RescanSummary {
    /// 补回的任务（已入队并落盘）
    pub added: Vec<DownloadTask>,
    /// 实际扫过文件的剧数，仅用于日志。
    pub scanned_series: usize,
}

/// 从磁盘补回任务记录。
///
/// 只加不改：已有任务记录的集（无论什么状态）一律不动——把失败中的任务
/// 悄悄改成已完成，比少登记一条更让人困惑。
pub fn rescan_from_disk(state: &AppState) -> AppResult<RescanSummary> {
    let settings = state.settings();
    let series_list: Vec<Series> = state.store.series_all()?;

    let mut summary = RescanSummary {
        added: Vec::new(),
        scanned_series: 0,
    };

    for series in &series_list {
        let Some(dir) = series_dir(state, &settings, series) else {
            continue;
        };
        let found = episode_files_on_disk(&settings, series, &dir);
        if found.is_empty() {
            continue;
        }
        summary.scanned_series += 1;

        for (vid_index, path) in found {
            // 再次确认体积：精确寻址与扫描两个来源都过同一道闸
            let Ok(meta) = std::fs::metadata(&path) else {
                continue;
            };
            if meta.len() < MIN_EPISODE_BYTES {
                continue;
            }
            let episode = series
                .episodes
                .iter()
                .find(|e| e.vid_index == vid_index);
            let mut task = DownloadTask::new(
                &series.series_id,
                &series.title,
                vid_index,
                episode.map(|e| e.vid.as_str()).unwrap_or(""),
                episode.map(|e| e.title.as_str()).unwrap_or(""),
            );
            task.mark_completed(&path.to_string_lossy(), meta.len());

            // enqueue 自带按 (series_id, vid_index) 去重：同一集已有记录时
            // 返回的是老任务，id 对不上就不算新增
            let queued = state.queue().enqueue(task.clone());
            if queued.id != task.id {
                continue;
            }
            log::info!(
                "[Rescan] 补登记 {} 第 {vid_index} 集 ← {}",
                series.title,
                path.display()
            );
            summary.added.push(task);
        }
    }

    if !summary.added.is_empty() {
        crate::store::persist_tasks(state, "Rescan")?;
    }
    Ok(summary)
}

/// 某部剧的下载目录。
///
/// 任务记录里的成品路径优先于按设置推导：下载根目录或命名模板后来改过，
/// 推导出来的目录就不再是文件真正在的地方。都没有就说明这剧没下过，跳过。
fn series_dir(state: &AppState, settings: &Settings, series: &Series) -> Option<PathBuf> {
    for task in state.queue().of_series(&series.series_id) {
        if task.file_path.is_empty() {
            continue;
        }
        if let Some(parent) = Path::new(&task.file_path).parent() {
            if parent.is_dir() {
                return Some(parent.to_path_buf());
            }
        }
    }
    let dir = settings.series_dir(&series.title);
    dir.is_dir().then_some(dir)
}

/// 扫出目录里能对上集号的分集文件，按集号升序。
fn episode_files_on_disk(
    settings: &Settings,
    series: &Series,
    dir: &Path,
) -> BTreeMap<u32, PathBuf> {
    let mut found: BTreeMap<u32, PathBuf> = BTreeMap::new();

    // 1) 精确寻址：按当前模板渲染文件名，存在即登记
    for ep in &series.episodes {
        let stem = settings.render_file_name(&series.title, ep.vid_index, &ep.title);
        let path = dir.join(format!("{stem}.mp4"));
        if file_is_episode(&path) {
            found.insert(ep.vid_index, path);
        }
    }

    // 2) 尾数扫描兜底：排序保证同名同集号时结果确定
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter_map(|e| e.file_name().to_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    for name in names {
        if !name.to_ascii_lowercase().ends_with(".mp4") || name.contains("合集") {
            continue;
        }
        // 后缀恒为 4 个 ASCII 字符（上面已按小写判定过），按长度切最稳，
        // 不必为 ".Mp4" 这类大小写变体逐一匹配
        let stem = &name[..name.len() - 4];
        let Some(idx) = trailing_index(stem) else {
            continue;
        };
        if found.contains_key(&idx) {
            continue;
        }
        let path = dir.join(&name);
        if file_is_episode(&path) {
            found.insert(idx, path);
        }
    }

    found
}

/// 文件存在且够大，才算一条有效分集。
fn file_is_episode(path: &Path) -> bool {
    std::fs::metadata(path)
        .map(|m| m.is_file() && m.len() >= MIN_EPISODE_BYTES)
        .unwrap_or(false)
}

/// 文件名末尾的 1–4 位数字当集号。「剧名 007」→ 7；没有数字或 0 视为认不出。
fn trailing_index(stem: &str) -> Option<u32> {
    let s = stem.trim_end();
    let stripped = s.trim_end_matches(|c: char| c.is_ascii_digit());
    let digits_len = s.len() - stripped.len();
    if digits_len == 0 || digits_len > MAX_INDEX_DIGITS {
        return None;
    }
    let idx: u32 = s[stripped.len()..].parse().ok()?;
    (idx > 0).then_some(idx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::model::{Episode, TaskStatus};

    /// 临时目录 + **数据目录重定向守卫**。
    ///
    /// `rescan_from_disk` 补回任务时会真的落盘 data.json——不重定向的话，
    /// 跑一次测试就把开发机的下载记录覆盖成测试夹具（剧名「我的剧」会
    /// 出现在真实应用里，真踩过一次）。守卫必须在测试体内存活到结尾，
    /// 所以和目录一起返回，调用方想省都省不掉。
    fn sandbox(tag: &str) -> (PathBuf, crate::store::paths::ScopedDataDir) {
        let dir = std::env::temp_dir().join(format!("hg-rescan-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let scope = crate::store::paths::ScopedDataDir::new(&dir);
        (dir, scope)
    }

    fn series_in(state: &AppState, series: Series) {
        state.store.upsert_series(&series).expect("测试库写入");
    }

    fn series_with(title: &str, episodes: Vec<u32>) -> Series {
        Series {
            series_id: "1".into(),
            title: title.into(),
            episodes: episodes
                .into_iter()
                .map(|i| Episode {
                    vid_index: i,
                    vid: format!("v{i}"),
                    title: format!("第{i}集标题"),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        }
    }

    fn settings_with_dir(dir: &Path) -> Settings {
        Settings {
            download_dir: dir.to_string_lossy().to_string(),
            ..Default::default()
        }
    }

    /// series_dir 推导走的是 `settings.series_dir(title)` = `<root>/红果短剧/<title>`。
    fn make_series_dir(root: &Path, title: &str) -> PathBuf {
        let dir = root.join("红果短剧").join(title);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 写一个过体积门槛的分集文件（内容不重要，够大即可）。
    fn write_episode(dir: &Path, name: &str) {
        std::fs::write(dir.join(name), vec![0u8; MIN_EPISODE_BYTES as usize + 1]).unwrap();
    }

    #[test]
    fn trailing_index_parses_padded_numbers() {
        assert_eq!(trailing_index("剧名 007"), Some(7));
        assert_eq!(trailing_index("剧名 42"), Some(42));
        assert_eq!(trailing_index("剧名 1234"), Some(1234));
    }

    #[test]
    fn trailing_index_rejects_bad_inputs() {
        assert_eq!(trailing_index("剧名"), None, "没有数字认不出集号");
        assert_eq!(trailing_index("剧名 000"), None, "第 0 集不存在");
        assert_eq!(trailing_index("剧名 12345"), None, "超过 4 位不当集号");
        assert_eq!(trailing_index("剧名 007 "), Some(7), "结尾空格要容忍");
    }

    #[test]
    fn registers_files_missing_from_task_records() {
        let (root, _scope) = sandbox("basic");
        let state = AppState::default();
        series_in(&state, series_with("我的剧", vec![1, 2, 3]));
        state.replace_settings(settings_with_dir(&root));
        let dir = make_series_dir(&root, "我的剧");

        // 命名模板是 TitleIndex：精确寻址应认出第 1、2 集
        write_episode(&dir, "我的剧 001.mp4");
        write_episode(&dir, "我的剧 002.mp4");
        // 档案里没有第 5 集：尾数扫描兜底
        write_episode(&dir, "我的剧 005.mp4");

        let summary = rescan_from_disk(&state).expect("扫描应成功");
        assert_eq!(summary.added.len(), 3, "三个磁盘文件都应补登记");

        let got: Vec<u32> = {
            let mut v: Vec<u32> = state
                .queue()
                .all()
                .iter()
                .map(|t| t.vid_index)
                .collect();
            v.sort();
            v
        };
        assert_eq!(got, vec![1, 2, 5]);

        // 补回的任务必须是已完成、带路径带体积
        let t5 = state
            .queue()
            .all()
            .into_iter()
            .find(|t| t.vid_index == 5)
            .unwrap();
        assert_eq!(t5.status, TaskStatus::Completed);
        assert!(t5.file_path.ends_with("我的剧 005.mp4"));
        assert_eq!(t5.total, MIN_EPISODE_BYTES + 1);
        assert_eq!(t5.vid, "", "档案里没有的集拿不到 vid，留空待解析");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn leaves_existing_task_records_alone() {
        let (root, _scope) = sandbox("existing");
        let state = AppState::default();
        series_in(&state, series_with("我的剧", vec![1, 2]));
        state.replace_settings(settings_with_dir(&root));
        let dir = make_series_dir(&root, "我的剧");
        write_episode(&dir, "我的剧 001.mp4");

        // 第 1 集已有记录（哪怕失败中）：不能被覆盖成已完成
        let failed = DownloadTask::new("1", "我的剧", 1, "v1", "第1集标题");
        let id = state.queue().enqueue(failed).id;

        let summary = rescan_from_disk(&state).expect("扫描应成功");
        assert!(summary.added.is_empty(), "已有记录的集不该重复登记");

        let t1 = state.queue().get(&id).unwrap();
        assert_eq!(t1.status, TaskStatus::Pending, "原记录状态不应被改动");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn skips_small_and_merge_outputs() {
        let (root, _scope) = sandbox("skip");
        let state = AppState::default();
        series_in(&state, series_with("我的剧", vec![1]));
        state.replace_settings(settings_with_dir(&root));
        let dir = make_series_dir(&root, "我的剧");

        // 小于 100KB 的残缺文件
        std::fs::write(dir.join("我的剧 001.mp4"), b"tiny").unwrap();
        // 合并产物不算分集
        write_episode(&dir, "我的剧 合集.mp4");

        let summary = rescan_from_disk(&state).expect("扫描应成功");
        assert!(
            summary.added.is_empty(),
            "残缺文件与合并产物都不该被登记"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn title_index_episode_template_is_matched_exactly() {
        // 「剧名 007 标题」结尾不是数字，尾数扫描对它是盲的——必须靠精确寻址
        let (root, _scope) = sandbox("title-template");
        let state = AppState::default();
        series_in(&state, series_with("我的剧", vec![7]));
        let mut settings = settings_with_dir(&root);
        settings.naming = crate::domain::model::settings::NamingTemplate::TitleIndexEpisode;
        state.replace_settings(settings);
        let dir = make_series_dir(&root, "我的剧");

        write_episode(&dir, "我的剧 007 第7集标题.mp4");

        let summary = rescan_from_disk(&state).expect("扫描应成功");
        assert_eq!(summary.added.len(), 1, "带标题的模板也要能对上");
        assert_eq!(summary.added[0].vid_index, 7);
        assert_eq!(summary.added[0].vid, "v7", "档案里的 vid 要抄进任务");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn prefers_task_record_dir_over_settings_derivation() {
        // 下载根目录后来改了：设置推导出的目录是空的，但任务记录里的旧路径还在
        let (root, _scope) = sandbox("moved-root");
        let state = AppState::default();
        series_in(&state, series_with("我的剧", vec![1, 2]));
        state.replace_settings(settings_with_dir(&root)); // 新根目录（无文件）

        let old_dir = root.join("旧根").join("我的剧");
        std::fs::create_dir_all(&old_dir).unwrap();
        write_episode(&old_dir, "我的剧 002.mp4");

        let mut t1 = DownloadTask::new("1", "我的剧", 1, "v1", "");
        let gone = old_dir.join("我的剧 001.mp4");
        std::fs::write(&gone, vec![0u8; 8]).unwrap();
        t1.mark_completed(&gone.to_string_lossy(), 8);
        state.queue().enqueue(t1);

        let summary = rescan_from_disk(&state).expect("扫描应成功");
        assert_eq!(summary.added.len(), 1, "应从旧目录补回第 2 集");
        assert_eq!(summary.added[0].vid_index, 2);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn series_without_dir_is_skipped() {
        let (root, _scope) = sandbox("no-dir");
        let state = AppState::default();
        series_in(&state, series_with("没下过的剧", vec![1]));
        state.replace_settings(settings_with_dir(&root));

        let summary = rescan_from_disk(&state).expect("扫描应成功");
        assert_eq!(summary.scanned_series, 0);
        assert!(summary.added.is_empty());

        let _ = std::fs::remove_dir_all(&root);
    }
}
