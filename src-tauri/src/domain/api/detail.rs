//! 分集列表：调接口 + 解析。
//!
//! 分集在响应里的位置是固定的：`data[series_id].video_data.video_list[]`。
//! 不要用「递归找第一个数组」的办法——`video_data` 下面还有 `celebrities`、
//! `abstract_tags` 等一堆数组，猜出来的必然是错的，而且不会报错。

use serde_json::Value;

use crate::domain::model::Episode;
use crate::error::{AppError, AppResult};

/// 一部剧的元信息 + 分集。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EpisodeList {
    /// 平台侧的剧集 id
    pub series_id: String,
    /// 剧名
    pub title: String,
    /// 封面
    pub cover: String,
    /// 分集，按 `vid_index` 升序
    pub episodes: Vec<Episode>,
    /// 全剧收藏数（video_data.followed_cnt，右栏「☆ N」数据源）
    pub followed_cnt: i64,
}

/// 取一部剧的分集列表。
pub async fn fetch_episode_list(
    series_id: &str,
    env: &super::client::ApiEnv,
) -> AppResult<EpisodeList> {
    let payload = serde_json::to_vec(&crate::domain::api::params::detail_payload(series_id))
        .map_err(|e| AppError::Signer(e.to_string()))?;

    let bytes = crate::domain::api::client::api_call(
        crate::domain::api::params::DETAIL_PATH,
        Some(payload),
        env,
    )
    .await?;

    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析响应失败: {e}")))?;
    parse_episodes(&value)
}

/// 从 detail 响应里解析分集。
pub fn parse_episodes(response: &Value) -> AppResult<EpisodeList> {
    if let Some(code) = response.get("code").and_then(Value::as_i64) {
        if code != 0 {
            let msg = response
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("未知错误");
            return Err(AppError::Media(format!("详情接口返回 {code}: {msg}")));
        }
    }

    let data = response
        .get("data")
        .and_then(Value::as_object)
        .filter(|d| !d.is_empty())
        .ok_or_else(|| AppError::Media("响应里没有 data".into()))?;

    let (sid, node) = data.iter().next().expect("data 已确认非空");
    let video_data = node.get("video_data").cloned().unwrap_or(Value::Null);

    let list = video_data
        .get("video_list")
        .and_then(Value::as_array)
        .ok_or_else(|| AppError::Media("响应里没有 video_data.video_list".into()))?;

    let mut episodes: Vec<Episode> = list
        .iter()
        .filter_map(|item| {
            let vid = item.get("vid")?.as_str()?.to_string();
            Some(Episode {
                vid_index: item.get("vid_index").and_then(Value::as_u64).unwrap_or(0) as u32,
                vid,
                title: item
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                file_stem: String::new(),
                comment_count: item
                    .get("comment_count")
                    .and_then(Value::as_i64)
                    .unwrap_or(0),
                digg_count: item.get("digged_count").and_then(Value::as_i64).unwrap_or(0),
            })
        })
        .collect();

    if episodes.is_empty() {
        return Err(AppError::Media("分集列表为空".into()));
    }
    episodes.sort_by_key(|e| e.vid_index);

    let cover = pick(&video_data, &["series_cover", "cover_url"]);
    Ok(EpisodeList {
        series_id: sid.clone(),
        title: pick(&video_data, &["series_title"]),
        cover,
        episodes,
        followed_cnt: video_data
            .get("followed_cnt")
            .and_then(Value::as_i64)
            .unwrap_or(0),
    })
}

