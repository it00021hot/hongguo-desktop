//! 旧版 data.json 的一次性迁移。
//!
//! 背景：2026-10 之前的应用把全部状态存进单个 data.json（Electron 时代
//! snake_case，之后 camelCase + alias 双读）。存储层切到 Turso 内嵌库后，
//! 首次启动要把旧文件完整搬进库，然后改名留底，用户零感知。
//!
//! 兼容面（一条都不能少，丢了用户的下载记录就静默消失）：
//! - Electron 旧档的 snake_case 键名：`LegacyData` 复用了模型的全部
//!   `#[serde(alias)]`，模型层测试已逐字段锁死；
//! - 解析失败：与旧版 `DataStore::load` 同口径——先备份 `data.json.corrupt-*`
//!   再用空库启动，不覆盖不吞文件；
//! - 迁移后改名失败（文件被占用等）：导入是幂等的（全部 INSERT OR REPLACE），
//!   下次启动重导一遍也不会出现重复行。

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::domain::model::{DownloadTask, MergeTask, PlaybackMap, Series, Settings};

/// 旧 data.json 的顶层结构。
///
/// 字段全部 `#[serde(default)]`：旧文件缺哪段就少搬哪段，不因字段
/// 缺失而整体失败（与 `DataStore::load` 的宽容口径一致）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LegacyData {
    #[serde(default)]
    pub settings: Option<Settings>,
    #[serde(default)]
    pub tasks: Vec<DownloadTask>,
    #[serde(default)]
    pub series: Vec<Series>,
    #[serde(default)]
    pub playback: PlaybackMap,
    #[serde(default)]
    pub merge_tasks: Vec<MergeTask>,
}

/// 迁移结果，用于启动日志。
#[derive(Debug, PartialEq)]
pub enum MigrationOutcome {
    /// 没有旧文件（新装或已迁移过）
    NoLegacyFile,
    /// 旧文件解析失败，已备份后按空库处理
    CorruptBackedUp,
    /// 迁移成功：搬了多少行、改名是否留底成功
    Migrated {
        tasks: usize,
        series: usize,
        playback: usize,
        merges: usize,
        renamed: bool,
    },
}

/// 迁移入口。`json_path` 通常是 [`crate::store::paths::data_file`]。
pub fn run(
    store: &super::bridge::Store,
    json_path: &Path,
) -> crate::error::AppResult<MigrationOutcome> {
    if !json_path.exists() {
        return Ok(MigrationOutcome::NoLegacyFile);
    }
    let bytes = match std::fs::read(json_path) {
        Ok(b) => b,
        Err(e) => {
            // 读不出来（权限/占用）：不动它，留待下次启动再试。
            // 此时按空库继续，用户至少能改设置——比直接拒绝启动好。
            log::error!("[Migrate] 读取旧数据文件失败: {e}");
            return Ok(MigrationOutcome::NoLegacyFile);
        }
    };
    let legacy: LegacyData = match serde_json::from_slice(&bytes) {
        Ok(d) => d,
        Err(e) => {
            log::error!("[Migrate] 旧数据文件解析失败: {e}");
            super::recover::backup_corrupt(json_path);
            return Ok(MigrationOutcome::CorruptBackedUp);
        }
    };

    let counts = (
        legacy.tasks.len(),
        legacy.series.len(),
        legacy.merge_tasks.len(),
    );
    let playback_rows: usize = legacy.playback.values().map(|m| m.len()).sum();
    store.import_legacy(&legacy)?;

    // 导入成功后改名留底。失败不回滚：导入幂等，重导无害。
    let renamed = rename_to_migrated(json_path);
    log::info!(
        "[Migrate] data.json → hongguo.db 完成：任务 {} 剧 {} 进度 {} 合并 {}（留底: {}）",
        counts.0,
        counts.1,
        playback_rows,
        counts.2,
        renamed
    );
    Ok(MigrationOutcome::Migrated {
        tasks: counts.0,
        series: counts.1,
        playback: playback_rows,
        merges: counts.2,
        renamed,
    })
}

