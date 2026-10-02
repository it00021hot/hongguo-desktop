//! 原子写。
//!
//! 直接 `write` 会先截断目标文件，若此刻进程被杀 / 掉电，文件就停在「写了一半」的
//! 状态，下次启动解析失败 → 下载记录、设置、剧集档案全部归零。
//! `rename` 在同一文件系统内是原子的，读到的永远是「改动前」或「改动后」的完整文件。

use std::path::Path;

use crate::error::{AppError, AppResult};

/// 先写同目录临时文件，再 rename 覆盖。
pub fn write_atomic(path: &Path, contents: &[u8]) -> AppResult<()> {
    let dir = path
        .parent()
        .ok_or_else(|| AppError::Io(format!("路径没有父目录: {}", path.display())))?;
    std::fs::create_dir_all(dir).map_err(|e| AppError::Io(format!("创建目录失败: {e}")))?;

    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "data.json".to_string());
    let tmp = dir.join(format!(".{file_name}.tmp"));

    if let Err(e) = std::fs::write(&tmp, contents) {
        // 临时文件清理失败不影响主流程
        let _ = std::fs::remove_file(&tmp);
        return Err(AppError::Io(format!("写入临时文件失败: {e}")));
    }

    // Windows 上 rename 到已存在的文件会失败，先删再改名。
    // 这会牺牲「原子替换」语义，因此仅在 Windows 上走这条路。
    #[cfg(windows)]
    if path.exists() {
        let _ = std::fs::remove_file(path);
    }

    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        AppError::Io(format!("原子替换失败: {e}"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("hg-atomic-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn write_then_read_back() {
        let dir = temp_dir("basic");
        let file = dir.join("data.json");
        write_atomic(&file, b"{\"a\":1}").unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), b"{\"a\":1}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn overwrite_keeps_no_tmp_left() {
        let dir = temp_dir("overwrite");
        let file = dir.join("data.json");
        write_atomic(&file, b"{\"a\":1}").unwrap();
        write_atomic(&file, b"{\"a\":2}").unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), b"{\"a\":2}");

        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.contains(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "残留临时文件: {leftovers:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn creates_parent_dirs() {
        let dir = temp_dir("nested");
        let file = dir.join("a").join("b").join("data.json");
        write_atomic(&file, b"{}").unwrap();
        assert!(file.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
