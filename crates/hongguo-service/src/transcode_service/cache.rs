//! 兼容转码缓存：滚动窗口，最多保留 [`KEEP_EPISODES`] 集，按 atime 淘汰。
//! 播放档与合并归档分文件名（`.play.mp4` / `.mp4`），避免播放压分辨率后污染合并输入。

use std::path::PathBuf;

use hongguo_media_platform::RateProfile;
use hongguo_store::paths::compat_cache_dir;

/// 某集的缓存文件名（归档/合并默认）。
pub fn cache_file(series_id: &str, vid_index: u32) -> PathBuf {
    cache_file_for(series_id, vid_index, RateProfile::Archive)
}

/// 按码率档选缓存路径：播放档带 `.play` 后缀，与归档互不覆盖。
pub fn cache_file_for(series_id: &str, vid_index: u32, rate: RateProfile) -> PathBuf {
    let stem = format!("{}_{:03}", sanitize(series_id), vid_index);
    let name = match rate {
        RateProfile::Archive => format!("{stem}.mp4"),
        RateProfile::Playback => format!("{stem}.play.mp4"),
    };
    compat_cache_dir().join(name)
}

/// 命中的缓存路径；没有则 `None`。存在的文件必须够大才算命中，避免把上次的半成品当结果。
pub fn cached_path(series_id: &str, vid_index: u32) -> Option<PathBuf> {
    cached_path_for(series_id, vid_index, RateProfile::Archive)
}

/// 按码率档查缓存。
pub fn cached_path_for(series_id: &str, vid_index: u32, rate: RateProfile) -> Option<PathBuf> {
    let path = cache_file_for(series_id, vid_index, rate);
    if hongguo_protocol::grow::is_incomplete(&path) {
        return None;
    }
    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    if size > hongguo_common::consts::units::MIN_MEDIA_BYTES {
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

/// 最多保留的集数：兼容播放是「当前集 + 预转下一集」的滚动窗口，三份有余；
/// 攒多了就是吃磁盘（实测能滚到 14GB+）。合并转码的瞬时多集不经这里约束——
/// 中途逐出会让拼接拿到断路径，消费完就地删除（见 `merge_service::compat`）。
const KEEP_EPISODES: usize = 3;

/// 按 atime 淘汰：只保留最近 [`KEEP_EPISODES`] 个文件，返回删除数。
/// 被占用的文件（正在被播放器读）删不掉就跳过，宁可暂时多留一个。
pub fn trim() -> usize {
    let dir = compat_cache_dir();
    let Ok(entries) = hongguo_store::paths::qa_read_dir(&dir) else {
        return 0;
    };
    let mut items: Vec<(std::time::SystemTime, PathBuf)> = Vec::new();
    for e in entries.flatten() {
        let p = e.path();
        if p.extension().and_then(|x| x.to_str()) != Some("mp4") {
            continue;
        }
        if hongguo_protocol::grow::is_incomplete(&p) {
            continue;
        }
        let Ok(m) = e.metadata() else { continue };
        let at = m
            .accessed()
            .or_else(|_| m.modified())
            .unwrap_or(std::time::UNIX_EPOCH);
        items.push((at, p));
    }
    if items.len() <= KEEP_EPISODES {
        return 0;
    }
    items.sort_by_key(|(t, _)| *t);
    let excess = items.len() - KEEP_EPISODES;
    let mut removed = 0;
    for (_, p) in &items[..excess] {
        if hongguo_store::paths::qa_remove_file(p).is_ok() {
            removed += 1;
        }
    }
    removed
}

pub fn clear() -> usize {
    let dir = compat_cache_dir();
    let mut n = 0;
    if let Ok(entries) = hongguo_store::paths::qa_read_dir(&dir) {
        for e in entries.flatten() {
            if e.path().extension().and_then(|x| x.to_str()) == Some("mp4")
                && hongguo_store::paths::qa_remove_file(&e.path()).is_ok()
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
    use hongguo_store::paths::ScopedDataDir;

    #[test]
    fn cache_file_is_padded() {
        let p = cache_file("123", 7);
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        assert!(name.ends_with("007.mp4"), "实际: {name}");
    }

    #[test]
    fn playback_cache_uses_a_distinct_suffix() {
        let p = cache_file_for("s", 1, RateProfile::Playback);
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        assert!(name.ends_with("001.play.mp4"), "实际: {name}");
        assert_ne!(
            cache_file_for("s", 1, RateProfile::Archive),
            cache_file_for("s", 1, RateProfile::Playback)
        );
    }

    #[test]
    fn cache_file_sanitizes_id() {
        let p = cache_file("a/b:c", 1);
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        assert!(!name.contains('/') && !name.contains(':'));
    }

    #[test]
    fn trim_keeps_only_the_newest_episodes() {
        let dir = std::env::temp_dir().join(format!("hg-trim-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let _scope = ScopedDataDir::new(&dir);
        std::fs::create_dir_all(compat_cache_dir()).unwrap();
        for i in 1..=5 {
            std::fs::write(cache_file("s", i), vec![0u8; 128]).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(15));
        }
        assert_eq!(trim(), 2, "5 个文件应删掉最旧的 2 个");
        assert!(!cache_file("s", 1).exists() && !cache_file("s", 2).exists());
        assert!(cache_file("s", 3).exists() && cache_file("s", 5).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn trim_below_the_cap_is_a_no_op() {
        let dir = std::env::temp_dir().join(format!("hg-trim-cap-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let _scope = ScopedDataDir::new(&dir);
        std::fs::create_dir_all(compat_cache_dir()).unwrap();
        for i in 1..=3 {
            std::fs::write(cache_file("s", i), vec![0u8; 128]).unwrap();
        }
        assert_eq!(trim(), 0, "未超上限不动文件");
        assert!(cache_file("s", 1).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
