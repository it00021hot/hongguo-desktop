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
///
/// 服务端对 plan/v 的**首次调用**下发冷响应——猜你喜欢 cell 还在但
/// `cell_data` 里的短剧子格稀少甚至为空，连打会逐次填充（2026-10-08
/// 实测同进程 4 连打 guess 1→6→8→9）。客户端每次进详情页只打一次、
/// 前端还带 10 分钟缓存，冷响应会原样上屏成「猜你喜欢消失」。
/// 对策：guess 空时短间隔重试（最多 3 调），取首个非空结果；works
/// 各次稳定不受影响，全空就退最后一次的响应。
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

    let mut last = RelatedSeries::default();
    for attempt in 0..3u8 {
        if attempt > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        }
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
        let related = parse_related_series(&value)?;
        if !related.guess.is_empty() {
            return Ok(related);
        }
        last = related;
    }
    Ok(last)
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
        // 猜你喜欢 cell 的条目不在自己的 video_data 里，而在 **cell_data[]**
        // （二级 cell 列表，cell_name 形如「双列短剧」）各自的 video_data 里
        // ——2026-10-07 活体探测实锤（此前只读 video_data 恒为 0 条）。
        // hgplayer 二进制里 `json:"guess" guess_mvs` 即此结构。
        let raw_items: Vec<&Value> = match cell.get("video_data").and_then(Value::as_array) {
            Some(arr) if !arr.is_empty() => arr.iter().collect(),
            _ => cell
                .get("cell_data")
                .and_then(Value::as_array)
                .map(|subs| {
                    subs.iter()
                        .filter_map(|sub| sub.get("video_data").and_then(Value::as_array))
                        .flatten()
                        .collect()
                })
                .unwrap_or_default(),
        };
        let items = raw_items
            .iter()
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
                        .unwrap_or_else(|| raw.get("score").and_then(Value::as_f64).unwrap_or(0.0)),
                    play_cnt: int_field(raw, "play_cnt"),
                    episode_cnt,
                    video_desc: str_field(raw, "video_desc"),
                })
            })
            .collect();
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
                digg_count: item
                    .get("digged_count")
                    .and_then(Value::as_i64)
                    .unwrap_or(0),
                // 时长（秒，整型；选集格「02:35」角标数据源）
                duration: item.get("duration").and_then(Value::as_i64).unwrap_or(0),
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

// ---------------------------------------------------------------- 剧集元信息

/// 详情页头部元信息端点（2026-10-07 抓 hgplayer 详情页锁定）：
/// `POST /novel/player/video_detail/v1/`，body `{"biz_param":{…},"series_id"}`。
/// 追剧数/播放量/季徽/题材标签/备案号都在这里——preload 分集接口不带这些。
pub const VIDEO_DETAIL_PATH: &str = "/novel/player/video_detail/v1/";

/// 详情页头部的剧集元信息（对齐 hgplayer 头部数据面）。
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SeriesMeta {
    pub series_id: String,
    pub title: String,
    pub cover: String,
    /// 追剧数（followed_cnt，44.7万人追剧）
    pub followed_cnt: i64,
    /// 全剧播放量（series_play_cnt，150.8万次播放）
    pub play_cnt: i64,
    /// 红果热度值（hot_score，3786万；2026-10-08 抓包实锤与 hgplayer
    /// 头部「🔥红果热度值3786万」同源同值）
    pub hot_score: i64,
    /// 备案号（record_info.record_number；响应里有 show 开关，前端恒显即可）
    pub record_number: String,
    /// 季徽（secondary_infos data_type=0 的 content，如「第1季」）
    pub season: String,
    /// 题材标签（secondary_infos data_type=3 的 content：玄幻/逆袭/修真…）
    pub tags: Vec<String>,
}

/// 拉详情页头部元信息。失败交调用方降级（头部缺这几行不影响主功能）。
pub async fn fetch_series_meta(
    series_id: &str,
    env: &super::client::ApiEnv,
) -> AppResult<SeriesMeta> {
    // body 照抄抓包（audit 纪律）：screen_width_px 是字符串形态
    let body = serde_json::to_vec(&serde_json::json!({
        "biz_param": {
            "caller_scene": "single_col",
            "detail_page_version": 1,
            "disable_digg_stat": false,
            "disable_video_relate_book": false,
            "from_video_id": "",
            "need_all_video_definition": false,
            "need_mp4_align": false,
            "screen_width_px": "1078",
            "source": 4,
            "use_os_player": false,
            "use_server_dns": false,
            "video_id_type": 1,
        },
        "series_id": series_id,
    }))
    .map_err(|e| AppError::Signer(e.to_string()))?;

    let bytes = super::client::api_call(VIDEO_DETAIL_PATH, Some(body), env).await?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析详情元信息失败: {e}")))?;
    parse_series_meta(&value, series_id)
}

