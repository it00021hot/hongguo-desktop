//! 数据目录与缓存目录解析。沿用 Electron 版路径（`%APPDATA%/hongguo-downloader`），便于用户平滑迁移下载记录与设置。
//!
//! 多壳并存：壳可在启动最先声明数据根（[`set_shell_data_root`]），落在与默认根平级的独立目录——与 Tauri 客户端不共用任何路径，库、缓存互不混用也互不锁库；绝不嵌进默认根（清理/卸载/误删会互相波及）。Tauri 壳不声明，维持根目录不变。

use std::path::{Path, PathBuf};
use std::sync::{OnceLock, RwLock};

static SHELL_DATA_ROOT: OnceLock<String> = OnceLock::new();

/// 绝对数据根覆盖：移动端没有桌面数据目录语义（`dirs::data_dir()` 返回 `None`，回退会落到进程不可写的 `/data/local/tmp`，开库直接失败）。壳在启动最先把 app 私有目录钉进来，一次为准。
static ABSOLUTE_DATA_ROOT: OnceLock<PathBuf> = OnceLock::new();

/// 缓存根覆盖（用户配置的 `cache_dir`，`None` = 跟随数据目录）。不是 OnceLock：设置保存后要即时作用于**新写入**的缓存，改目录不用重启。
static CACHE_ROOT_OVERRIDE: RwLock<Option<PathBuf>> = RwLock::new(None);

/// 声明本进程的壳数据根（目录名，与默认根 `hongguo-downloader` 平级）。
/// 必须在任何路径解析前调用，一次为准。
pub fn set_shell_data_root(name: &str) {
    let _ = SHELL_DATA_ROOT.set(name.to_string());
}

/// 声明本进程的绝对数据根（移动端用，见 `ABSOLUTE_DATA_ROOT`）。必须在任何路径解析前调用，一次为准；优先级低于 `HONGGUO_DATA_DIR`（测试重定向与用户显式指定的语义不变）。
pub fn set_absolute_data_root(dir: &Path) {
    let _ = ABSOLUTE_DATA_ROOT.set(dir.to_path_buf());
}

/// 应用缓存根覆盖。启动装载与设置保存后调用（见 `Settings::cache_override`）。
pub fn set_cache_root_override(dir: Option<PathBuf>) {
    *CACHE_ROOT_OVERRIDE
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = dir;
}

/// 应用数据目录。
pub fn data_dir() -> PathBuf {
    if let Ok(explicit) = std::env::var("HONGGUO_DATA_DIR") {
        return PathBuf::from(explicit);
    }
    if let Some(root) = ABSOLUTE_DATA_ROOT.get() {
        return root.clone();
    }
    let parent = dirs::data_dir().unwrap_or_else(std::env::temp_dir);
    resolve_data_root(&parent, SHELL_DATA_ROOT.get().map(String::as_str))
}

/// 缓存根：用户配置优先，否则跟随数据目录。数据库/崩溃日志/下载等用户数据
/// 不走这里——可配置的只有可随时清的缓存。
fn cache_root() -> PathBuf {
    let configured = CACHE_ROOT_OVERRIDE
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();
    configured.unwrap_or_else(data_dir)
}

/// 由「系统数据目录 + 壳声明」解析数据根：未声明 = Electron 迁移的默认根；
/// 声明 = 与默认根**平级**的独立目录（绝不嵌进默认根里）。
fn resolve_data_root(parent: &Path, declared: Option<&str>) -> PathBuf {
    match declared {
        Some(name) => parent.join(name),
        None => parent.join("hongguo-downloader"),
    }
}

/// 数据库文件名。
pub const DB_FILE: &str = "hongguo.db";

/// 兼容模式转码缓存目录。
pub fn compat_cache_dir() -> PathBuf {
    cache_root().join("compat-cache")
}

pub fn online_cache_dir() -> PathBuf {
    cache_root().join("online-cache")
}

/// 封面转码缓存目录（HEIC→JPEG 产物，按 URL 哈希寻址，可随时清）。
pub fn cover_cache_dir() -> PathBuf {
    cache_root().join("cover-cache")
}

/// 数据库文件完整路径。
pub fn db_file() -> PathBuf {
    data_dir().join(DB_FILE)
}