/// 改名 `data.json` → `data.json.migrated`。已是这个名字（或刚被并发进程
/// 抢先改过）时按成功处理。
fn rename_to_migrated(json_path: &Path) -> bool {
    let target = json_path.with_extension("json.migrated");
    if target.exists() {
        return true;
    }
    match std::fs::rename(json_path, &target) {
        Ok(()) => true,
        Err(e) => {
            log::warn!("[Migrate] 旧文件改名留底失败（下次启动会重导，无害）: {e}");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_json(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("hg-migrate-{tag}-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        dir.join("data.json")
    }

    #[test]
    fn migrates_electron_era_file_and_renames() {
        let path = temp_json("electron");
        // 这份就是 store::mod 旧测试里的 Electron 真实形状（snake_case）
        std::fs::write(
            &path,
            r#"{
                "settings": {
                    "download_dir": "D:\\dl", "naming": "TitleIndex", "max_concurrency": 5,
                    "proxy": {"mode": {"mode": "manual"}, "url": "http://127.0.0.1:1080"},
                    "auto_delete_after_play": false, "auto_next_episode": true, "theme": "dark"
                },
                "tasks": [{
                    "id": "t1", "series_id": "1", "series_title": "剧", "vid_index": 2,
                    "vid": "v2", "ep_title": "第二集", "file_path": "a.mp4", "temp_path": "",
                    "status": "completed", "downloaded": 10, "total": 10, "error": "",
                    "created_at": 1, "updated_at": 2
                }],
                "series": [{
                    "series_id": "1", "title": "剧", "cover": "c", "episode_count": 77,
                    "episodes": [{"vid_index": 1, "vid": "v1", "title": "第1集", "file_stem": "s001"}]
                }],
                "playback": {"1": {"2": {"current_time": 15.5, "duration": 300.0, "updated_at": 42}}},
                "merge_tasks": []
            }"#,
        )
        .unwrap();

        let store = super::super::bridge::Store::open_memory().unwrap();
        let outcome = run(&store, &path).unwrap();
        match outcome {
            MigrationOutcome::Migrated {
                tasks,
                series,
                playback,
                merges,
                renamed,
            } => {
                assert_eq!((tasks, series, playback, merges), (1, 1, 1, 0));
                assert!(renamed, "迁移后必须改名留底");
            }
            other => panic!("应当迁移成功: {other:?}"),
        }

        // 内容核对：设置、任务、进度都进库了
        let settings = store.settings().unwrap().expect("设置应已导入");
        assert_eq!(settings.download_dir, "D:\\dl");
        assert_eq!(settings.max_concurrency, 5);
        assert_eq!(store.tasks().unwrap().len(), 1);
        assert_eq!(
            store
                .playback_position("1", 2)
                .unwrap()
                .map(|p| p.current_time),
            Some(15.5)
        );
        assert!(store.series_by_id("1").unwrap().is_some());

        // 旧文件已改名
        assert!(!path.exists());
        assert!(path.with_extension("json.migrated").exists());
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn missing_file_is_not_an_error() {
        let store = super::super::bridge::Store::open_memory().unwrap();
        let path = temp_json("missing").with_file_name("nope.json");
        assert_eq!(run(&store, &path).unwrap(), MigrationOutcome::NoLegacyFile);
    }

    #[test]
    fn corrupt_file_is_backed_up_and_ignored() {
        let path = temp_json("corrupt");
        std::fs::write(&path, b"{ not valid json").unwrap();
        let store = super::super::bridge::Store::open_memory().unwrap();
        assert_eq!(
            run(&store, &path).unwrap(),
            MigrationOutcome::CorruptBackedUp
        );
        assert!(store.tasks().unwrap().is_empty(), "坏文件不导入任何东西");

        let dir = path.parent().unwrap();
        let backups = std::fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains("corrupt-"))
            .count();
        assert_eq!(backups, 1, "坏文件要留一份备份供手工捞回");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn running_migration_again_is_idempotent() {
        let path = temp_json("twice");
        std::fs::write(&path, r#"{"series": [{"series_id": "9", "title": "t"}]}"#).unwrap();
        let store = super::super::bridge::Store::open_memory().unwrap();
        run(&store, &path).unwrap();
        // 手工把文件改回原名，模拟「上次改名失败」
        std::fs::rename(path.with_extension("json.migrated"), &path).unwrap();
        run(&store, &path).unwrap();
        assert_eq!(store.series_all().unwrap().len(), 1, "重导不得翻倍");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }
}
