//! 剧集领域模型。
//!
//! ⚠️ 这两个结构既落盘（`data.json`，沿用 Electron 版的 snake_case 键名以便
//! 平滑迁移）又走 IPC（前端 `schema.ts` 声明 camelCase）。所以字段一律
//! `rename` 成 camelCase 发出，同时用 `alias` 兼容旧的 snake_case 落盘键。

use serde::{Deserialize, Serialize};

/// 一部短剧。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Series {
    #[serde(alias = "series_id")]
    pub series_id: String,
    pub title: String,
    #[serde(default)]
    pub cover: String,
    #[serde(default, alias = "episode_count")]
    pub episode_count: u32,
    #[serde(default)]
    pub tags: Vec<String>,
    /// 完整分集（拉取后写入）
    #[serde(default)]
    pub episodes: Vec<Episode>,
    /// 用户主动从列表移除的标记。移除列表与删除文件是两个独立操作。
    #[serde(default)]
    pub dismissed: bool,
}

/// 一集。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Episode {
    /// 集号，从 1 开始
    #[serde(alias = "vid_index")]
    pub vid_index: u32,
    /// 平台侧的 vid
    #[serde(default)]
    pub vid: String,
    #[serde(default)]
    pub title: String,
    /// 落盘后的文件名（不含扩展名）
    #[serde(alias = "file_stem")]
    pub file_stem: String,
    /// 该集评论数（detail 响应公开计数，右栏「💬 N」数据源；旧档案缓存缺省 0）
    #[serde(default)]
    pub comment_count: i64,
    /// 该集点赞数（detail 响应公开计数，右栏「♥ N」数据源）
    #[serde(default)]
    pub digg_count: i64,
    /// 该集时长（秒；detail 响应 video_list[].duration，详情页选集格展示用，
    /// 旧档案缓存缺省 0）
    #[serde(default)]
    pub duration: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn series_roundtrips_through_json() {
        let s = Series {
            series_id: "123456".into(),
            title: "测试剧".into(),
            episodes: vec![Episode {
                vid_index: 1,
                vid: "v1".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: Series = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn old_data_without_dismissed_still_parses() {
        // 兼容旧版 data.json：没有 dismissed 字段
        let json = r#"{"series_id":"1","title":"t"}"#;
        let s: Series = serde_json::from_str(json).unwrap();
        assert!(!s.dismissed);
    }

    #[test]
    fn reads_legacy_snake_case_data_file() {
        // Electron 版落盘就是 snake_case，迁移时必须能直接读
        let json = r#"{"series_id":"1","title":"剧","episode_count":77,"episodes":[{"vid_index":1,"vid":"v1","title":"第1集","file_stem":"s001"}]}"#;
        let s: Series = serde_json::from_str(json).unwrap();
        assert_eq!(s.series_id, "1");
        assert_eq!(s.episode_count, 77);
        assert_eq!(s.episodes[0].vid_index, 1);
        assert_eq!(s.episodes[0].file_stem, "s001");
    }

    #[test]
    fn goes_out_as_camel_case_for_ipc() {
        // 前端 seriesSchema / episodeSchema 声明的是 camelCase
        let s = Series {
            series_id: "1".into(),
            title: "剧".into(),
            episode_count: 77,
            episodes: vec![Episode {
                vid_index: 1,
                vid: "v1".into(),
                file_stem: "s001".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["seriesId"], "1");
        assert_eq!(v["episodeCount"], 77);
        assert_eq!(v["episodes"][0]["vidIndex"], 1);
        assert_eq!(v["episodes"][0]["fileStem"], "s001");
        assert!(v.get("series_id").is_none());
    }

    #[test]
    fn reads_camel_case_payload_back() {
        // resolve_series 的返回值要能通过前端同一份 schema 的往返
        let json = r#"{"seriesId":"1","title":"剧","cover":"c","episodeCount":2,"tags":["爱情"],"episodes":[{"vidIndex":1,"vid":"v","title":"t","fileStem":"f"}],"dismissed":false}"#;
        let s: Series = serde_json::from_str(json).unwrap();
        assert_eq!(s.episodes[0].vid_index, 1);
        assert_eq!(s.episodes[0].file_stem, "f");
    }
}
