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
    if let Some(parent) = dirs::data_dir() {
        hongguo_common::offline_qa::note_production_root(parent.join(name));
    }
}

/// 声明本进程的绝对数据根（移动端用，见 `ABSOLUTE_DATA_ROOT`）。必须在任何路径解析前调用，一次为准；优先级低于 `HONGGUO_DATA_DIR`（测试重定向与用户显式指定的语义不变）。
pub fn set_absolute_data_root(dir: &Path) {
    let _ = ABSOLUTE_DATA_ROOT.set(dir.to_path_buf());
    hongguo_common::offline_qa::note_production_root(dir.to_path_buf());
}

/// 应用缓存根覆盖。启动装载与设置保存后调用（见 `Settings::cache_override`）。
pub fn set_cache_root_override(dir: Option<PathBuf>) {
    *CACHE_ROOT_OVERRIDE
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = dir;
}

/// 离线 QA 必须自带已经存在的真实目录。未开离线 QA 时直接通过，正式启动仍走默认目录。
pub fn offline_qa_data_dir_ok() -> Result<(), String> {
    if !hongguo_common::offline_qa::enabled() {
        return Ok(());
    }
    isolated_data_dir().map(|_| ())
}

/// 开库前用和写文件相同的路径解析。不按文件名枚举旁路。
pub(crate) fn refuse_offline_store_open(db_path: &Path) -> Result<(), String> {
    if !hongguo_common::offline_qa::enabled() || db_path == Path::new(":memory:") {
        return Ok(());
    }
    prepare_qa_io(db_path).map(|_| ())
}

/// 正式模式下原样返回。QA 下解析现存祖先并拒绝符号链接，不删除、不跟随。
pub fn refuse_offline_output(path: &Path) -> Result<(), String> {
    prepare_qa_io(path).map(|_| ())
}

pub fn prepare_qa_io(path: &Path) -> Result<PathBuf, String> {
    sync_production_roots();
    hongguo_common::offline_qa::prepare_qa_io(path)
}

pub fn qa_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    sync_production_roots();
    hongguo_common::offline_qa::qa_write(path, bytes)
}

pub fn qa_rename(from: &Path, to: &Path) -> std::io::Result<()> {
    sync_production_roots();
    hongguo_common::offline_qa::qa_rename(from, to)
}

pub fn qa_remove_file(path: &Path) -> std::io::Result<()> {
    sync_production_roots();
    hongguo_common::offline_qa::qa_remove_file(path)
}

pub fn qa_create_dir_all(path: &Path) -> std::io::Result<()> {
    sync_production_roots();
    hongguo_common::offline_qa::qa_create_dir_all(path)
}

pub fn qa_read_dir(path: &Path) -> std::io::Result<std::fs::ReadDir> {
    sync_production_roots();
    hongguo_common::offline_qa::qa_read_dir(path)
}

pub fn qa_remove_dir_all(path: &Path) -> std::io::Result<()> {
    sync_production_roots();
    hongguo_common::offline_qa::qa_remove_dir_all(path)
}

pub fn qa_create(path: &Path) -> std::io::Result<std::fs::File> {
    sync_production_roots();
    hongguo_common::offline_qa::qa_create(path)
}

fn sync_production_roots() {
    if let Some(parent) = dirs::data_dir()
        && let Some(name) = SHELL_DATA_ROOT.get()
    {
        hongguo_common::offline_qa::note_production_root(resolve_data_root(
            &parent,
            Some(name.as_str()),
        ));
    }
    if let Some(root) = ABSOLUTE_DATA_ROOT.get() {
        hongguo_common::offline_qa::note_production_root(root.clone());
    }
}

fn isolated_data_dir() -> Result<PathBuf, String> {
    sync_production_roots();
    hongguo_common::offline_qa::isolation_root()
}

