//! 兼容格式合并的分集转码缓存；播放不转码。

use std::path::PathBuf;

use hongguo_store::paths::compat_cache_dir;

// Format 4 contains HEVC archives; earlier AVC artifacts must be regenerated.
const COMPLETION_FORMAT: &str = "4";

/// 某集的缓存文件名（归档/合并默认）。
pub fn cache_file(series_id: &str, vid_index: u32) -> PathBuf {
    compat_cache_dir().join(format!("{}_{:03}.mp4", sanitize(series_id), vid_index))
}

/// 命中的缓存路径；没有则 `None`。只有完成标识和当前文件一致才算命中，避免把上次的半成品当结果。
pub fn cached_path(series_id: &str, vid_index: u32) -> Option<PathBuf> {
    let path = cache_file(series_id, vid_index);
    is_complete(&path).then_some(path)
}

pub(crate) fn is_complete(path: &std::path::Path) -> bool {
    completion_matches(path)
}

fn marker_path(path: &std::path::Path) -> PathBuf {
    PathBuf::from(format!("{}.complete", path.display()))
}

fn marker_partial(path: &std::path::Path) -> PathBuf {
    PathBuf::from(format!("{}.complete.writing", path.display()))
}

/// 把完成标识绑到当前文件字节。写到临时名再改名；改名失败则没有完成标识。
pub fn seal_complete(path: &std::path::Path) -> std::io::Result<()> {
    let bytes = std::fs::read(path)?;
    let body = format!(
        "{COMPLETION_FORMAT}\n{}\n{}\n",
        bytes.len(),
        sha256_hex(&bytes)
    );
    let partial = marker_partial(path);
    let dest = marker_path(path);
    hongguo_common::file_io::write(&partial, body.as_bytes())?;
    if let Err(err) = hongguo_common::file_io::rename(&partial, &dest) {
        let _ = hongguo_common::file_io::remove_file(&partial);
        return Err(err);
    }
    Ok(())
}

pub fn discard_completion(path: &std::path::Path) {
    let _ = hongguo_common::file_io::remove_file(&marker_path(path));
    let _ = hongguo_common::file_io::remove_file(&marker_partial(path));
}

pub fn completion_matches(path: &std::path::Path) -> bool {
    let marker = marker_path(path);
    let Ok(raw) = std::fs::read_to_string(marker) else {
        return false;
    };
    let mut lines = raw.lines();
    if lines.next() != Some(COMPLETION_FORMAT) {
        return false;
    }
    let Ok(len) = lines.next().unwrap_or("").parse::<u64>() else {
        return false;
    };
    let Some(hash) = lines.next() else {
        return false;
    };
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    if meta.len() != len {
        return false;
    }
    let Ok(bytes) = std::fs::read(path) else {
        return false;
    };
    sha256_hex(&bytes) == hash
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(bytes))
}

fn sanitize(id: &str) -> String {
    id.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

pub fn clear() -> usize {
    let dir = compat_cache_dir();
    let mut n = 0;
    if let Ok(entries) = hongguo_common::file_io::read_dir(&dir) {
        for e in entries.flatten() {
            if e.path().extension().and_then(|x| x.to_str()) == Some("mp4")
                && hongguo_common::file_io::remove_file(&e.path()).is_ok()
            {
                n += 1;
            }
        }
    }
    n
}
