//! 分集列表：调接口 + 解析。
//!
//! 分集在响应里的位置是固定的：`data[series_id].video_data.video_list[]`。
//! 不要用「递归找第一个数组」的办法——`video_data` 下面还有 `celebrities`、
//! `abstract_tags` 等一堆数组，猜出来的必然是错的，而且不会报错。

use serde_json::Value;

use crate::domain::model::Episode;
use crate::error::{AppError, AppResult};

/// 相关作品（同系列各季 + 同 IP 作品）。
///
/// 端点抓包实证（2026-10-07 hgplayer 1.1.5 详情页「相关推荐」tab 懒加载）：
/// `GET /reading/bookapi/plan/v?book_id=<series_id>&from=detail_page_more_related
/// &scene=10&...`（reading 族轻签名头，lq 域）。响应 `data[]` 是 cell 列表：
/// `cell_name:"相关作品"` 的 cell 里 `video_data[]` 每项带 `tag_info.text`
/// 角标（`第1季`/`同IP`/…）。
pub const PLAN_PATH: &str = "/reading/bookapi/plan/v";

/// 相关作品 / 猜你喜欢里的一条。
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelatedItem {
    pub series_id: String,
    pub title: String,
    pub cover: String,
    /// 角标文案（`第1季`/`同IP`…，服务端下发，无则空）
    pub tag: String,
    /// 评分（0 = 无分）
    pub score: f64,
    pub play_cnt: i64,
    /// 0 = 未上线（「即将上线」态）
    pub episode_cnt: u32,
    pub video_desc: String,
}

/// 详情页相关推荐 tab 的两块内容。
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelatedSeries {
    /// 相关作品·系列（同系列各季 + 同 IP）
    pub works: Vec<RelatedItem>,
    /// 猜你喜欢（plan 响应可能给空 cell，空时前端回落既有推荐源）
    pub guess: Vec<RelatedItem>,
}

/// 拉一部剧的相关作品·系列。
pub async fn fetch_related_series(
    series_id: &str,
    env: &super::client::ApiEnv,
) -> AppResult<RelatedSeries> {
    let biz_query: Vec<(String, String)> = [
        ("book_id", series_id),
        ("bookstore_tab", "0"),
        ("bookstore_tab_type", "0"),
        ("current_chapter_num", "0"),
        ("from", "detail_page_more_related"),
        ("is_horizontal_screen", "false"),
        ("limit", "0"),
        ("need_personal_recommend", "1"),
        ("offset", "0"),
        ("post_id", "0"),
        ("scene", "10"),
        ("total_chapter_num", "0"),
    ]
    .iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();

    let bytes = crate::domain::api::client::api_call_reading(
        crate::domain::api::danmaku::LQ_API_ORIGIN,
        PLAN_PATH,
        None,
        &biz_query,
        env,
    )
    .await?;

    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析响应失败: {e}")))?;
    parse_related_series(&value)
}

