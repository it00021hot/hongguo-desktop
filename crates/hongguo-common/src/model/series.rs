//! 剧集领域模型。
//!
//! 字段一律 camelCase：落库整行存 JSON 列、走 IPC 都用同一形态
//! （前端 `schema.ts` 同步声明 camelCase）。

use serde::{Deserialize, Serialize};

/// 一部短剧。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Series {
    pub series_id: String,
    pub title: String,
    #[serde(default)]
    pub cover: String,
    #[serde(default)]
    pub episode_count: u32,
    /// 全剧收藏数（detail 响应 video_data.followed_cnt，右栏「☆ N」数据源；
    /// 旧档案缓存缺省 0 = 不显示）
    #[serde(default)]
    pub followed_cnt: i64,
    /// 剧的条目类型（官方 content_type：漫剧 1004 / 真人剧 1 / 书城条目
    /// 1001…）。真播批量分组 key 与单集请求 content_type 的持久来源——
    /// 起播时整剧各集按它注册进取流注册表；0 = 未知（旧档案缓存），
    /// 取流走单集占位值（服务端对该字段容错）。
    #[serde(default)]
    pub content_type: i64,
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
    pub vid_index: u32,
    /// 平台侧的 vid
    #[serde(default)]
    pub vid: String,
    #[serde(default)]
    pub title: String,
    /// 落盘后的文件名（不含扩展名）
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
        // 库里的旧 JSON 行没有 dismissed 字段（该列后加）：default 补 false
        let json = r#"{"seriesId":"1","title":"t"}"#;
        let s: Series = serde_json::from_str(json).unwrap();
        assert!(!s.dismissed);
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