/// 解析 video_detail 响应。字段在 `data[series_id]` 下（与 preload 的
/// `video_data` 包一层不同，这个端点是平铺的），两层都兜一下。
pub fn parse_series_meta(response: &Value, series_id: &str) -> AppResult<SeriesMeta> {
    if let Some(code) = response.get("code").and_then(Value::as_i64) {
        if code != 0 {
            let msg = response
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("未知错误");
            return Err(AppError::Media(format!("详情元信息接口返回 {code}: {msg}")));
        }
    }
    let node = response
        .pointer(&format!("/data/{series_id}"))
        .or_else(|| {
            response
                .get("data")
                .and_then(|d| d.as_object().and_then(|o| o.values().next()))
        })
        .ok_or_else(|| AppError::Media("详情元信息响应里没有 data".into()))?;
    let vd = node.get("video_data").unwrap_or(node);

    // secondary_infos：data_type 0 = 季徽（highlight），3 = 题材标签
    let mut season = String::new();
    let mut tags = Vec::new();
    if let Some(items) = node
        .pointer("/secondary_infos")
        .or_else(|| vd.pointer("/secondary_infos"))
        .and_then(Value::as_array)
    {
        for item in items {
            let content = item
                .get("content")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if content.is_empty() {
                continue;
            }
            match item.get("data_type").and_then(Value::as_i64) {
                Some(0) if season.is_empty() => season = content.to_string(),
                Some(3) => tags.push(content.to_string()),
                _ => {}
            }
        }
    }

    let sid = pick(vd, &["series_id_str", "series_id"]);
    Ok(SeriesMeta {
        series_id: if sid.is_empty() {
            series_id.to_string()
        } else {
            sid
        },
        title: pick(vd, &["series_title"]),
        cover: pick(vd, &["series_cover"]),
        followed_cnt: vd
            .get("followed_cnt")
            .and_then(Value::as_i64)
            .or_else(|| {
                vd.get("followed_cnt")
                    .and_then(Value::as_str)
                    .and_then(|s| s.parse().ok())
            })
            .unwrap_or(0),
        play_cnt: vd
            .get("series_play_cnt")
            .and_then(Value::as_i64)
            .or_else(|| {
                vd.get("series_play_cnt")
                    .and_then(Value::as_str)
                    .and_then(|s| s.parse().ok())
            })
            .unwrap_or(0),
        hot_score: vd
            .get("hot_score")
            .and_then(Value::as_i64)
            .or_else(|| {
                vd.get("hot_score")
                    .and_then(Value::as_str)
                    .and_then(|s| s.parse().ok())
            })
            .unwrap_or(0),
        record_number: vd
            .pointer("/record_info/record_number")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        season,
        tags,
    })
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
                            { "vid": "v2", "vid_index": 2, "title": "第二集", "duration": 92 },
                            { "vid": "v1", "vid_index": 1, "title": "第一集", "duration": 155 }
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
        assert_eq!(list.episodes[0].duration, 155, "时长（秒）随分集带出");
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
    fn parses_series_meta_from_video_detail() {
        // 2026-10-07 抓包样本：字段平铺在 data[sid] 下（无 video_data 包层），
        // 计数字段是字符串形态；季徽 data_type=0，题材 data_type=3
        let v: Value = serde_json::json!({
            "code": 0,
            "data": {
                "7678641104899542041": {
                    "series_id_str": "7678641104899542041",
                    "series_title": "序列：我一人即是黄昏议会",
                    "series_cover": "https://example/cover.heic",
                    "followed_cnt": "1069774",
                    "series_play_cnt": "5160926",
                    "hot_score": 44270843,
                    "record_info": { "record_number": "（番茄）网微剧备字（2026）第847205号", "show": true },
                    "secondary_infos": [
                        { "content": "第1季", "data_type": 0, "highlight": true },
                        { "content": "玄幻", "data_type": 3 },
                        { "content": "逆袭", "data_type": 3 },
                        { "content": "忽略项", "data_type": 9 }
                    ]
                }
            }
        });
        let m = parse_series_meta(&v, "7678641104899542041").unwrap();
        assert_eq!(m.title, "序列：我一人即是黄昏议会");
        assert_eq!(m.followed_cnt, 1_069_774, "字符串形态计数要能解析");
        assert_eq!(m.play_cnt, 5_160_926);
        assert_eq!(m.hot_score, 44_270_843, "热度值直取 hot_score");
        assert_eq!(m.season, "第1季");
        assert_eq!(m.tags, vec!["玄幻", "逆袭"], "data_type=3 才是题材标签");
        assert_eq!(m.record_number, "（番茄）网微剧备字（2026）第847205号");
    }

    #[test]
    fn guess_cell_items_live_in_cell_data() {
        // 2026-10-07 活体探测实锤：猜你喜欢 cell 的条目走 **cell_data[]**
        // （二级 cell 各带 video_data），不是自己的 video_data——
        // 只读 video_data 时猜你喜欢恒为 0 条（tab 计数 4 vs 第三方 33 事故）
        let v: Value = serde_json::json!({
            "code": 0,
            "data": [
                { "cell_name": "相关作品", "video_data": [
                    { "series_id": "1", "title": "第二季", "episode_cnt": 52 }
                ]},
                { "cell_name": "猜你喜欢", "cell_data": [
                    { "cell_name": "双列短剧", "video_data": [
                        { "series_id": "2", "title": "A 剧", "score": "8.0", "play_cnt": 443041, "episode_cnt": 121 }
                    ]},
                    { "cell_name": "双列短剧", "video_data": [
                        { "series_id": "3", "title": "B 剧" }
                    ]}
                ]}
            ]
        });
        let rel = parse_related_series(&v).unwrap();
        assert_eq!(rel.works.len(), 1);
        assert_eq!(rel.guess.len(), 2, "cell_data 里的条目要拍平进猜你喜欢");
        assert_eq!(rel.guess[0].score, 8.0);
        assert_eq!(rel.guess[0].play_cnt, 443_041);
    }

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
        let payload = serde_json::to_vec(&crate::domain::api::params::detail_payload(
            "7690883800057777177",
        ))
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
        let count_keys: Vec<&String> = keys
            .iter()
            .filter(|k| k.contains("count") || k.contains("cnt"))
            .collect();
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

    /// 活体探测：plan/v 的真实 cell 结构 + 剧评响应 extra 的评分字段。
    /// 用法：`PROBE_SERIES_ID=<id> cargo test probe_live -- --ignored --nocapture`
    /// （默认疯神镇妖官）。probe_detail_related_series 是原有探测，见下。
    #[ignore = "直连真实接口的探测用例"]
    #[tokio::test]
    async fn probe_fengshen_live() {
        let proxy = crate::domain::model::ProxyConfig::default();
        // PROBE_DEVICE_FILE 给真实设备档案（默认静态兜底档案）：
        // 评分/猜你喜欢疑随设备信任度下发，匿名静态档案拿到的是空。
        let env = match std::env::var("PROBE_DEVICE_FILE") {
            Ok(path) => {
                let json = std::fs::read_to_string(&path).expect("读设备档案");
                let device = serde_json::from_str(&json).expect("解析设备档案");
                crate::domain::api::client::ApiEnv {
                    proxy,
                    device,
                    cookie: None,
                    x_tt_token: None,
                }
            }
            Err(_) => crate::domain::api::client::ApiEnv::anonymous(proxy),
        };
        let sid = std::env::var("PROBE_SERIES_ID").unwrap_or_else(|_| "7685637575473630270".into());

        let rel = fetch_related_series(&sid, &env).await.expect("plan");
        println!("[plan] works={} guess={}", rel.works.len(), rel.guess.len());
        for w in rel.works.iter().chain(rel.guess.iter()).take(6) {
            println!(
                "[plan]   {} | {} | tag={} score={} ep={} play={}",
                w.series_id, w.title, w.tag, w.score, w.episode_cnt, w.play_cnt
            );
        }

        match super::super::danmaku::fetch_series_comments_page(&sid, "", &env).await {
            Ok(page) => {
                println!(
                    "[reviews] total={} score={:?} score_cnt={} tags={:?}",
                    page.page.total, page.score, page.score_cnt, page.tags
                );
            }
            Err(e) => println!("[reviews] ERR: {e}"),
        }

        match fetch_series_meta(&sid, &env).await {
            Ok(m) => println!(
                "[meta] title={} followed={} play={} score? season={} tags={:?} record={}",
                m.title, m.followed_cnt, m.play_cnt, m.season, m.tags, m.record_number
            ),
            Err(e) => println!("[meta] ERR: {e}"),
        }

        // video_detail 原始响应里搜评分键（头部 8.0分 的可能来源）
        let payload = serde_json::to_vec(&serde_json::json!({
            "biz_param": {
                "caller_scene": "single_col", "detail_page_version": 1,
                "disable_digg_stat": false, "disable_video_relate_book": false,
                "from_video_id": "", "need_all_video_definition": false,
                "need_mp4_align": false, "screen_width_px": "1078", "source": 4,
                "use_os_player": false, "use_server_dns": false, "video_id_type": 1,
            },
            "series_id": sid,
        }))
        .unwrap();
        let vd_bytes = crate::domain::api::client::api_call(VIDEO_DETAIL_PATH, Some(payload), &env)
            .await
            .expect("video_detail");
        let vd_raw = String::from_utf8_lossy(&vd_bytes).to_string();
        for key in ["\"score\"", "rating", "digg_cnt", "comment_count"] {
            let mut from = 0;
            for _ in 0..2 {
                let Some(pos) = vd_raw[from..].find(key) else {
                    break;
                };
                let at = from + pos;
                let lo = at.saturating_sub(40);
                let hi = (at + 100).min(vd_raw.len());
                println!(
                    "[vdetail] {key} @ {at}: ...{}",
                    vd_raw[lo..hi].replace(char::is_whitespace, " ")
                );
                from = at + 1;
            }
        }

        // 原始 plan 响应的 cell 名与条数（不经解析，防解析器吞内容）
        let biz_query: Vec<(String, String)> = [
            ("book_id", sid.as_str()),
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
            super::super::danmaku::LQ_API_ORIGIN,
            PLAN_PATH,
            None,
            &biz_query,
            &env,
        )
        .await
        .expect("plan raw");
        // PROBE_DUMP_SET 时把原始响应落盘，供离线分析（服务端结构常变，
        // 反复编译探测太慢）。
        if let Ok(dir) = std::env::var("PROBE_DUMP") {
            let path = std::path::Path::new(&dir).join("plan-raw.json");
            std::fs::write(&path, &bytes).expect("写 plan-raw.json");
            println!("[plan raw] dumped -> {}", path.display());
        }
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        for cell in v
            .get("data")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let name = cell.get("cell_name").and_then(Value::as_str).unwrap_or("?");
            let n = cell
                .get("video_data")
                .and_then(Value::as_array)
                .map(|a| a.len())
                .unwrap_or(0);
            println!("[plan raw] cell={name} items={n}");
            // cell 里的其它数组字段（hgplayer 的 guess_mvs tag 暗示猜你喜欢
            // 可能不走 video_data 而走专用字段）
            if let Some(obj) = cell.as_object() {
                for (k, val) in obj {
                    if k == "video_data" || k == "cell_name" {
                        continue;
                    }
                    if let Some(arr) = val.as_array() {
                        println!("[plan raw]   cell={name} arr {k} len={}", arr.len());
                        if let Some(first) = arr.first() {
                            let keys: Vec<_> = first
                                .as_object()
                                .map(|o| o.keys().cloned().collect())
                                .unwrap_or_default();
                            println!("[plan raw]     first keys: {keys:?}");
                            println!(
                                "[plan raw]     first: {}",
                                serde_json::to_string(first).unwrap_or_default()
                            );
                        }
                    }
                }
            }
        }
        let raw = String::from_utf8_lossy(&bytes).to_string();
        for key in ["guess_mvs", "guess"] {
            let mut from = 0;
            for _ in 0..3 {
                let Some(pos) = raw[from..].find(key) else {
                    break;
                };
                let at = from + pos;
                let lo = at.saturating_sub(60);
                let hi = (at + 120).min(raw.len());
                println!(
                    "[plan raw] {key} @ {at}: ...{}...",
                    raw[lo..hi].replace(char::is_whitespace, " ")
                );
                from = at + 1;
            }
        }
    }

    #[ignore = "直连真实接口的探测用例"]
    #[tokio::test]
    async fn probe_detail_related_series() {
        let env = crate::domain::api::client::ApiEnv::anonymous(
            crate::domain::model::ProxyConfig::default(),
        );
        let payload = serde_json::to_vec(&crate::domain::api::params::detail_payload(
            "7689382439004671038",
        ))
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
        let mut vkeys: Vec<_> = vd
            .as_object()
            .map(|o| o.keys().cloned().collect())
            .unwrap_or_default();
        vkeys.sort();
        println!("[rel] video_data keys: {vkeys:?}");
        let s = serde_json::to_string(&v).unwrap();
        for key in [
            "relation",
            "relate_series",
            "series_list",
            "season",
            "同IP",
            "相关",
            "recommend",
        ] {
            let n = s.matches(key).count();
            if n > 0 {
                println!("  [{key}] x{n}");
            }
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
        let rel = fetch_related_series("7687961503718198334", &env)
            .await
            .expect("相关作品");
        println!(
            "[related] works {} 条, guess {} 条",
            rel.works.len(),
            rel.guess.len()
        );
        for w in rel.works.iter().take(6) {
            println!(
                "  [{}] {} score={} {}集 play={}",
                w.tag, w.title, w.score, w.episode_cnt, w.play_cnt
            );
        }
        assert!(!rel.works.is_empty(), "多季剧必有相关作品");
    }
}
