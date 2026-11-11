use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use hongguo_store::paths;
use hongguo_video::streaming::SparseBuffer;
use parking_lot::Mutex;
use sha2::{Digest, Sha256};

const CAPACITY: u64 = 200 * 1024 * 1024;
const EXTENSION: &str = "range";
static GENERATION: Mutex<u64> = Mutex::new(0);

#[derive(Clone)]
pub(super) struct MediaCache {
    dir: PathBuf,
    fingerprint: String,
    generation: u64,
}

impl MediaCache {
    pub(super) fn new(
        device: &hongguo_signer::device::DeviceProfile,
        vid: &str,
        definition: u32,
        file_hash: &str,
    ) -> Option<Self> {
        let device_id = device.get("device_id");
        let install_id = device.get("iid");
        if device_id.is_empty() || install_id.is_empty() || file_hash.is_empty() {
            return None;
        }
        let mut digest = Sha256::new();
        for part in [device_id, install_id, vid, file_hash] {
            digest.update(part.as_bytes());
            digest.update([0]);
        }
        digest.update(definition.to_be_bytes());
        Some(Self::for_key(hex::encode(digest.finalize())))
    }

    pub(super) fn for_key(fingerprint: String) -> Self {
        Self {
            dir: paths::online_cache_dir(),
            fingerprint,
            generation: *GENERATION.lock(),
        }
    }

    pub(super) fn key(&self) -> &str {
        &self.fingerprint
    }

    pub(super) async fn load(&self) -> Option<Arc<SparseBuffer>> {
        let cache = self.clone();
        crate::runtime::spawn_blocking(move || cache.load_ranges())
            .await
            .ok()
            .flatten()
    }

    fn load_ranges(&self) -> Option<Arc<SparseBuffer>> {
        let generation = GENERATION.lock();
        if self.generation != *generation {
            return None;
        }
        let entries = files(&self.dir);
        let source_len = entries
            .iter()
            .filter_map(|entry| {
                entry
                    .range(&self.fingerprint)
                    .map(|(len, _)| (entry.modified, len))
            })
            .max()?
            .1;
        let sparse = SparseBuffer::new(source_len);
        for entry in entries {
            if let Some((total, start)) = entry.range(&self.fingerprint)
                && total == source_len
                && let Ok(bytes) = paths::qa_read(&entry.path)
                && bytes.len() as u64 == entry.bytes
            {
                sparse.write(start, &bytes);
            }
        }
        (sparse.downloaded() != 0).then_some(sparse)
    }

    pub(super) async fn remember(&self, total: u64, start: u64, bytes: Vec<u8>) -> Vec<u8> {
        let size = bytes.len() as u64;
        if total > CAPACITY || size == 0 || start.saturating_add(size) > total {
            return bytes;
        }
        let cache = self.clone();
        let bytes = Arc::new(bytes);
        let to_write = Arc::clone(&bytes);
        let result =
            crate::runtime::spawn_blocking(move || cache.write(total, start, &to_write)).await;
        if let Ok(Err(error)) = result {
            log::debug!("[Online] 写入媒体磁盘缓存失败: {error}");
        }
        Arc::unwrap_or_clone(bytes)
    }

    fn write(&self, total: u64, start: u64, bytes: &[u8]) -> std::io::Result<()> {
        let generation = GENERATION.lock();
        if self.generation != *generation {
            return Ok(());
        }
        let path = self
            .dir
            .join(format!("{}-{total}-{start}.{EXTENSION}", self.fingerprint));
        paths::qa_create_dir_all(&self.dir)?;
        let temp = path.with_extension("tmp");
        paths::qa_write(&temp, bytes)?;
        paths::qa_rename(&temp, &path)?;
        prune(&self.dir);
        Ok(())
    }
}

struct CacheFile {
    path: PathBuf,
    bytes: u64,
    modified: SystemTime,
}

impl CacheFile {
    fn range(&self, fingerprint: &str) -> Option<(u64, u64)> {
        let stem = self.path.file_stem()?.to_str()?;
        let (total, start) = stem
            .strip_prefix(fingerprint)?
            .strip_prefix('-')?
            .split_once('-')?;
        let total = total.parse::<u64>().ok()?;
        let start = start.parse::<u64>().ok()?;
        (total <= CAPACITY && self.bytes > 0 && start.checked_add(self.bytes)? <= total)
            .then_some((total, start))
    }
}

