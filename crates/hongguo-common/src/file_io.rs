use std::io::{Read, Write};
use std::path::Path;

pub fn write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut file = create(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

pub fn read(path: &Path) -> std::io::Result<Vec<u8>> {
    let mut file = open_read(path)?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    Ok(bytes)
}

pub fn read_to_string(path: &Path) -> std::io::Result<String> {
    String::from_utf8(read(path)?)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))
}

pub fn rename(from: &Path, to: &Path) -> std::io::Result<()> {
    refuse_symlink(from)?;
    refuse_symlink(to)?;
    std::fs::rename(from, to)
}

pub fn remove_file(path: &Path) -> std::io::Result<()> {
    refuse_symlink(path)?;
    std::fs::remove_file(path)
}

pub fn remove_dir_all(path: &Path) -> std::io::Result<()> {
    refuse_symlink(path)?;
    std::fs::remove_dir_all(path)
}

pub fn read_dir(path: &Path) -> std::io::Result<std::fs::ReadDir> {
    refuse_symlink(path)?;
    std::fs::read_dir(path)
}

pub fn clear_for_replace(path: &Path) -> std::io::Result<()> {
    refuse_symlink(path)?;
    // 预清理失败由后续写入或替换报告。
    let _ = std::fs::remove_file(path);
    Ok(())
}

fn refuse_symlink(path: &Path) -> std::io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
        Ok(metadata) if metadata.file_type().is_symlink() => Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!("refusing symbolic link: {}", path.display()),
        )),
        Ok(_) => Ok(()),
    }
}

#[cfg(unix)]
fn open_read(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
}

#[cfg(unix)]
pub fn create(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
    {
        Ok(file) => Ok(file),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            std::fs::OpenOptions::new()
                .write(true)
                .truncate(true)
                .custom_flags(libc::O_NOFOLLOW)
                .open(path)
        }
        Err(error) => Err(error),
    }
}

#[cfg(not(unix))]
fn open_read(path: &Path) -> std::io::Result<std::fs::File> {
    refuse_symlink(path)?;
    std::fs::File::open(path)
}

#[cfg(not(unix))]
pub fn create(path: &Path) -> std::io::Result<std::fs::File> {
    refuse_symlink(path)?;
    std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)
}
