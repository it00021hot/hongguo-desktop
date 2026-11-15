//! 缓存占用统计与一键清理：在线内存流、在线媒体区间、兼容转码及封面转码。

use serde::Serialize;

use hongguo_store::paths::{compat_cache_dir, cover_cache_dir};

/// 单处缓存的占用。
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CachePart {
    /// 条目数（磁盘上是文件数，内存流是分条数）。
    pub files: u64,
    pub bytes: u64,
}

/// 缓存占用总览（前端直接消费）。
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheUsage {
    /// 在线流缓存：在内存里，不占磁盘。
    pub online: CachePart,
    /// 在线加密媒体区间。
    pub online_disk: CachePart,
    /// 兼容转码缓存：磁盘 `compat-cache/`，只统计 `.mp4`（与清理口径一致）。
    pub compat: CachePart,
    /// 封面/头像转码缓存：磁盘 `cover-cache/`。
    pub cover: CachePart,
}

impl CacheUsage {
    /// 磁盘占用合计。
    pub fn disk_bytes(&self) -> u64 {
        self.online_disk.bytes + self.compat.bytes + self.cover.bytes
    }
}

/// 当前占用总览。
pub fn usage() -> CacheUsage {
    let (files, bytes) = crate::play_service::online::status();
    let (disk_files, disk_bytes) = crate::play_service::online::disk::usage();
    CacheUsage {
        online: CachePart {
            files: files as u64,
            bytes,
        },
        online_disk: CachePart {
            files: disk_files,
            bytes: disk_bytes,
        },
        compat: dir_part(&compat_cache_dir(), Some("mp4")),
        cover: dir_part(&cover_cache_dir(), None),
    }
}

/// 一键清理的结果（前端直接消费）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheClearResult {
    /// 清掉的条目数（内存分条 + 磁盘文件）。
    pub items: u64,
    /// 释放的磁盘字节（内存流不在磁盘上，不计入）。
    pub bytes: u64,
}

/// 一键清空缓存。
pub fn clear() -> CacheClearResult {
    let before = usage();
    let online = crate::play_service::online::clear();
    let (disk_files, disk_bytes) = crate::play_service::online::disk::clear();
    let compat = crate::transcode_service::cache::clear();
    let cover = clear_dir(&cover_cache_dir());
    CacheClearResult {
        items: online as u64 + disk_files + compat as u64 + cover,
        bytes: disk_bytes + before.compat.bytes + before.cover.bytes,
    }
}

/// 目录占用；`ext` 给定时只统计该扩展名的文件（与清理口径一致）。
fn dir_part(dir: &std::path::Path, ext: Option<&str>) -> CachePart {
    let mut part = CachePart::default();
    let Ok(entries) = hongguo_store::paths::qa_read_dir(dir) else {
        return part;
    };
    for e in entries.flatten() {
        if !e.path().is_file() {
            continue;
        }
        if let Some(want) = ext
            && e.path().extension().and_then(|x| x.to_str()) != Some(want)
        {
            continue;
        }
        if let Ok(m) = e.metadata() {
            part.files += 1;
            part.bytes += m.len();
        }
    }
    part
}

/// 清空目录下全部文件，返回删掉的个数。目录不存在按空处理。
fn clear_dir(dir: &std::path::Path) -> u64 {
    let mut n = 0;
    if let Ok(entries) = hongguo_store::paths::qa_read_dir(dir) {
        for e in entries.flatten() {
            if hongguo_store::paths::qa_remove_file(&e.path()).is_ok() {
                n += 1;
            }
        }
    }
    n
}

/// 自动清理策略。TTL 与上限都独立生效：先按「最后修改时间超过 TTL」删，再按
/// 「占用超过上限」从最旧开始删到上限以内；两档都关（0）时什么都不做——用户没配就别动他的缓存。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AutoCleanPolicy {
    /// 磁盘缓存占用上限（字节）；0 = 不限。
    pub limit_bytes: u64,
    /// 缓存最长保留天数；0 = 关闭 TTL。
    pub ttl_days: u32,
}

impl AutoCleanPolicy {
    /// 策略是否完全关闭（两档都没有约束）。
    pub fn is_disabled(&self) -> bool {
        self.limit_bytes == 0 && self.ttl_days == 0
    }
}