/// 按候选字段名取第一个非空字符串。
fn pick(value: &Value, keys: &[&str]) -> String {
    keys.iter()
        .find_map(|k| value.get(*k).and_then(Value::as_str))
        .unwrap_or_default()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// 实网样本：celebrities 等干扰数组排在 video_list 之前。
    fn real_response() -> Value {
        json!({
            "code": 0,
            "data": {
                "7687919221593885758": {
                    "video_data": {
                        "series_title": "二嫁有喜",
                        "series_cover": "https://cdn/cover.jpg",
                        "celebrities": [
                            { "nickname": "演员甲" },
                            { "nickname": "演员乙" }
                        ],
                        "abstract_tags": [],
                        "video_list": [
                            { "vid": "v2", "vid_index": 2, "title": "第二集" },
                            { "vid": "v1", "vid_index": 1, "title": "第一集" }
                        ]
                    }
                }
            }
        })
    }

    #[test]
    fn parses_real_shape_ignoring_decoy_arrays() {
        let list = parse_episodes(&real_response()).unwrap();
        assert_eq!(list.series_id, "7687919221593885758");
        assert_eq!(list.title, "二嫁有喜");
        assert_eq!(list.cover, "https://cdn/cover.jpg");
        assert_eq!(list.episodes.len(), 2, "不能把 celebrities 当成分集");
        assert_eq!(list.episodes[0].vid, "v1", "应按 vid_index 升序");
        assert_eq!(list.episodes[0].title, "第一集");
    }

    #[test]
    fn falls_back_to_cover_url() {
        let v = json!({
            "code": 0,
            "data": { "1": { "video_data": {
                "cover_url": "https://cdn/b.jpg",
                "video_list": [{ "vid": "a", "vid_index": 1 }]
            }}}
        });
        assert_eq!(parse_episodes(&v).unwrap().cover, "https://cdn/b.jpg");
    }

    #[test]
    fn missing_video_list_errors() {
        let v = json!({ "code": 0, "data": { "1": { "video_data": { "celebrities": [] } } } });
        let err = parse_episodes(&v).unwrap_err();
        assert!(err.to_string().contains("video_data.video_list"));
    }

    #[test]
    fn empty_video_list_errors() {
        let v = json!({ "code": 0, "data": { "1": { "video_data": { "video_list": [] } } } });
        assert!(parse_episodes(&v).is_err(), "空列表不能当成正常结果");
    }

    #[test]
    fn business_code_errors() {
        let v = json!({ "code": 101000, "message": "服务异常", "data": {} });
        let err = parse_episodes(&v).unwrap_err();
        assert!(err.to_string().contains("101000"));
    }

    #[test]
    fn items_without_vid_are_skipped() {
        let v = json!({ "code": 0, "data": { "1": { "video_data": { "video_list": [
            { "vid": "a", "vid_index": 1 }, { "title": "没有 vid" }, { "vid": "c", "vid_index": 3 }
        ]}}}});
        let list = parse_episodes(&v).unwrap();
        assert_eq!(list.episodes.len(), 2);
        assert_eq!(list.episodes[1].vid, "c");
    }

    #[test]
    fn missing_vid_index_sorts_first() {
        let v = json!({ "code": 0, "data": { "1": { "video_data": { "video_list": [
            { "vid": "b", "vid_index": 5 }, { "vid": "a" }
        ]}}}});
        let list = parse_episodes(&v).unwrap();
        assert_eq!(list.episodes[0].vid, "a", "缺 vid_index 视作 0，排最前");
    }
}

#[cfg(test)]
mod probe {
    use super::*;

    /// 详情响应里的公开计数盘点（信息流 landpage 的 comment_count 恒 0、
    /// 无点赞/收藏数——hgplayer 右栏计数需要一个真实来源）。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_detail_counts() {
        let env = crate::domain::api::client::ApiEnv::anonymous(
            crate::domain::model::ProxyConfig::default(),
        );
        let payload = serde_json::to_vec(
            &crate::domain::api::params::detail_payload("7690883800057777177"),
        )
        .unwrap();
        let bytes = crate::domain::api::client::api_call(
            crate::domain::api::params::DETAIL_PATH,
            Some(payload),
            &env,
        )
        .await
        .expect("detail");
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        let series = &v["data"]["7690883800057777177"];
        let first_ep = &series["video_data"]["video_list"][0];
        let keys: Vec<String> = first_ep
            .as_object()
            .map(|o| o.keys().cloned().collect())
            .unwrap_or_default();
        let count_keys: Vec<&String> =
            keys.iter().filter(|k| k.contains("count") || k.contains("cnt")).collect();
        println!("[detail-counts] 首集计数类键: {count_keys:?}");
        for k in count_keys {
            println!("  {k} = {}", first_ep[k]);
        }
        // followed_cnt 的路径定位（候选指针逐个试）
        for ptr in [
            "/video_data/followed_cnt",
            "/video_data/video_detail/followed_cnt",
            "/followed_cnt",
        ] {
            if let Some(v) = series.pointer(ptr) {
                println!("  followed_cnt 路径: {ptr} = {v}");
            }
        }
        let s = serde_json::to_string(series).unwrap_or_default();
        for key in ["digged_count", "followed_cnt", "comment_count", "play_cnt"] {
            let n = s.matches(&format!("\"{key}\"")).count();
            println!("  响应中 \"{key}\" 出现 {n} 次");
        }
    }
}
