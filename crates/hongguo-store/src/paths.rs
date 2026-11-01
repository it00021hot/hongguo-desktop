//! 数据目录与缓存目录解析。
//!
//! 沿用 Electron 版的路径（`%APPDATA%/hongguo-downloader`），便于用户平滑迁移
//! 下载记录与设置，不因为换了框架就丢数据。
//!
//! 多壳并存：壳可在启动最先声明自己的数据根（[`set_shell_data_root`]），
//! 落在**与默认根平级的独立目录**——与 Tauri 客户端不共用任何路径：库、
//! 缓存互不混用，也互不锁库（GPUI 壳与 Tauri 客户端并存实录：两边抢同一
//! 把库锁，后启动的直接拒绝启动）。曾用过「默认根下嵌子目录」的形态，
//! 被明确否决：数据仍落在 Tauri 的根里，清理/卸载/误删会互相波及。Tauri
//! 壳不声明，维持根目录不变（存量用户数据不动）。

use std::path::{Path, PathBuf};
use std::sync::{OnceLock, RwLock};

static SHELL_DATA_ROOT: OnceLock<String> = OnceLock::new();

/// 绝对数据根覆盖：移动端（Android/iOS）没有桌面数据目录语义——
/// `dirs::data_dir()` 在 Android 返回 `None`，回退会落到进程不可写的
/// `/data/local/tmp`，开库直接失败。壳（Tauri 移动入口）在启动最先
/// 把 app 私有目录钉进来，一次为准。
static ABSOLUTE_DATA_ROOT: OnceLock<PathBuf> = OnceLock::new();

/// 缓存根覆盖（用户在设置里配置的 `cache_dir`）。`None` = 跟随数据目录。
/// 不是 OnceLock：设置保存后要即时作用于**新写入**的缓存（cover 每次写盘、
/// compat 每次转码前都现调 [`cache_root`]），改目录不用重启。
static CACHE_ROOT_OVERRIDE: RwLock<Option<PathBuf>> = RwLock::new(None);

/// 声明本进程的壳数据根（目录名，与默认根 `hongguo-downloader` 平级）。
/// 必须在任何路径解析前调用，一次为准。
pub fn set_shell_data_root(name: &str) {
    let _ = SHELL_DATA_ROOT.set(name.to_string());
}

/// 声明本进程的绝对数据根（移动端用，见 [`ABSOLUTE_DATA_ROOT`]）。
/// 必须在任何路径解析前调用，一次为准；优先级低于 `HONGGUO_DATA_DIR`
/// （测试重定向与用户显式指定的语义不变）。
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
    CACHE_ROOT_OVERRIDE
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
        .unwrap_or_else(data_dir)
}

/// 由「系统数据目录 + 壳声明」解析数据根：未声明 = Electron 迁移的默认根；
/// 声明 = 与默认根**平级**的独立目录（绝不嵌进默认根里）。
fn resolve_data_root(parent: &Path, declared: Option<&str>) -> PathBuf {
    match declared {
        Some(name) => parent.join(name),
        None => parent.join("hongguo-downloader"),
    }
}

/// 数据库文件名（M1 起的正式存储）。
pub const DB_FILE: &str = "hongguo.db";

/// 兼容模式转码缓存目录。
pub fn compat_cache_dir() -> PathBuf {
    cache_root().join("compat-cache")
}

/// 封面转码缓存目录（HEIC→JPEG 产物，按 URL 哈希寻址，可随时清）。
pub fn cover_cache_dir() -> PathBuf {
    cache_root().join("cover-cache")
}

/// 兼容转码缓存上限（沿用现版 4GB）。
pub const COMPAT_CACHE_MAX_BYTES: u64 = 4 * 1024 * 1024 * 1024;

/// 数据库文件完整路径。
pub fn db_file() -> PathBuf {
    data_dir().join(DB_FILE)
}

/// 单测期间把数据目录重定向到临时目录。
///
/// 服务层的落盘路径是写死的 [`data_dir`]，直接测「有没有真的写进磁盘」就会
/// 写进用户真实的数据目录，把下载记录冲掉。
///
/// `HONGGUO_DATA_DIR` 是进程级的：两个用例各改各的会互相把对方的路径顶掉，
/// 于是「断言写到 A 目录」的用例读到的是 B 的文件。所以这里带一把全局锁，
/// 并在 drop 时还原原值——用例中途 panic 也不会把污染留给后面的测试。
// 测试辅助件：本 crate 与下游 crate 的测试都要用（落盘用例不能写进用户真实
// %APPDATA%），而依赖 crate 对下游以非 test 方式编译——`#[cfg(test)]` 对下游
// 不可见。用 feature 显式开启，避免测试代码常驻生产构建。见 Cargo.toml。
//
// 锁在模块级：`ScopedDataDir`（env 重定向）与缓存根覆盖的用例改的都是
// [`compat_cache_dir`] / [`cover_cache_dir`] 的输入，必须串行，否则两个测试
// 各改一半输入，断言落到两个目录上（与历史 1/15 随机失败同因）。
#[cfg(any(test, feature = "test-support"))]
static TEST_PATH_INPUTS_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(any(test, feature = "test-support"))]
pub struct ScopedDataDir {
    _lock: std::sync::MutexGuard<'static, ()>,
    previous: Option<std::ffi::OsString>,
}

