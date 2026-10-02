//! 数据目录与缓存目录解析。
//!
//! 沿用 Electron 版的路径（`%APPDATA%/hongguo-downloader`），便于用户平滑迁移
//! 下载记录与设置，不因为换了框架就丢数据。

use std::path::PathBuf;

/// 应用数据目录。
pub fn data_dir() -> PathBuf {
    if let Ok(explicit) = std::env::var("HONGGUO_DATA_DIR") {
        return PathBuf::from(explicit);
    }
    dirs::data_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("hongguo-downloader")
}

/// 数据文件名。
pub const DATA_FILE: &str = "data.json";

/// 兼容模式转码缓存目录。
pub fn compat_cache_dir() -> PathBuf {
    data_dir().join("compat-cache")
}

/// 兼容转码缓存上限（沿用现版 4GB）。
pub const COMPAT_CACHE_MAX_BYTES: u64 = 4 * 1024 * 1024 * 1024;

/// 数据文件完整路径。
pub fn data_file() -> PathBuf {
    data_dir().join(DATA_FILE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_file_is_inside_data_dir() {
        assert_eq!(data_file().parent().unwrap(), data_dir());
    }

    #[test]
    fn cache_dirs_are_separate() {
        assert!(compat_cache_dir()
            .to_string_lossy()
            .contains("compat-cache"));
        assert_ne!(compat_cache_dir(), data_dir());
    }

    #[test]
    fn limits_match_legacy_values() {
        assert_eq!(COMPAT_CACHE_MAX_BYTES, 4_294_967_296);
    }
}