/// 应用数据目录。
pub fn data_dir() -> PathBuf {
    if hongguo_common::offline_qa::enabled() {
        if let Ok(dir) = isolated_data_dir() {
            return dir;
        }
        // 校验失败也不回落到生产目录。开库入口会在创建任何文件前拒绝。
        return PathBuf::from(
            std::env::var("HONGGUO_DATA_DIR").unwrap_or_else(|_| "offline-qa-rejected".into()),
        );
    }
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
    if hongguo_common::offline_qa::enabled() {
        let Ok(allowed) = isolated_data_dir() else {
            return data_dir();
        };
        if let Some(config) = configured
            && let Ok(bound) = hongguo_common::offline_qa::prepare_qa_io(&config)
        {
            return bound;
        }
        return allowed;
    }
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

/// 封面转码缓存目录（HEIC→JPEG 产物，按 URL 哈希寻址，可随时清）。
pub fn cover_cache_dir() -> PathBuf {
    cache_root().join("cover-cache")
}

/// 数据库文件完整路径。
pub fn db_file() -> PathBuf {
    data_dir().join(DB_FILE)
}

/// 测试辅助（feature `test-support` 门控）：单测期间把数据目录重定向到临时目录——服务层落盘路径写死 [`data_dir`]，不重定向就会写进用户真实数据目录。
/// `HONGGUO_DATA_DIR` 与缓存根覆盖都是路径函数的进程级输入、跨用例互顶：这把模块级锁把相关用例串行化，drop 时还原，用例中途 panic 也不留污染。
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
        set_cache_root_override(None);
        assert_eq!(compat_cache_dir(), data_dir().join("compat-cache"));
    }

    #[test]
    fn shell_data_root_is_sibling_not_nested() {
        let parent = Path::new("/data");
        let gpui = resolve_data_root(parent, Some("hongguo-gpui"));
        assert_eq!(gpui, Path::new("/data/hongguo-gpui"));
        let tauri = resolve_data_root(parent, None);
        assert_eq!(tauri, Path::new("/data/hongguo-downloader"));
        assert!(!gpui.starts_with(&tauri) && !tauri.starts_with(&gpui));
    }

    fn scratch(name: &str) -> PathBuf {
        let base = std::env::var_os("CARGO_TARGET_DIR")
            .map(PathBuf::from)
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| {
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.qa-a-offline-scratch")
            });
        let dir = base.join("hg-offline-qa-scratch").join(name);
        if dir.is_absolute() {
            dir
        } else {
            std::env::current_dir().unwrap().join(dir)
        }
    }

    struct EnvRestore {
        key: &'static str,
        previous: Option<std::ffi::OsString>,
    }

    impl EnvRestore {
        fn set(key: &'static str, value: &str) -> Self {
            let previous = std::env::var_os(key);
            unsafe { std::env::set_var(key, value) };
            Self { key, previous }
        }
    }

    impl Drop for EnvRestore {
        fn drop(&mut self) {
            match &self.previous {
                Some(value) => unsafe { std::env::set_var(self.key, value) },
                None => unsafe { std::env::remove_var(self.key) },
            }
        }
    }

    #[test]
    fn offline_missing_dir_does_not_create_a_database() {
        let _lock = TEST_PATH_INPUTS_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let missing = scratch("missing");
        let _ = std::fs::remove_dir_all(&missing);
        assert!(!missing.exists());
        let _qa = EnvRestore::set(hongguo_common::offline_qa::ENV, "1");
        let _dir = EnvRestore::set("HONGGUO_DATA_DIR", &missing.to_string_lossy());
        let db = missing.join(DB_FILE);
        let opened = crate::Store::open(&db);
        assert!(opened.is_err());
        assert!(!missing.exists(), "拒绝发生在创建目录之前");
        assert!(!db.exists(), "拒绝发生在创建数据库文件之前");
    }

    #[test]
    fn offline_rejects_symlink_file_blank_and_production_alias() {
        let _lock = TEST_PATH_INPUTS_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let root = scratch("alias");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let real = root.join("real");
        let nested = root.join("prod").join("child");
        std::fs::create_dir_all(&real).unwrap();
        std::fs::create_dir_all(&nested).unwrap();
        let link = root.join("link");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&real, &link).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(&real, &link).unwrap();
        let file = root.join("not-a-dir");
        std::fs::write(&file, b"keep").unwrap();
        let real_abs = std::fs::canonicalize(&real).unwrap();
        let nested_abs = std::fs::canonicalize(&nested).unwrap();
        let prod = nested_abs.parent().unwrap().to_path_buf();
        let _qa = EnvRestore::set(hongguo_common::offline_qa::ENV, "1");

        let link_err =
            hongguo_common::offline_qa::validate_isolated_dir(&link.to_string_lossy(), &[])
                .unwrap_err();
        assert!(link_err.contains("符号链接"));
        let _link_dir = EnvRestore::set("HONGGUO_DATA_DIR", &link.to_string_lossy());
        assert!(crate::Store::open(link.join(DB_FILE)).is_err());
        assert!(!real.join(DB_FILE).exists());

        let file_err =
            hongguo_common::offline_qa::validate_isolated_dir(&file.to_string_lossy(), &[])
                .unwrap_err();
        assert!(file_err.contains("不是目录"));
        assert_eq!(std::fs::read(&file).unwrap(), b"keep");

        let blank = format!(" {} ", real_abs.display());
        assert!(hongguo_common::offline_qa::validate_isolated_dir(&blank, &[]).is_err());

        let nested_err = hongguo_common::offline_qa::validate_isolated_dir(
            &nested_abs.to_string_lossy(),
            std::slice::from_ref(&prod),
        )
        .unwrap_err();
        assert!(nested_err.contains("生产"));
        assert!(
            hongguo_common::offline_qa::validate_isolated_dir(
                &real_abs.to_string_lossy(),
                std::slice::from_ref(&prod)
            )
            .is_ok()
        );

        let _off = EnvRestore::set(hongguo_common::offline_qa::ENV, "0");
        assert!(offline_qa_data_dir_ok().is_ok());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn offline_final_file_symlink_and_cache_override_stay_inside() {
        let _lock = TEST_PATH_INPUTS_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let root = scratch("final-link");
        let _ = std::fs::remove_dir_all(&root);
        let isolated = root.join("isolated");
        let prod = root.join("system-data").join("hongguo-downloader");
        std::fs::create_dir_all(&isolated).unwrap();
        std::fs::create_dir_all(&prod).unwrap();
        let outside = prod.join("hongguo.db");
        std::fs::write(&outside, b"keep-prod").unwrap();
        let db = isolated.join(DB_FILE);
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, &db).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_file(&outside, &db).unwrap();
        let sidecar = PathBuf::from(format!("{}.register-block", db.display()));
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, &sidecar).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_file(&outside, &sidecar).unwrap();
        let _qa = EnvRestore::set(hongguo_common::offline_qa::ENV, "1");
        let _data = EnvRestore::set("HONGGUO_DATA_DIR", &isolated.to_string_lossy());
        let err = refuse_offline_store_open(&db).unwrap_err();
        assert!(err.contains("最终路径") || err.contains("符号"));
        assert!(crate::Store::open(&db).is_err());
        assert_eq!(std::fs::read(&outside).unwrap(), b"keep-prod");
        assert!(
            std::fs::symlink_metadata(&db)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert!(refuse_offline_output(&sidecar).is_err());
        set_cache_root_override(Some(prod.clone()));
        let cover = cover_cache_dir();
        assert!(cover.starts_with(std::fs::canonicalize(&isolated).unwrap()));
        assert!(!prod.join("cover-cache").exists());
        set_cache_root_override(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn qa_io_keeps_outside_sentinels_and_still_writes_inside() {
        let _lock = TEST_PATH_INPUTS_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let root = scratch("qa-io");
        let _ = std::fs::remove_dir_all(&root);
        let isolated = root.join("isolated");
        let outside = root.join("mock-prod");
        std::fs::create_dir_all(&isolated).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let sentinel = outside.join("SENTINEL");
        std::fs::write(&sentinel, b"KEEP").unwrap();
        let _qa = EnvRestore::set(hongguo_common::offline_qa::ENV, "1");
        let _data = EnvRestore::set("HONGGUO_DATA_DIR", &isolated.to_string_lossy());

        let dotdot = isolated.join("..").join("dotdot-new");
        set_cache_root_override(Some(dotdot.clone()));
        let resolved = cover_cache_dir();
        assert!(resolved.starts_with(std::fs::canonicalize(&isolated).unwrap()));
        assert!(qa_create_dir_all(&dotdot.join("cover-cache")).is_err());
        assert!(
            !root.join("dotdot-new").exists(),
            "dotdot 不能在隔离外建目录"
        );

        let ancestor = isolated.join("ancestor");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, &ancestor).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(&outside, &ancestor).unwrap();
        set_cache_root_override(Some(ancestor.join("new-cache")));
        assert!(qa_create_dir_all(&ancestor.join("new-cache")).is_err());
        assert!(!outside.join("new-cache").exists());

        set_cache_root_override(None);
        let cover = isolated.join("cover-cache");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, &cover).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(&outside, &cover).unwrap();
        assert!(qa_create_dir_all(&cover_cache_dir()).is_err());
        assert!(qa_write(&cover.join("leaf.tmp"), b"LEAK").is_err());
        assert_eq!(std::fs::read(&sentinel).unwrap(), b"KEEP");
        assert!(
            std::fs::symlink_metadata(&cover)
                .unwrap()
                .file_type()
                .is_symlink()
        );

        let _ = std::fs::remove_file(&cover);
        std::fs::create_dir_all(&cover).unwrap();
        let leaf = cover.join("real.tmp");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&sentinel, &leaf).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_file(&sentinel, &leaf).unwrap();
        assert!(qa_write(&leaf, b"LEAK").is_err());
        assert_eq!(std::fs::read(&sentinel).unwrap(), b"KEEP");
        assert!(qa_remove_file(&leaf).is_err());
        assert!(
            std::fs::symlink_metadata(&leaf)
                .unwrap()
                .file_type()
                .is_symlink()
        );

        let inside = isolated.join("notes.txt");
        qa_write(&inside, b"inside").unwrap();
        assert_eq!(std::fs::read(&inside).unwrap(), b"inside");
        qa_remove_file(&inside).unwrap();
        assert!(!inside.exists());
        assert_eq!(std::fs::read(&sentinel).unwrap(), b"KEEP");
        set_cache_root_override(None);
        let _ = std::fs::remove_dir_all(&root);
    }
}