/// 解析 plan/v 响应为相关作品两块内容。
fn parse_related_series(value: &Value) -> AppResult<RelatedSeries> {
    if let Some(code) = value.get("code").and_then(Value::as_i64) {
        if code != 0 {
            let msg = value
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("未知错误");
            return Err(AppError::Media(format!("相关作品接口返回 {code}: {msg}")));
        }
    }

    let mut related = RelatedSeries::default();
    for cell in value
        .get("data")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
    {
        let cell_name = cell.get("cell_name").and_then(Value::as_str).unwrap_or("");
        let items = cell
            .get("video_data")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|raw| {
                        let series_id = raw.get("series_id").and_then(Value::as_str)?;
                        if series_id.is_empty() {
                            return None;
                        }
                        // 角标：tag_info.text；缺失且未上线时给「即将上线」
                        let tag = raw
                            .pointer("/tag_info/text")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string();
                        let episode_cnt = int_field(raw, "episode_cnt").max(0) as u32;
                        Some(RelatedItem {
                            series_id: series_id.to_string(),
                            title: str_field(raw, "title"),
                            cover: str_field(raw, "cover"),
                            tag: if tag.is_empty() && episode_cnt == 0 {
                                "即将上线".to_string()
                            } else {
                                tag
                            },
                            // score 线上是字符串形态（"8.0"）
                            score: raw
                                .get("score")
                                .and_then(Value::as_str)
                                .and_then(|s| s.parse::<f64>().ok())
                                .unwrap_or_else(|| {
                                    raw.get("score").and_then(Value::as_f64).unwrap_or(0.0)
                                }),
                            play_cnt: int_field(raw, "play_cnt"),
                            episode_cnt,
                            video_desc: str_field(raw, "video_desc"),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        match cell_name {
            "相关作品" => related.works = items,
            "猜你喜欢" => related.guess = items,
            _ => {}
        }
    }
    Ok(related)
}

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

/// 字符串字段（缺失给空串）。
fn str_field(v: &Value, key: &str) -> String {
    v.get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

/// 数字字段（线上形态可能是字符串数字，缺失给 0）。
fn int_field(v: &Value, key: &str) -> i64 {
    v.get(key)
        .map(|x| {
            x.as_i64()
                .or_else(|| x.as_str().and_then(|s| s.parse().ok()))
                .unwrap_or(0)
        })
        .unwrap_or(0)
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
        let v: Value = serde_json::json!({ "code": 101000, "message": "服务异常", "data": {} });
        let err = parse_episodes(&v).unwrap_err();
        assert!(err.to_string().contains("101000"));
    }

    /// 相关作品解析：cell_name 分流 + tag_info 角标 + score 字符串形态
    /// + 未上线兜底「即将上线」（2026-10-07 plan/v 抓包样本的形状）。
    #[test]
    fn parses_related_series_cells() {
        let v: Value = serde_json::json!({
            "code": 0,
            "data": [
                {
                    "cell_name": "相关作品",
                    "video_data": [
                        {
                            "series_id": "7664807332483697726",
                            "title": "一年升一境凡人修仙之外门杂役",
                            "cover": "https://cdn/c.jpg",
                            "score": "8.0",
                            "play_cnt": 5299902,
                            "episode_cnt": 125,
                            "video_desc": "只因凡品灵根",
                            "tag_info": { "text": "第1季" }
                        },
                        {
                            "series_id": "7664807332483697727",
                            "title": "第四季",
                            "episode_cnt": 0,
                            "play_cnt": 0
                        }
                    ]
                },
                { "cell_name": "猜你喜欢", "video_data": [
                    { "series_id": "999", "title": "别的剧", "episode_cnt": 10 }
                ] }
            ]
        });
        let rel = parse_related_series(&v).unwrap();
        assert_eq!(rel.works.len(), 2);
        assert_eq!(rel.works[0].tag, "第1季");
        assert_eq!(rel.works[0].score, 8.0, "score 字符串形态要能读");
        assert_eq!(rel.works[0].episode_cnt, 125);
        assert_eq!(rel.works[1].tag, "即将上线", "无角标且未上线给兜底文案");
        assert_eq!(rel.guess.len(), 1);
        assert_eq!(rel.guess[0].series_id, "999");
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

#[cfg(test)]
mod probe2 {
    use super::*;
    /// 找 preload 详情响应里的「相关作品·系列」字段（hgplayer 详情页同款）。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_detail_related_series() {
        let env = crate::domain::api::client::ApiEnv::anonymous(
            crate::domain::model::ProxyConfig::default(),
        );
        let payload = serde_json::to_vec(
            &crate::domain::api::params::detail_payload("7689382439004671038"),
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
        let node = &v["data"]["7689382439004671038"];
        let mut keys: Vec<_> = node.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        println!("[rel] data.<sid> keys: {keys:?}");
        let vd = &node["video_data"];
        let mut vkeys: Vec<_> = vd.as_object().map(|o| o.keys().cloned().collect()).unwrap_or_default();
        vkeys.sort();
        println!("[rel] video_data keys: {vkeys:?}");
        let s = serde_json::to_string(&v).unwrap();
        for key in ["relation", "relate_series", "series_list", "season", "同IP", "相关", "recommend"] {
            let n = s.matches(key).count();
            if n > 0 { println!("  [{key}] x{n}"); }
        }
    }
}

#[cfg(test)]
mod probe_related {
    use super::*;
    /// 相关作品·系列真连探测（2026-10-07 端点实装验证；多季剧才有多条）。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_related_series_live() {
        let env = crate::domain::api::client::ApiEnv::anonymous(
            crate::domain::model::ProxyConfig::default(),
        );
        let rel = fetch_related_series("7687961503718198334", &env).await.expect("相关作品");
        println!("[related] works {} 条, guess {} 条", rel.works.len(), rel.guess.len());
        for w in rel.works.iter().take(6) {
            println!("  [{}] {} score={} {}集 play={}", w.tag, w.title, w.score, w.episode_cnt, w.play_cnt);
        }
        assert!(!rel.works.is_empty(), "多季剧必有相关作品");
    }
}
