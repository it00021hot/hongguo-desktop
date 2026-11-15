//! 从磁盘补回下载任务记录：记录丢失/被清空、或早期版本没登记时，只要磁盘上有文件
//! 且能对上集号就重新登记为「已完成」。启动自动跑一遍，下载管理页也有手动入口。
//! 认集号两步走：1) 精确寻址——按当前命名模板算文件名（结尾非数字的模板只有这条路）；
//! 2) 尾数扫描兜底——模板改过、档案里没有的孤儿文件按文件名末尾数字认集号。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::app_state::AppState;
use hongguo_common::AppResult;
use hongguo_common::consts::units::MIN_MEDIA_BYTES as MIN_EPISODE_BYTES;
use hongguo_common::model::{DownloadTask, Series, Settings};

/// 尾数扫描认的集号最长位数。四位数封顶，避免把「2023 合集」这类年份当集号。
const MAX_INDEX_DIGITS: usize = 4;

/// 一次扫描的结果。
pub struct RescanSummary {
    /// 补回的任务（已入队并落盘）
    pub added: Vec<DownloadTask>,
    /// 实际扫过文件的剧数，仅用于日志。
    pub scanned_series: usize,
}

/// 从磁盘补回任务记录。只加不改：已有任务记录的集（无论什么状态）一律不动
/// ——把失败中的任务悄悄改成已完成，比少登记一条更让人困惑。
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
            let Ok(meta) = std::fs::metadata(&path) else {
                continue;
            };
            if meta.len() < MIN_EPISODE_BYTES {
                continue;
            }
            let episode = series.episodes.iter().find(|e| e.vid_index == vid_index);
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
        crate::download_service::persist_tasks(&state.store, &state.queue(), "Rescan")?;
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
        if let Some(parent) = Path::new(&task.file_path).parent()
            && parent.is_dir()
        {
            return Some(parent.to_path_buf());
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
    let mut names: Vec<String> = hongguo_store::paths::qa_read_dir(dir)
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