fn files(dir: &Path) -> Vec<CacheFile> {
    let Ok(entries) = paths::qa_read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if path.extension()?.to_str()? != EXTENSION || !entry.file_type().ok()?.is_file() {
                return None;
            }
            let metadata = entry.metadata().ok()?;
            Some(CacheFile {
                path,
                bytes: metadata.len(),
                modified: metadata.modified().ok()?,
            })
        })
        .collect()
}

fn prune(dir: &Path) {
    let mut entries = files(dir);
    let mut bytes: u64 = entries.iter().map(|entry| entry.bytes).sum();
    entries.sort_unstable_by_key(|entry| entry.modified);
    for entry in entries {
        if bytes <= CAPACITY {
            break;
        }
        if paths::qa_remove_file(&entry.path).is_ok() {
            bytes -= entry.bytes;
        }
    }
}

pub(crate) fn usage() -> (u64, u64) {
    let _generation = GENERATION.lock();
    let entries = files(&paths::online_cache_dir());
    (
        entries.len() as u64,
        entries.iter().map(|entry| entry.bytes).sum(),
    )
}

/// 与官方 getCacheSize 一致：除 360P 外，最后一个已有媒体字节的档位优先。
pub(crate) fn cached_definition(
    device: &hongguo_signer::device::DeviceProfile,
    vid: &str,
    candidates: &[hongguo_api::resolution_policy::Candidate],
) -> Option<u32> {
    let entries = files(&paths::online_cache_dir());
    candidates
        .iter()
        .filter(|value| value.definition != 360)
        .filter(|value| {
            if hongguo_stream::cache().cached_bytes(vid, value.definition) > 0 {
                return true;
            }
            value
                .file_hash
                .as_deref()
                .and_then(|hash| MediaCache::new(device, vid, value.definition, hash))
                .is_some_and(|cache| {
                    entries
                        .iter()
                        .any(|entry| entry.range(cache.key()).is_some())
                })
        })
        .map(|value| value.definition)
        .next_back()
}

pub(crate) fn clear() -> (u64, u64) {
    let mut generation = GENERATION.lock();
    *generation = generation.wrapping_add(1);
    let mut files_removed = 0;
    let mut bytes_removed = 0;
    let dir = paths::online_cache_dir();
    for entry in files(&dir) {
        if paths::qa_remove_file(&entry.path).is_ok() {
            files_removed += 1;
            bytes_removed += entry.bytes;
        }
    }
    if let Ok(entries) = paths::qa_read_dir(&dir) {
        for entry in entries.flatten() {
            if entry.path().extension().is_some_and(|ext| ext == "tmp") {
                let _ = paths::qa_remove_file(&entry.path());
            }
        }
    }
    (files_removed, bytes_removed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hongguo_signer::device::DeviceProfile;

    #[test]
    fn reloads_ranges_without_crossing_profiles_or_quality_and_clear_invalidates_writers() {
        let dir = std::env::temp_dir().join(format!("hg-online-ranges-{}", std::process::id()));
        let _scope = paths::ScopedDataDir::new(&dir);
        let device = DeviceProfile::from_pairs(&[("device_id", "100"), ("iid", "101")], "");
        let source = "file-hash";
        let cache = MediaCache::new(&device, "13", 720, source).unwrap();
        cache.write(12, 0, b"abc").unwrap();
        cache.write(12, 9, b"xyz").unwrap();

        let reopened = MediaCache::new(&device, "13", 720, source).unwrap();
        let sparse = reopened.load_ranges().unwrap();
        assert_eq!(sparse.len(), 12);
        assert_eq!(sparse.next_gap(0), Some((3, 9)));
        let mut tail = [0; 3];
        sparse.read_into(9, &mut tail);
        assert_eq!(&tail, b"xyz");
        assert!(
            MediaCache::new(&device, "13", 1080, source)
                .unwrap()
                .load_ranges()
                .is_none()
        );

        let mut other_device = device;
        other_device.set("iid", "102");
        assert!(
            MediaCache::new(&other_device, "13", 720, source)
                .unwrap()
                .load_ranges()
                .is_none()
        );
        assert_eq!(clear(), (2, 6));
        cache.write(12, 3, b"late").unwrap();
        assert_eq!(usage(), (0, 0));
    }
}
