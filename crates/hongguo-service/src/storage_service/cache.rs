//! 缓存占用统计与一键清理。
//!
//! 三处缓存在此汇成一个入口：在线内存流（`play_service::online` 背后的
//! `StreamCache`）、兼容转码产物（`transcode_service::cache` 落
//! `compat-cache/`）、封面/头像转码产物（`protocol::cover` 落
//! `cover-cache/`）。设置页此前只暴露前两处的清理按钮且都不报大小，
//! cover-cache 更是从来没有清理入口——封面图片只进不出。

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

/// 三处缓存的占用总览（前端直接消费）。
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheUsage {
    /// 在线流缓存：在内存里，不占磁盘。
    pub online: CachePart,
    /// 兼容转码缓存：磁盘 `compat-cache/`，只统计 `.mp4`（与清理口径一致）。
    pub compat: CachePart,
    /// 封面/头像转码缓存：磁盘 `cover-cache/`。
    pub cover: CachePart,
}

impl CacheUsage {
    /// 磁盘占用合计。在线流活在内存里，重启即消失，不混进磁盘数字。
    pub fn disk_bytes(&self) -> u64 {
        self.compat.bytes + self.cover.bytes
    }
}

/// 当前占用总览。
pub fn usage() -> CacheUsage {
    let (files, bytes) = crate::play_service::online::status();
    CacheUsage {
        online: CachePart {
            files: files as u64,
            bytes,
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

/// 一键清空三处缓存。
pub fn clear() -> CacheClearResult {
    let before = usage();
    let online = crate::play_service::online::clear();
    let compat = crate::transcode_service::cache::clear();
    let cover = clear_dir(&cover_cache_dir());
    CacheClearResult {
        items: online as u64 + compat as u64 + cover,
        bytes: before.disk_bytes(),
    }
}

/// 目录占用；`ext` 给定时只统计该扩展名的文件（与清理口径一致，统计
/// 与清理对不上就是「清完数字不变」的假清理）。
fn dir_part(dir: &std::path::Path, ext: Option<&str>) -> CachePart {
    let mut part = CachePart::default();
    let Ok(entries) = std::fs::read_dir(dir) else {
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
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            if std::fs::remove_file(e.path()).is_ok() {
                n += 1;
            }
        }
    }
    n
}

// —— 自动清理（TTL + 占用上限）——

/// 自动清理策略。TTL 与上限都独立生效：先按「最后修改时间超过 TTL」删，
/// 再按「占用超过上限」从最旧开始删到上限以内。
///
/// 两档都关（0）时 [`auto_clean`] 什么都不做——用户没配就别动他的缓存。
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

/// 收集封面缓存的文件账。
///
/// **只收封面/头像转码产物**（`cover-cache/`）：那是纯缓存，删了只影响
/// 下次显示要重新下载一次。
///
/// **兼容转码产物（`compat-cache/`）不进这里**：它由
/// `transcode_service::cache::trim()` 按磁盘空闲空间自己管预算（2–16GB），
/// 已有淘汰机制；而它的文件是**正在播的集**的供数源——`hongguo-local://`
/// 每次 range 请求都重新开文件（见 `hongguo-protocol::local::serve`），
/// 播到一半被删就会断这一集。自研引擎 + 平台解不了 HEVC 时才走这条路，
/// 但没必要为清缓存冒断播的风险。
fn collect_files() -> Vec<CacheFile> {
    let mut out = Vec::new();
    let dir = cover_cache_dir();
    let Ok(entries) = std::fs::read_dir(&dir) else {
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

/// 刚落盘不久的文件不删（分钟）。刚写下的封面很可能正要被页面加载
/// （`ensure_sized_cached` 写 `.tmp` 再 rename），删了会让这一帧的封面
/// 变成占位块。十分钟足够让所有在途加载走完。
const MIN_FILE_AGE_MINUTES: u64 = 10;

/// 按策略自动清理磁盘缓存（**只清封面/头像转码产物**）。
///
/// - TTL：最后修改时间早于 `ttl_days` 天的文件被删除；
/// - 上限：剩余占用仍超 `limit_bytes` 时，从最旧的文件开始删到上限以内；
/// - 刚落盘 10 分钟内的文件一律不删（多半正要被页面加载）；
/// - 删不掉的文件（权限、占用）跳过，不报错。
///
/// 兼容转码产物（`compat-cache/`）**不在范围内**：它是正在播的集的供数源
/// （`hongguo-local://` 每次 range 请求重新开文件），播到一半被删会断播；
/// 它另有按磁盘空闲的预算机制（`transcode_service::cache::trim`）。
///
/// 返回值与手动 [`clear`] 同构：删掉多少个文件、释放多少字节。
pub fn auto_clean(policy: AutoCleanPolicy, now: std::time::SystemTime) -> CacheClearResult {
    if policy.is_disabled() {
        return CacheClearResult {
            items: 0,
            bytes: 0,
        };
    }
    let mut files = collect_files();
    let mut removed = 0u64;
    let mut freed = 0u64;
    // 刚落盘不久的文件一律不删（两段都算）：它多半正要被页面加载。
    let min_age = std::time::Duration::from_secs(MIN_FILE_AGE_MINUTES * 60);
    let age_of = |file: &CacheFile| -> std::time::Duration {
        now.duration_since(file.modified)
            .unwrap_or(std::time::Duration::ZERO)
    };
    files.retain(|file| age_of(file) >= min_age);

    // 1) TTL：过期即删
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
            if std::fs::remove_file(&file.path).is_ok() {
                removed += 1;
                freed += file.bytes;
            }
        }
        files = kept;
    }

    // 2) 上限：超了就从最旧的开始删
    if policy.limit_bytes > 0 {
        let mut total: u64 = files.iter().map(|f| f.bytes).sum();
        if total > policy.limit_bytes {
            files.sort_by_key(|f| f.modified);
            for file in files {
                if total <= policy.limit_bytes {
                    break;
                }
                if std::fs::remove_file(&file.path).is_ok() {
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

/// 自动清理周期任务：启动即跑一轮，之后每 [`AUTO_CLEAN_INTERVAL`] 一轮。
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
                let _ = tokio::task::spawn_blocking(move || auto_clean(policy, std::time::SystemTime::now()))
                    .await;
            }
            tokio::time::sleep(AUTO_CLEAN_INTERVAL).await;
        }
    });
}

/// 自动清理的轮询间隔。缓存是慢变量（天级），1 小时一轮足够及时，
/// 又不至于每小时扫一遍目录。
const AUTO_CLEAN_INTERVAL: std::time::Duration = std::time::Duration::from_secs(60 * 60);


#[cfg(test)]
mod tests {
    use super::*;

    /// 落盘用例把数据目录重定向到临时目录（RAII + 全局锁），不然统计和
    /// 清理扫到的是用户真实的 %APPDATA%。
    #[test]
    fn usage_and_clear_cover_both_disk_caches() {
        let dir = std::env::temp_dir().join(format!("hg-cache-stats-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let _scope = hongguo_store::paths::ScopedDataDir::new(&dir);

        let cover = cover_cache_dir();
        std::fs::create_dir_all(&cover).unwrap();
        std::fs::write(cover.join("a.jpg"), vec![0u8; 100]).unwrap();
        std::fs::write(cover.join("b.png"), vec![0u8; 50]).unwrap();

        let compat = compat_cache_dir();
        std::fs::create_dir_all(&compat).unwrap();
        std::fs::write(compat.join("x_001.mp4"), vec![0u8; 200]).unwrap();

        let u = usage();
        assert_eq!(u.cover.files, 2, "cover-cache 全部文件都算（jpg/png 混放）");
        assert_eq!(u.cover.bytes, 150);
        assert_eq!(u.compat.files, 1);
        assert_eq!(u.compat.bytes, 200);
        assert_eq!(u.disk_bytes(), 350);

        let r = clear();
        assert_eq!(r.items, 3, "内存流测试里为空，磁盘上 3 个文件");
        assert_eq!(r.bytes, 350);

        let after = usage();
        assert_eq!(after.cover.files + after.compat.files, 0, "清完应归零");
        assert_eq!(after.disk_bytes(), 0);
    }

    #[test]
    fn missing_dirs_read_as_empty() {
        let dir = std::env::temp_dir().join(format!("hg-cache-empty-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let _scope = hongguo_store::paths::ScopedDataDir::new(&dir);

        let u = usage();
        assert_eq!(u.compat.files, 0);
        assert_eq!(u.cover.files, 0);
        // 清理空缓存也是一次成功操作
        assert_eq!(clear().items, 0);
    }

    /// 写一个缓存文件；`age_days` 表示「它有多老」——测试里通过把
    /// `now` 往前推来表达过期，不去改文件的 mtime（免得为一个用例引
    /// 第三方时间戳依赖）。
    fn write_aged(dir: &std::path::Path, name: &str, bytes: usize) {
        std::fs::write(dir.join(name), vec![0u8; bytes]).unwrap();
    }

    /// 「现在」往前推 n 天。
    fn days_ago(days: u64) -> std::time::SystemTime {
        std::time::SystemTime::now() - std::time::Duration::from_secs(days * 24 * 60 * 60)
    }

    /// 「现在」往后推 n 天（表达「文件已经这么老」）。
    fn days_from_now(days: u64) -> std::time::SystemTime {
        std::time::SystemTime::now() + std::time::Duration::from_secs(days * 24 * 60 * 60)
    }

    /// TTL：文件比保留期限老就被删，还新鲜就留下。
    ///
    /// 参照时间 `now` 往前推来表达「文件有多老」（不去改文件 mtime，免得
    /// 为一个用例引第三方时间戳依赖）：同一批文件先用「才 1 天」的参照时间
    /// 判定（未过期，应留下），再用「已经 10 天」的参照时间判定（过期，应
    /// 删掉）。
    #[test]
    fn auto_clean_removes_files_older_than_ttl() {
        let dir = std::env::temp_dir().join(format!("hg-cache-ttl-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let _scope = hongguo_store::paths::ScopedDataDir::new(&dir);
        let cover = cover_cache_dir();
        std::fs::create_dir_all(&cover).unwrap();
        write_aged(&cover, "a.jpg", 100);
        write_aged(&cover, "b.jpg", 200);
        let policy = AutoCleanPolicy {
            limit_bytes: 0,
            ttl_days: 3,
        };

        // 参照时间只比文件早 1 天：未过期
        let kept = auto_clean(policy, days_ago(1));
        assert_eq!(kept.items, 0, "未过期不该删");
        assert!(cover.join("a.jpg").exists());
        assert!(cover.join("b.jpg").exists());

        // 参照时间比文件晚 10 天：过期
        let expired = auto_clean(policy, days_from_now(10));
        assert_eq!(expired.items, 2);
        assert_eq!(expired.bytes, 300);
        assert!(!cover.join("a.jpg").exists());
        assert!(!cover.join("b.jpg").exists());
    }

    /// 上限：超了从最旧开始删到上限以内（写入间隔给不同的 mtime）。
    ///
    /// 参照时间往后推一天——刚落盘 10 分钟内的文件有护栏不删（见
    /// `auto_clean_keeps_fresh_files`）。
    #[test]
    fn auto_clean_trims_to_limit_oldest_first() {
        let dir = std::env::temp_dir().join(format!("hg-cache-cap-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let _scope = hongguo_store::paths::ScopedDataDir::new(&dir);
        let cover = cover_cache_dir();
        std::fs::create_dir_all(&cover).unwrap();
        write_aged(&cover, "a.jpg", 100);
        std::thread::sleep(std::time::Duration::from_millis(20));
        write_aged(&cover, "b.jpg", 100);
        std::thread::sleep(std::time::Duration::from_millis(20));
        write_aged(&cover, "c.jpg", 100);

        let result = auto_clean(
            AutoCleanPolicy {
                limit_bytes: 250,
                ttl_days: 0,
            },
            days_from_now(1),
        );
        assert_eq!(result.items, 1, "删到 200 ≤ 250 即可停");
        assert_eq!(result.bytes, 100);
        assert!(!cover.join("a.jpg").exists(), "最旧的先走");
        assert!(cover.join("b.jpg").exists());
        assert!(cover.join("c.jpg").exists());
    }

    /// 两档都关 = 不动用户的数据。
    #[test]
    fn auto_clean_disabled_touches_nothing() {
        let dir = std::env::temp_dir().join(format!("hg-cache-off-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let _scope = hongguo_store::paths::ScopedDataDir::new(&dir);
        let cover = cover_cache_dir();
        std::fs::create_dir_all(&cover).unwrap();
        write_aged(&cover, "a.jpg", 100);

        let result = auto_clean(AutoCleanPolicy::default(), days_ago(400));
        assert_eq!(result.items, 0);
        assert!(cover.join("a.jpg").exists());
    }

    /// 刚落盘不久的文件不删——哪怕超上限也不删：它多半正要被页面加载
    /// （写 `.tmp` 再 rename 刚落盘），删了会让这一帧的封面变占位块。
    #[test]
    fn auto_clean_keeps_fresh_files() {
        let dir = std::env::temp_dir().join(format!("hg-cache-fresh-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let _scope = hongguo_store::paths::ScopedDataDir::new(&dir);
        let cover = cover_cache_dir();
        std::fs::create_dir_all(&cover).unwrap();
        write_aged(&cover, "a.jpg", 100);

        // 上限 0 字节（超得不能再超），但文件才落盘几秒
        let result = auto_clean(
            AutoCleanPolicy {
                limit_bytes: 1,
                ttl_days: 0,
            },
            std::time::SystemTime::now(),
        );
        assert_eq!(result.items, 0, "刚落盘的文件不删");
        assert!(cover.join("a.jpg").exists());
    }

    /// 兼容转码产物（正在播的集的供数源）不在自动清理范围内——它由
    /// `transcode_service::cache::trim()` 按磁盘空闲自己管预算。
    #[test]
    fn auto_clean_leaves_compat_cache_alone() {
        let dir = std::env::temp_dir().join(format!("hg-cache-compat-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let _scope = hongguo_store::paths::ScopedDataDir::new(&dir);
        let compat = compat_cache_dir();
        std::fs::create_dir_all(&compat).unwrap();
        write_aged(&compat, "series_001.mp4", 500);

        let result = auto_clean(
            AutoCleanPolicy {
                limit_bytes: 1,
                ttl_days: 1,
            },
            days_from_now(30),
        );
        assert_eq!(result.items, 0);
        assert!(
            compat.join("series_001.mp4").exists(),
            "兼容转码产物不能被自动清理删掉（会断正在播的集）"
        );
    }
}
