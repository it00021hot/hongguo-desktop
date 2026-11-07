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

/// 离线 QA 必须自带已经存在的真实目录。未开离线 QA 时直接通过，正式启动仍走默认目录。
pub fn offline_qa_data_dir_ok() -> Result<(), String> {
    if !hongguo_common::offline_qa::enabled() {
        return Ok(());
    }
    isolated_data_dir().map(|_| ())
}

/// 开库前的最后一道门：无效目录、生产目录及其别名都在创建父目录之前失败。
pub(crate) fn refuse_offline_store_open(db_path: &Path) -> Result<(), String> {
    if !hongguo_common::offline_qa::enabled() || db_path == Path::new(":memory:") {
        return Ok(());
    }
    let allowed = isolated_data_dir()?;
    let Some(parent) = db_path.parent() else {
        return Err("离线 QA 拒绝没有父目录的库路径".into());
    };
    if !parent.exists() {
        return Err("离线 QA 拒绝创建数据目录".into());
    }
    let parent_canon =
        std::fs::canonicalize(parent).map_err(|e| format!("离线 QA 无法规范化库目录: {e}"))?;
    if parent_canon != allowed {
        return Err("离线 QA 拒绝打开隔离目录以外的库".into());
    }
    Ok(())
}

fn isolated_data_dir() -> Result<PathBuf, String> {
    let raw = std::env::var("HONGGUO_DATA_DIR").unwrap_or_default();
    validate_isolated_dir(&raw, &production_candidates())
}

fn production_candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(parent) = dirs::data_dir() {
        out.push(resolve_data_root(&parent, None));
        if let Some(name) = SHELL_DATA_ROOT.get() {
            out.push(resolve_data_root(&parent, Some(name.as_str())));
        }
    }
    if let Some(root) = ABSOLUTE_DATA_ROOT.get() {
        out.push(root.clone());
    }
    out
}

/// 不创建目录。`production` 用规范化路径比较；生产目录尚不存在时按词法绝对路径拒绝同路径和子目录。
fn validate_isolated_dir(raw: &str, production: &[PathBuf]) -> Result<PathBuf, String> {
    if raw != raw.trim() || raw.is_empty() {
        return Err("HONGGUO_DATA_DIR 为空或带首尾空白，拒绝在开库前另开一个目录".into());
    }
    let path = Path::new(raw);
    if !path.is_absolute() {
        return Err(
            "HONGGUO_OFFLINE_QA 已设置，但 HONGGUO_DATA_DIR 不是绝对路径，拒绝打开默认用户目录"
                .into(),
        );
    }
    let meta = std::fs::symlink_metadata(path).map_err(|_| {
        "HONGGUO_DATA_DIR 不存在。离线 QA 只使用预先建好的真实目录，不会创建它".to_string()
    })?;
    if meta.file_type().is_symlink() {
        return Err("HONGGUO_DATA_DIR 是符号链接，离线 QA 拒绝别名".into());
    }
    if !meta.is_dir() {
        return Err("HONGGUO_DATA_DIR 不是目录".into());
    }
    let canon =
        std::fs::canonicalize(path).map_err(|e| format!("无法规范化 HONGGUO_DATA_DIR: {e}"))?;
    for root in production {
        if path_conflicts(&canon, root) {
            return Err("HONGGUO_OFFLINE_QA 不能使用生产数据目录、它的子目录或别名".into());
        }
    }
    Ok(canon)
}

fn path_conflicts(candidate: &Path, production: &Path) -> bool {
    let prod = std::fs::canonicalize(production).unwrap_or_else(|_| production.to_path_buf());
    candidate == prod || candidate.starts_with(&prod)
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

        let link_err = validate_isolated_dir(&link.to_string_lossy(), &[]).unwrap_err();
        assert!(link_err.contains("符号链接"));
        let _link_dir = EnvRestore::set("HONGGUO_DATA_DIR", &link.to_string_lossy());
        assert!(crate::Store::open(link.join(DB_FILE)).is_err());
        assert!(!real.join(DB_FILE).exists());

        let file_err = validate_isolated_dir(&file.to_string_lossy(), &[]).unwrap_err();
        assert!(file_err.contains("不是目录"));
        assert_eq!(std::fs::read(&file).unwrap(), b"keep");

        let blank = format!(" {} ", real_abs.display());
        assert!(validate_isolated_dir(&blank, &[]).is_err());

        let nested_err =
            validate_isolated_dir(&nested_abs.to_string_lossy(), std::slice::from_ref(&prod))
                .unwrap_err();
        assert!(nested_err.contains("生产"));
        assert!(
            validate_isolated_dir(&real_abs.to_string_lossy(), std::slice::from_ref(&prod)).is_ok()
        );

        let _off = EnvRestore::set(hongguo_common::offline_qa::ENV, "0");
        assert!(offline_qa_data_dir_ok().is_ok());
        let _ = std::fs::remove_dir_all(&root);
    }
}