/// 缓存文件的一行账（大小 + 最后修改时间）。
struct CacheFile {
    path: std::path::PathBuf,
    bytes: u64,
    modified: std::time::SystemTime,
}

/// 收集封面缓存的文件账。**只收封面/头像转码产物**（纯缓存，删了只影响下次显示要重新下载）；
/// 兼容转码产物不进这里——它是正在播的集的供数源，播到一半被删会断播，且另有按磁盘空闲的预算机制。
fn collect_files() -> Vec<CacheFile> {
    let mut out = Vec::new();
    let dir = cover_cache_dir();
    let Ok(entries) = hongguo_store::paths::qa_read_dir(&dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        let modified = meta.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        out.push(CacheFile {
            path,
            bytes: meta.len(),
            modified,
        });
    }
    out
}

/// 刚落盘不久的文件不删（分钟）：刚写下的封面很可能正要被页面加载，删了会让这一帧的封面变占位块。
const MIN_FILE_AGE_MINUTES: u64 = 10;

/// 按策略自动清理磁盘缓存（**只清封面/头像转码产物**）：TTL 过期即删、超上限从最旧开始删、
/// 刚落盘 10 分钟内不删、删不掉的文件（权限/占用）跳过不报错。返回值与手动 [`clear`] 同构。
pub fn auto_clean(policy: AutoCleanPolicy, now: std::time::SystemTime) -> CacheClearResult {
    if policy.is_disabled() {
        return CacheClearResult { items: 0, bytes: 0 };
    }
    let mut files = collect_files();
    let mut removed = 0u64;
    let mut freed = 0u64;
    let min_age = std::time::Duration::from_secs(MIN_FILE_AGE_MINUTES * 60);
    let age_of = |file: &CacheFile| -> std::time::Duration {
        now.duration_since(file.modified)
            .unwrap_or(std::time::Duration::ZERO)
    };
    files.retain(|file| age_of(file) >= min_age);

    if policy.ttl_days > 0 {
        let ttl = std::time::Duration::from_secs(u64::from(policy.ttl_days) * 24 * 60 * 60);
        let mut expired = Vec::new();
        let mut kept = Vec::new();
        for file in files {
            if age_of(&file) >= ttl {
                expired.push(file);
            } else {
                kept.push(file);
            }
        }
        for file in expired {
            if hongguo_store::paths::qa_remove_file(&file.path).is_ok() {
                removed += 1;
                freed += file.bytes;
            }
        }
        files = kept;
    }

    if policy.limit_bytes > 0 {
        let mut total: u64 = files.iter().map(|f| f.bytes).sum();
        if total > policy.limit_bytes {
            files.sort_by_key(|f| f.modified);
            for file in files {
                if total <= policy.limit_bytes {
                    break;
                }
                if hongguo_store::paths::qa_remove_file(&file.path).is_ok() {
                    removed += 1;
                    freed += file.bytes;
                    total = total.saturating_sub(file.bytes);
                }
            }
        }
    }

    if removed > 0 {
        log::info!(
            "[Cache] 自动清理：删掉 {removed} 个封面缓存文件、释放 {} 字节（上限 {} MB，TTL {} 天）",
            freed,
            policy.limit_bytes / 1024 / 1024,
            policy.ttl_days
        );
    }
    CacheClearResult {
        items: removed,
        bytes: freed,
    }
}

/// 自动清理周期任务：启动即跑一轮，之后每 `AUTO_CLEAN_INTERVAL` 一轮。
/// 策略每轮从设置里现读——用户改了上限/周期，下一轮就生效。
pub fn start_auto_clean_loop(state: crate::app_state::AppState) {
    crate::runtime::spawn(async move {
        loop {
            let policy = AutoCleanPolicy {
                limit_bytes: state.settings().cache_limit_bytes(),
                ttl_days: state.settings().cache_ttl_days,
            };
            if !policy.is_disabled() {
                // 删文件要碰文件系统，放阻塞池，不占异步运行时
                let _ = tokio::task::spawn_blocking(move || {
                    auto_clean(policy, std::time::SystemTime::now())
                })
                .await;
            }
            tokio::time::sleep(AUTO_CLEAN_INTERVAL).await;
        }
    });
}

/// 自动清理的轮询间隔：缓存是慢变量（天级），1 小时一轮足够及时。
const AUTO_CLEAN_INTERVAL: std::time::Duration = std::time::Duration::from_secs(60 * 60);