#[cfg(any(test, feature = "test-support"))]
impl ScopedDataDir {
    /// 指向 `dir` 并保证它存在。返回的守卫活着期间，本进程的数据读写全落在 `dir`。
    pub fn new(dir: &std::path::Path) -> Self {
        let lock = TEST_PATH_INPUTS_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        std::fs::create_dir_all(dir).expect("创建测试数据目录");
        let previous = std::env::var_os("HONGGUO_DATA_DIR");
        // SAFETY: 测试专用。进程级 env 写入在 edition 2024 起标为 unsafe；
        // 上面那把静态锁保证同进程的测试串行改这个变量，无并发窗口。
        unsafe { std::env::set_var("HONGGUO_DATA_DIR", dir) };
        Self {
            _lock: lock,
            previous,
        }
    }
}

#[cfg(any(test, feature = "test-support"))]
impl Drop for ScopedDataDir {
    fn drop(&mut self) {
        match self.previous.take() {
            // SAFETY: 同 new——锁还在持有期间（守卫尚未析构完），串行访问
            Some(v) => unsafe { std::env::set_var("HONGGUO_DATA_DIR", v) },
            None => unsafe { std::env::remove_var("HONGGUO_DATA_DIR") },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 这两个用例读的是**进程级**的 `HONGGUO_DATA_DIR`，而服务层的用例会通过
    /// [`ScopedDataDir`] 改它。不拿同一把锁的话，一次断言里的两次读可能落到
    /// 两个不同目录上——随机失败（实测约 1/15 次）。
    #[test]
    fn db_file_is_inside_data_dir() {
        let dir = std::env::temp_dir().join(format!("hg-paths-file-{}", std::process::id()));
        let _scope = ScopedDataDir::new(&dir);
        assert_eq!(db_file(), dir.join(DB_FILE));
    }

    #[test]
    fn cache_dirs_are_separate() {
        let dir = std::env::temp_dir().join(format!("hg-paths-cache-{}", std::process::id()));
        let _scope = ScopedDataDir::new(&dir);
        assert_eq!(compat_cache_dir(), dir.join("compat-cache"));
        assert_ne!(compat_cache_dir(), data_dir());
    }

    /// 缓存根覆盖生效于两处磁盘缓存；数据库留在数据目录（用户数据不跟着走）。
    /// 与 env 重定向用例共用模块级锁（见 `TEST_PATH_INPUTS_LOCK` 注释）。
    #[test]
    fn cache_root_override_redirects_disk_caches_only() {
        let _lock = TEST_PATH_INPUTS_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let root = std::env::temp_dir().join(format!("hg-cache-root-{}", std::process::id()));
        set_cache_root_override(Some(root.clone()));
        assert_eq!(compat_cache_dir(), root.join("compat-cache"));
        assert_eq!(cover_cache_dir(), root.join("cover-cache"));
        assert_eq!(db_file(), data_dir().join(DB_FILE), "库不受缓存根影响");
        // 清场放锁内：泄漏进并发的 env 用例就是随机挂
        set_cache_root_override(None);
        assert_eq!(compat_cache_dir(), data_dir().join("compat-cache"));
    }

    #[test]
    fn limits_match_legacy_values() {
        assert_eq!(COMPAT_CACHE_MAX_BYTES, 4_294_967_296);
    }

    /// 壳声明的数据根必须与默认根**平级**：嵌进 `hongguo-downloader` 里等于
    /// 还在共用 Tauri 的数据目录（子目录方案被否决过，别改回去）。
    #[test]
    fn shell_data_root_is_sibling_not_nested() {
        let parent = Path::new("/data");
        let gpui = resolve_data_root(parent, Some("hongguo-gpui"));
        assert_eq!(gpui, Path::new("/data/hongguo-gpui"));
        // 与默认根互不包含
        let tauri = resolve_data_root(parent, None);
        assert_eq!(tauri, Path::new("/data/hongguo-downloader"));
        assert!(!gpui.starts_with(&tauri) && !tauri.starts_with(&gpui));
    }
}
