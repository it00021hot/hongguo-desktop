//! QA 读/写/删的唯一边界。正式模式原样把路径交给系统调用。
//! QA 打开时，先走到现存的真实祖先，拒绝符号链接，再把还不存在的普通文件名接回去。
//! `..` 由现存祖先的规范化结果消化，不拿用户字符串做 `starts_with`。
//! Unix 下读/写/创建对最终路径使用 O_NOFOLLOW：prepare 之后若叶被换成符号链接，打开失败，不跟随。
//! 父目录在 prepare 与系统调用之间被整体换成外链，仍属残留窗口；见本文件测试与冻结报告的缩小范围说明。

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

static EXTRA_PRODUCTION_ROOTS: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

#[cfg(test)]
static BETWEEN_PREPARE_AND_SYSCALL: Mutex<Option<Box<dyn Fn(&Path) + Send>>> = Mutex::new(None);

#[cfg(test)]
fn run_between_prepare_and_syscall(path: &Path) {
    if let Ok(guard) = BETWEEN_PREPARE_AND_SYSCALL.lock() {
        if let Some(hook) = guard.as_ref() {
            hook(path);
        }
    }
}

#[cfg(not(test))]
fn run_between_prepare_and_syscall(_path: &Path) {}

/// 壳数据根、绝对数据根这类生产目录。默认的 `hongguo-downloader` 已经算在里面。
pub fn note_production_root(path: PathBuf) {
    let mut roots = EXTRA_PRODUCTION_ROOTS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if !roots.iter().any(|existing| existing == &path) {
        roots.push(path);
    }
}

/// 当前 QA 允许写入的真实目录。未开 QA 时调用方不应依赖它做放行。
pub fn isolation_root() -> Result<PathBuf, String> {
    let raw = std::env::var("HONGGUO_DATA_DIR").unwrap_or_default();
    validate_isolated_dir(&raw, &production_candidates())
}

/// 正式模式返回原路径。QA 下返回可以交给系统调用的路径，失败时还没有写、没有删。
pub fn prepare_qa_io(path: &Path) -> Result<PathBuf, String> {
    if !super::enabled() || path == Path::new(":memory:") {
        return Ok(path.to_path_buf());
    }
    bind_inside(&isolation_root()?, path)
}

pub fn qa_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let path = prepare_io(path)?;
    run_between_prepare_and_syscall(&path);
    let mut file = open_write_leaf(&path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    confirm_regular_file_inside(&path)?;
    Ok(())
}

pub fn qa_read(path: &Path) -> std::io::Result<Vec<u8>> {
    let path = prepare_io(path)?;
    run_between_prepare_and_syscall(&path);
    let mut file = open_read_leaf(&path)?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    confirm_regular_file_inside(&path)?;
    Ok(bytes)
}

pub fn qa_read_to_string(path: &Path) -> std::io::Result<String> {
    let bytes = qa_read(path)?;
    String::from_utf8(bytes).map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))
}

pub fn qa_rename(from: &Path, to: &Path) -> std::io::Result<()> {
    let from = prepare_io(from)?;
    let to = prepare_io(to)?;
    refuse_leaf_symlink(&from)?;
    refuse_leaf_symlink(&to)?;
    std::fs::rename(&from, &to)?;
    Ok(())
}

pub fn qa_remove_file(path: &Path) -> std::io::Result<()> {
    let path = prepare_io(path)?;
    refuse_leaf_symlink(&path)?;
    std::fs::remove_file(path)
}

pub fn qa_remove_dir_all(path: &Path) -> std::io::Result<()> {
    let path = prepare_io(path)?;
    refuse_leaf_symlink(&path)?;
    let meta = std::fs::symlink_metadata(&path)?;
    if !meta.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "离线 QA 拒绝对非目录执行 remove_dir_all",
        ));
    }
    if super::enabled() {
        if let Ok(root) = isolation_root() {
            let canon = std::fs::canonicalize(&path)?;
            if canon == root {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "离线 QA 拒绝删除隔离根本身",
                ));
            }
        }
    }
    std::fs::remove_dir_all(path)
}

pub fn qa_create_dir_all(path: &Path) -> std::io::Result<()> {
    let path = prepare_io(path)?;
    std::fs::create_dir_all(path)
}

pub fn qa_read_dir(path: &Path) -> std::io::Result<std::fs::ReadDir> {
    let path = prepare_io(path)?;
    refuse_leaf_symlink(&path)?;
    std::fs::read_dir(path)
}

pub fn qa_create(path: &Path) -> std::io::Result<std::fs::File> {
    let path = prepare_io(path)?;
    run_between_prepare_and_syscall(&path);
    open_write_leaf(&path)
}

pub fn qa_open_append(path: &Path) -> std::io::Result<std::fs::File> {
    let path = prepare_io(path)?;
    run_between_prepare_and_syscall(&path);
    open_append_leaf(&path)
}

