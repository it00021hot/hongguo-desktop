//! 兼容转码缓存：上限 4GB，按 atime 淘汰。

use std::path::PathBuf;

use crate::store::paths::{compat_cache_dir, COMPAT_CACHE_MAX_BYTES};

/// 某集的缓存文件名。
pub fn cache_file(series_id: &str, vid_index: u32) -> PathBuf {
    compat_cache_dir().join(format!("{}_{:03}.mp4", sanitize(series_id), vid_index))
}

/// 命中的缓存路径；没有则 `None`。
///
/// 存在的文件必须够大才算命中，避免把上次的半成品当结果。
pub fn cached_path(series_id: &str, vid_index: u32) -> Option<PathBuf> {
    let path = cache_file(series_id, vid_index);
    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    if size > 100 * 1024 {
        Some(path)
    } else {
        None
    }
}

fn sanitize(id: &str) -> String {
    id.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

/// 兼容转码缓存上限的**下界**。
///
/// 低于这个值就没意义了：一部剧的转码产物动辄几百 MB，缓存放不下两集，
/// 合并时每集都要重转一遍。
const CAP_FLOOR: u64 = 2 * 1024 * 1024 * 1024;

/// 兼容转码缓存上限的**上界**。
///
/// 再大的盘也不该让缓存吃掉几十 GB——它是可重算的中间产物，不是用户的数据。
const CAP_CEILING: u64 = 16 * 1024 * 1024 * 1024;

/// 本次淘汰用多少字节做上限。
///
/// 固定 4GB 是拍脑袋的：大盘上限制太紧（250 集的剧按 libx264 crf23 约 6GB，
/// 必然反复淘汰重转），小盘上又放得太满。跟着可用空间走更合理：
/// **最多占当前空闲的一成**，落在 [2GB, 16GB] 之间。
fn effective_cap() -> u64 {
    let free = crate::media::disk_space::free_space_at(&compat_cache_dir()).unwrap_or(0);
    if free == 0 {
        // 查不到空闲就退回旧行为，不因为一次探测失败就把缓存清光
        return COMPAT_CACHE_MAX_BYTES;
    }
    (free / 10).clamp(CAP_FLOOR, CAP_CEILING)
}

/// 按 atime 淘汰到上限以下。
pub fn trim() -> usize {
    let dir = compat_cache_dir();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return 0;
    };
    let cap = effective_cap();

    let mut items: Vec<(std::time::SystemTime, PathBuf, u64)> = Vec::new();
    let mut total = 0u64;
    for e in entries.flatten() {
        let p = e.path();
        if p.extension().and_then(|x| x.to_str()) != Some("mp4") {
            continue;
        }
        let Ok(m) = e.metadata() else { continue };
        let at = m
            .accessed()
            .or_else(|_| m.modified())
            .unwrap_or(std::time::UNIX_EPOCH);
        total += m.len();
        items.push((at, p, m.len()));
    }
    if total <= cap {
        return 0;
    }

    items.sort_by_key(|(t, _, _)| *t);
    let mut removed = 0;
    for (_, p, size) in items {
        if total <= cap {
            break;
        }
        if std::fs::remove_file(&p).is_ok() {
            total -= size;
            removed += 1;
        }
    }
    removed
}

/// 清空缓存。
pub fn clear() -> usize {
    let dir = compat_cache_dir();
    let mut n = 0;
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for e in entries.flatten() {
            if e.path().extension().and_then(|x| x.to_str()) == Some("mp4")
                && std::fs::remove_file(e.path()).is_ok()
            {
                n += 1;
            }
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_file_is_padded() {
        let p = cache_file("123", 7);
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        assert!(name.ends_with("007.mp4"), "实际: {name}");
    }

    #[test]
    fn cache_file_sanitizes_id() {
        let p = cache_file("a/b:c", 1);
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        assert!(!name.contains('/') && !name.contains(':'));
    }
}
