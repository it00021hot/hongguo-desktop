//! 损坏数据文件的备份恢复。
//!
//! 解析失败时把原文件改名成 `data.json.corrupt-<时间戳>`，用户还能手工捞回
//! 下载记录——直接丢弃等于让用户白下载一遍。

use std::path::Path;

/// 把损坏的数据文件备份起来，返回备份路径。
pub fn backup_corrupt(path: &Path) -> Option<std::path::PathBuf> {
    let stamp = chrono::Utc::now().timestamp_millis();
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "data.json".to_string());
    let backup = path.with_file_name(format!("{name}.corrupt-{stamp}"));

    match std::fs::rename(path, &backup) {
        Ok(()) => {
            log::error!("[Store] 损坏文件已备份到: {}", backup.display());
            Some(backup)
        }
        Err(e) => {
            // 备份失败也不能阻断启动，用空数据继续
            log::error!("[Store] 备份损坏文件失败: {e}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backup_renames_original() {
        let dir = std::env::temp_dir().join(format!(
            "hg-recover-{}",
            std::thread::current()
                .name()
                .unwrap_or("t")
                .replace("::", "-")
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("data.json");
        std::fs::write(&f, b"broken").unwrap();

        let backup = backup_corrupt(&f).expect("应成功备份");
        assert!(backup.exists());
        assert!(!f.exists(), "原文件应被移走");
        assert!(std::fs::read(&backup).unwrap() == b"broken");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn backup_on_missing_file_is_none() {
        let missing = std::env::temp_dir().join("definitely-not-here-xyz.json");
        assert!(backup_corrupt(&missing).is_none());
    }
}