/// 替换前清掉已有文件。符号链接在解析阶段拒绝，不会删到链接外面。
/// 文件本来不存在，或清掉时遇到与旧调用相同的忽略错误，都返回成功。
pub fn qa_clear_for_replace(path: &Path) -> std::io::Result<()> {
    let path = prepare_io(path)?;
    match std::fs::symlink_metadata(&path) {
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
        Ok(meta) if meta.file_type().is_symlink() => Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!(
                "离线 QA 拒绝符号链接 {}，不会删除或跟随它",
                path.display()
            ),
        )),
        Ok(_) => match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => Ok(()),
        },
    }
}

fn prepare_io(path: &Path) -> std::io::Result<PathBuf> {
    prepare_qa_io(path)
        .map_err(|msg| std::io::Error::new(std::io::ErrorKind::PermissionDenied, msg))
}

fn refuse_leaf_symlink(path: &Path) -> std::io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
        Ok(meta) if meta.file_type().is_symlink() => Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!(
                "离线 QA 拒绝符号链接 {}，不会删除或跟随它",
                path.display()
            ),
        )),
        Ok(_) => Ok(()),
    }
}

fn confirm_regular_file_inside(path: &Path) -> std::io::Result<()> {
    if !super::enabled() {
        return Ok(());
    }
    let meta = std::fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!("离线 QA 拒绝符号链接 {}，读/写后校验失败", path.display()),
        ));
    }
    let canon = std::fs::canonicalize(path)?;
    let root = isolation_root()
        .map_err(|msg| std::io::Error::new(std::io::ErrorKind::PermissionDenied, msg))?;
    if !within_root(&canon, &root) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!("离线 QA 拒绝隔离目录以外的路径 {}", canon.display()),
        ));
    }
    Ok(())
}

#[cfg(unix)]
fn open_read_leaf(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
}

#[cfg(unix)]
fn open_write_leaf(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
    {
        Ok(file) => Ok(file),
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => std::fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path),
        Err(err) => Err(err),
    }
}

#[cfg(unix)]
fn open_append_leaf(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
}

#[cfg(not(unix))]
fn open_read_leaf(path: &Path) -> std::io::Result<std::fs::File> {
    refuse_leaf_symlink(path)?;
    std::fs::File::open(path)
}

#[cfg(not(unix))]
fn open_write_leaf(path: &Path) -> std::io::Result<std::fs::File> {
    refuse_leaf_symlink(path)?;
    std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)
}

#[cfg(not(unix))]
fn open_append_leaf(path: &Path) -> std::io::Result<std::fs::File> {
    refuse_leaf_symlink(path)?;
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
}

fn bind_inside(allowed: &Path, path: &Path) -> Result<PathBuf, String> {
    if !path.is_absolute() {
        return Err(format!("离线 QA 拒绝相对路径 {}", path.display()));
    }
    let mut missing: Vec<std::ffi::OsString> = Vec::new();
    let mut cursor = path.to_path_buf();
    loop {
        match std::fs::symlink_metadata(&cursor) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(format!(
                    "离线 QA 拒绝符号链接 {}，不会删除或跟随它",
                    cursor.display()
                ));
            }
            Ok(_) => {
                let canon = std::fs::canonicalize(&cursor)
                    .map_err(|err| format!("无法规范化 {}: {err}", cursor.display()))?;
                if !within_root(&canon, allowed) {
                    return Err(format!(
                        "离线 QA 拒绝隔离目录以外的路径 {}",
                        canon.display()
                    ));
                }
                missing.reverse();
                let mut resolved = canon;
                for name in missing {
                    if name == "." || name == ".." || name.is_empty() {
                        return Err("离线 QA 拒绝未规范化的路径段".into());
                    }
                    resolved.push(&name);
                    if let Ok(meta) = std::fs::symlink_metadata(&resolved) {
                        if meta.file_type().is_symlink() {
                            return Err(format!(
                                "离线 QA 拒绝符号链接 {}，不会删除或跟随它",
                                resolved.display()
                            ));
                        }
                        resolved = std::fs::canonicalize(&resolved)
                            .map_err(|err| format!("无法规范化路径: {err}"))?;
                        if !within_root(&resolved, allowed) {
                            return Err(format!(
                                "离线 QA 拒绝隔离目录以外的路径 {}",
                                resolved.display()
                            ));
                        }
                    }
                }
                return Ok(resolved);
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                let Some(name) = cursor.file_name() else {
                    return Err("离线 QA 无法解析路径".into());
                };
                if name == "." || name == ".." {
                    return Err("离线 QA 拒绝未规范化的路径段".into());
                }
                missing.push(name.to_os_string());
                let Some(parent) = cursor.parent() else {
                    return Err("离线 QA 找不到现存目录祖先".into());
                };
                if parent.as_os_str().is_empty() {
                    return Err("离线 QA 找不到现存目录祖先".into());
                }
                cursor = parent.to_path_buf();
            }
            Err(err) => return Err(format!("离线 QA 无法检查路径: {err}")),
        }
    }
}

