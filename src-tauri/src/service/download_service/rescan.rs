//! 扫描目录补登记。
//!
//! 任务记录丢失（换机、data.json 损坏）时，磁盘上的成品文件还在。
//! 扫描剧集目录把已有文件补回任务列表，让下载列表自愈。

use std::path::Path;

use crate::domain::model::{DownloadTask, Series};
use crate::service::download_service::queue::DownloadQueue;

/// 从文件名里解析集号。
///
/// 命名模板保证形如 `剧名 007 标题.mp4` 或 `剧名 007.mp4`，
/// 取第一段连续数字作为集号。解析不出来返回 `None`。
pub fn parse_index(file_name: &str) -> Option<u32> {
    let stem = Path::new(file_name).file_stem()?.to_string_lossy();
    // 跳过剧名部分：找「空格 + 数字」的模式，避免把剧名里的数字当集号
    let mut best: Option<u32> = None;
    for part in stem.split_whitespace() {
        if part.chars().all(|c| c.is_ascii_digit()) {
            if let Ok(n) = part.parse::<u32>() {
                best = Some(n);
                break;
            }
        }
    }
    best
}

/// 扫描剧集目录，返回可补登记的文件。
pub fn scan_series_dir(dir: &Path) -> Vec<(u32, std::path::PathBuf, u64)> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("mp4") {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        // 合并产物与残留临时文件不算已下载的单集
        if name.contains("合集") || name.ends_with(".enc.tmp") {
            continue;
        }
        let Some(index) = parse_index(name) else {
            continue;
        };
        let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
        out.push((index, path, size));
    }
    out.sort_by_key(|(i, _, _)| *i);
    out
}

/// 把扫描结果补登记进队列。
pub fn adopt_all(queue: &DownloadQueue, series: &Series, dir: &Path) -> usize {
    let mut n = 0;
    for (index, path, size) in scan_series_dir(dir) {
        let mut task = DownloadTask::new(&series.series_id, &series.title, index, "", "");
        task.file_path = path.to_string_lossy().to_string();
        queue.adopt_file(task, size);
        n += 1;
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_index_reads_padded_number() {
        assert_eq!(parse_index("剧名 007.mp4"), Some(7));
        assert_eq!(parse_index("剧名 007 第三集.mp4"), Some(7));
        assert_eq!(parse_index("剧名 12.mp4"), Some(12));
    }

    #[test]
    fn parse_index_ignores_titles_with_digits() {
        // 剧名里的数字不应被当成集号
        assert_eq!(parse_index("2024年的夏天 003.mp4"), Some(3));
    }

    #[test]
    fn parse_index_returns_none_without_number() {
        assert_eq!(parse_index("只有剧名.mp4"), None);
        assert_eq!(parse_index("合集.mp4"), None);
    }

    #[test]
    fn scan_finds_and_sorts_files() {
        let dir = std::env::temp_dir().join(format!(
            "hg-rescan-{}",
            std::thread::current()
                .name()
                .unwrap_or("t")
                .replace("::", "-")
        ));
        std::fs::create_dir_all(&dir).unwrap();
        for name in ["剧 003.mp4", "剧 001.mp4", "剧 002 标题.mp4"] {
            std::fs::write(dir.join(name), b"x").unwrap();
        }
        // 这两个应被排除
        std::fs::write(dir.join("剧 合集.mp4"), b"x").unwrap();
        std::fs::write(dir.join("剧 004.enc.tmp"), b"x").unwrap();
        std::fs::write(dir.join("剧 005.txt"), b"x").unwrap();

        let found = scan_series_dir(&dir);
        assert_eq!(
            found.iter().map(|(i, _, _)| *i).collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn scan_missing_dir_is_empty() {
        assert!(scan_series_dir(Path::new("/definitely/not/here")).is_empty());
    }

    #[test]
    fn adopt_all_backfills_queue() {
        let q = DownloadQueue::new();
        let dir = std::env::temp_dir().join(format!(
            "hg-adopt-{}",
            std::thread::current()
                .name()
                .unwrap_or("t")
                .replace("::", "-")
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("剧名 001.mp4"), b"12345").unwrap();

        let series = Series {
            series_id: "123".into(),
            title: "剧名".into(),
            ..Default::default()
        };
        assert_eq!(adopt_all(&q, &series, &dir), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
