//! 持久化。
//!
//! 沿用 Electron 版的做法：单 JSON 文件保存「设置 / 任务 / 剧集档案 / 播放进度 /
//! 合并记录」，不引入数据库，减少依赖与打包复杂度。

pub mod file;
pub mod paths;
pub mod recover;

use serde::{Deserialize, Serialize};

use crate::domain::model::{DownloadTask, MergeTask, PlaybackMap, Series, Settings};

/// 数据文件的完整内容。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DataStore {
    #[serde(default)]
    pub settings: Settings,
    #[serde(default)]
    pub tasks: Vec<DownloadTask>,
    #[serde(default)]
    pub series: Vec<Series>,
    #[serde(default)]
    pub playback: PlaybackMap,
    #[serde(default)]
    pub merge_tasks: Vec<MergeTask>,
}

impl DataStore {
    /// 构造空数据（尚未从磁盘加载时使用）。
    pub fn empty() -> Self {
        Self {
            settings: Settings::default(),
            tasks: Vec::new(),
            series: Vec::new(),
            playback: PlaybackMap::new(),
            merge_tasks: Vec::new(),
        }
    }

    /// 从磁盘加载。文件不存在返回空数据；解析失败先备份再用空数据。
    pub fn load(path: &std::path::Path) -> Self {
        if !path.exists() {
            return Self::empty();
        }
        match std::fs::read(path) {
            Ok(bytes) => match serde_json::from_slice::<DataStore>(&bytes) {
                Ok(store) => store,
                Err(e) => {
                    // 解析失败说明文件已损坏（写一半掉电、被外部工具改坏等）。
                    // 这里绝不能直接丢掉：先备份一份，用户还能手工捞回下载记录。
                    log::error!("[Store] 读取数据文件失败: {e}");
                    recover::backup_corrupt(path);
                    Self::empty()
                }
            },
            Err(e) => {
                log::error!("[Store] 读取数据文件失败: {e}");
                Self::empty()
            }
        }
    }

    /// 落盘（原子写）。
    pub fn save(&self, path: &std::path::Path) -> crate::error::AppResult<()> {
        let bytes = serde_json::to_vec_pretty(self)
            .map_err(|e| crate::error::AppError::StoreCorrupt(e.to_string()))?;
        file::write_atomic(path, &bytes)
    }

    /// 未被用户移除的剧集档案。
    pub fn visible_series(&self) -> Vec<&Series> {
        self.series.iter().filter(|s| !s.dismissed).collect()
    }

    /// 按 id 取剧集档案。
    pub fn series(&self, series_id: &str) -> Option<&Series> {
        self.series.iter().find(|s| s.series_id == series_id)
    }

    /// 登记或更新剧集档案。
    pub fn upsert_series(&mut self, incoming: Series) {
        match self
            .series
            .iter_mut()
            .find(|s| s.series_id == incoming.series_id)
        {
            Some(existing) => {
                // 保留用户已做的移除标记，避免重新拉取时被「复活」
                let dismissed = existing.dismissed;
                *existing = incoming;
                existing.dismissed = dismissed;
            }
            None => self.series.push(incoming),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("hg-store-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("data.json")
    }

    #[test]
    fn save_and_load_roundtrip() {
        let f = temp_file("roundtrip");
        let mut store = DataStore::empty();
        store.settings.max_concurrency = 8;
        store
            .tasks
            .push(DownloadTask::new("1", "剧", 1, "v1", "第一集"));
        store.series.push(Series {
            series_id: "1".into(),
            title: "剧".into(),
            ..Default::default()
        });

        store.save(&f).unwrap();
        let back = DataStore::load(&f);
        assert_eq!(store, back);
        let _ = std::fs::remove_dir_all(f.parent().unwrap());
    }

    #[test]
    fn load_missing_file_is_empty() {
        let f = temp_file("missing").with_file_name("nope.json");
        let store = DataStore::load(&f);
        assert!(store.tasks.is_empty());
        assert!(store.series.is_empty());
    }

    #[test]
    fn load_corrupt_file_backs_up_and_returns_empty() {
        let f = temp_file("corrupt");
        std::fs::write(&f, b"{ not valid json").unwrap();
        let store = DataStore::load(&f);
        assert!(store.tasks.is_empty());

        // 备份文件应已生成
        let dir = f.parent().unwrap();
        let backups: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.contains("corrupt-"))
            .collect();
        assert_eq!(backups.len(), 1, "应恰好生成一个损坏备份: {backups:?}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn upsert_preserves_dismissed_flag() {
        let mut store = DataStore::empty();
        store.upsert_series(Series {
            series_id: "1".into(),
            title: "剧".into(),
            ..Default::default()
        });
        store.series[0].dismissed = true;

        // 重新拉取不应复活已移除的剧
        store.upsert_series(Series {
            series_id: "1".into(),
            title: "剧（新标题）".into(),
            ..Default::default()
        });
        assert!(store.series[0].dismissed);
        assert_eq!(store.series[0].title, "剧（新标题）");
        assert_eq!(store.visible_series().len(), 0);
    }

    #[test]
    fn reads_electron_era_data_file_unchanged() {
        // Electron 版 data.json 的真实形状：设置/剧集/任务全是 snake_case。
        // 迁移时一个字段都不能丢——丢了用户的下载目录与下载记录会静默变回默认值。
        let json = r#"{
            "settings": {
                "download_dir": "D:\\dl",
                "naming": "TitleIndex",
                "max_concurrency": 5,
                "proxy": {"mode": {"mode": "manual"}, "url": "http://127.0.0.1:1080"},
                "compat_mode": true,
                "auto_delete_after_play": false,
                "auto_next_episode": true,
                "theme": "dark"
            },
            "tasks": [{
                "id": "t1", "series_id": "1", "series_title": "剧", "vid_index": 2,
                "vid": "v2", "ep_title": "第二集", "file_path": "a.mp4", "temp_path": "",
                "status": "completed", "downloaded": 10, "total": 10, "error": "",
                "created_at": 1, "updated_at": 2
            }],
            "series": [{
                "series_id": "1", "title": "剧", "cover": "c", "episode_count": 77,
                "tags": ["爱情"], "dismissed": false,
                "episodes": [{"vid_index": 1, "vid": "v1", "title": "第1集", "file_stem": "s001"}]
            }],
            "playback": {},
            "merge_tasks": []
        }"#;

        let store: DataStore = serde_json::from_str(json).expect("旧版 data.json 应能直接读");
        assert_eq!(store.settings.download_dir, "D:\\dl");
        assert_eq!(store.settings.max_concurrency, 5);
        assert_eq!(store.settings.theme, "dark");
        assert_eq!(
            store.settings.proxy.mode,
            crate::domain::model::ProxyMode::Manual
        );
        assert_eq!(store.tasks[0].series_id, "1");
        assert_eq!(store.tasks[0].vid_index, 2);
        assert_eq!(store.tasks[0].file_path, "a.mp4");
        assert_eq!(store.series[0].episode_count, 77);
        assert_eq!(store.series[0].episodes[0].vid_index, 1);
        assert_eq!(store.series[0].episodes[0].file_stem, "s001");
    }
}