/// 两边都已经是规范化后的真实路径。组件比较不会把 `/isolated` 当成 `/isolated-other` 的前缀。
pub(crate) fn within_root(path: &Path, allowed: &Path) -> bool {
    path == allowed || path.starts_with(allowed)
}

fn production_candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(parent) = dirs::data_dir() {
        out.push(parent.join("hongguo-downloader"));
    }
    let extras = EXTRA_PRODUCTION_ROOTS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    out.extend(extras.iter().cloned());
    out
}

pub fn validate_isolated_dir(raw: &str, production: &[PathBuf]) -> Result<PathBuf, String> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static LOCK: Mutex<()> = Mutex::new(());

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

    fn scratch(name: &str) -> PathBuf {
        let base = std::env::var_os("CARGO_TARGET_DIR")
            .map(PathBuf::from)
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| std::env::temp_dir());
        base.join("hg-offline-qa-io-unit").join(name)
    }

    #[test]
    fn qa_read_refuses_leaf_symlink_to_outside() {
        let _lock = LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let root = scratch("read-leaf");
        let _ = std::fs::remove_dir_all(&root);
        let isolated = root.join("isolated");
        let outside = root.join("outside");
        std::fs::create_dir_all(&isolated).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let sentinel = outside.join("SECRET");
        std::fs::write(&sentinel, b"LEAK-ME").unwrap();
        let leaf = isolated.join("cover.jpg");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&sentinel, &leaf).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_file(&sentinel, &leaf).unwrap();
        let _qa = EnvRestore::set(super::super::ENV, "1");
        let _data = EnvRestore::set("HONGGUO_DATA_DIR", &isolated.to_string_lossy());
        let err = qa_read(&leaf).expect_err("symlink leaf must not be readable");
        assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
        assert_eq!(std::fs::read(&sentinel).unwrap(), b"LEAK-ME");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn leaf_replaced_after_prepare_is_not_followed_on_write() {
        let _lock = LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let root = scratch("toc-leaf");
        let _ = std::fs::remove_dir_all(&root);
        let isolated = root.join("isolated");
        let outside = root.join("outside");
        std::fs::create_dir_all(&isolated).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let sentinel = outside.join("SENTINEL");
        std::fs::write(&sentinel, b"KEEP").unwrap();
        let leaf = isolated.join("new.bin");
        let _qa = EnvRestore::set(super::super::ENV, "1");
        let _data = EnvRestore::set("HONGGUO_DATA_DIR", &isolated.to_string_lossy());
        {
            let mut slot = BETWEEN_PREPARE_AND_SYSCALL.lock().unwrap();
            let sentinel = sentinel.clone();
            *slot = Some(Box::new(move |prepared: &Path| {
                let _ = std::fs::remove_file(prepared);
                #[cfg(unix)]
                std::os::unix::fs::symlink(&sentinel, prepared).unwrap();
                #[cfg(windows)]
                std::os::windows::fs::symlink_file(&sentinel, prepared).unwrap();
            }));
        }
        let err = qa_write(&leaf, b"LEAK").expect_err("post-prepare leaf symlink must fail");
        *BETWEEN_PREPARE_AND_SYSCALL.lock().unwrap() = None;
        let msg = format!("{err}").to_lowercase();
        assert!(
            err.kind() == std::io::ErrorKind::PermissionDenied
                || err.raw_os_error()
                    == Some({
                        #[cfg(unix)]
                        {
                            libc::ELOOP
                        }
                        #[cfg(not(unix))]
                        {
                            -1
                        }
                    })
                || msg.contains("loop")
                || format!("{err}").contains("符号链接"),
            "unexpected err={err:?}"
        );
        assert_eq!(std::fs::read(&sentinel).unwrap(), b"KEEP");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn remove_dir_all_refuses_isolation_root() {
        let _lock = LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let root = scratch("rm-root");
        let _ = std::fs::remove_dir_all(&root);
        let isolated = root.join("isolated");
        std::fs::create_dir_all(&isolated).unwrap();
        std::fs::write(isolated.join("keep.txt"), b"x").unwrap();
        let _qa = EnvRestore::set(super::super::ENV, "1");
        let _data = EnvRestore::set("HONGGUO_DATA_DIR", &isolated.to_string_lossy());
        let err = qa_remove_dir_all(&isolated).expect_err("must refuse deleting isolation root");
        assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
        assert!(isolated.join("keep.txt").exists());
        let child = isolated.join("child");
        std::fs::create_dir_all(&child).unwrap();
        qa_remove_dir_all(&child).unwrap();
        assert!(!child.exists());
        let _ = std::fs::remove_dir_all(&root);
    }
}
