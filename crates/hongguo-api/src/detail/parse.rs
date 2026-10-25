//! 详情域的 JSON 解析（分集列表 / 相关作品 / 头部元信息）。

use serde_json::Value;

use super::model::{EpisodeList, RelatedItem, RelatedSeries, SeriesCreator, SeriesMeta};
use hongguo_common::model::Episode;
use hongguo_common::utils::json::{check_code_in, int_field, str_field, str_field_any};
use hongguo_common::{AppError, AppResult};

/// 解析 plan/v 响应为相关作品两块内容。
pub(super) fn parse_related_series(value: &Value) -> AppResult<RelatedSeries> {
    check_code_in(value, "相关作品接口")?;

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
        // ——2026-10-07 活体探测实锤（此前只读 video_data 恒为 0 条）；
        // guess_mvs 结构即此形态。
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

/// 从 detail 响应里解析分集。
pub fn parse_episodes(response: &Value) -> AppResult<EpisodeList> {
    check_code_in(response, "详情接口")?;

    let data = response
        .get("data")
        .and_then(Value::as_object)
        .filter(|d| !d.is_empty())
        .ok_or_else(|| AppError::Media("响应里没有 data".into()))?;

    let (sid, node) = data.iter().next().expect("data 已确认非空");
    episode_list_of(sid, node)
}

/// 批量形态：`data` 的每个 series 各解析一条（官方预取对 6 部剧打一次
/// `multi_video_detail/preload`，逗号串 series_id，响应 data 按剧分发，
/// 2026-10-25 抓包 + 直连 probe 实证）。单部残缺跳过，不拖垮整批。
pub fn parse_episodes_batch(response: &Value) -> AppResult<Vec<EpisodeList>> {
    check_code_in(response, "详情接口")?;

    let data = response
        .get("data")
        .and_then(Value::as_object)
        .filter(|d| !d.is_empty())
        .ok_or_else(|| AppError::Media("响应里没有 data".into()))?;

    let lists = data
        .iter()
        .filter_map(|(sid, node)| episode_list_of(sid, node).ok())
        .collect::<Vec<_>>();
    if lists.is_empty() {
        return Err(AppError::Media("批量分集列表为空".into()));
    }
    Ok(lists)
}

/// 单剧节点 → `EpisodeList`（`parse_episodes` 的主体抽出，批量共用）。
fn episode_list_of(series_id: &str, node: &Value) -> AppResult<EpisodeList> {
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

    let cover = str_field_any(&video_data, &["series_cover", "cover_url"]);
    Ok(EpisodeList {
        series_id: series_id.to_string(),
        title: str_field_any(&video_data, &["series_title"]),
        cover,
        episodes,
        followed_cnt: video_data
            .get("followed_cnt")
            .and_then(Value::as_i64)
            .unwrap_or(0),
        content_type: video_data
            .get("content_type")
            .and_then(Value::as_i64)
            .unwrap_or(0),
    })
}

/// 解析 video_detail 响应。字段在 `data[series_id]` 下（与 preload 的
/// `video_data` 包一层不同，这个端点是平铺的），两层都兜一下。
pub fn parse_series_meta(response: &Value, series_id: &str) -> AppResult<SeriesMeta> {
    check_code_in(response, "详情元信息接口")?;
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

    let sid = str_field_any(vd, &["series_id_str", "series_id"]);
    Ok(SeriesMeta {
        series_id: if sid.is_empty() {
            series_id.to_string()
        } else {
            sid
        },
        title: str_field_any(vd, &["series_title"]),
        cover: str_field_any(vd, &["series_cover"]),
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
        intro: str_field_any(vd, &["series_intro", "video_desc", "abstract"]),
        // 官方模型对照字段（VideoDetailVideoData）：抓包样本未及、防御性
        // 读取，缺席走默认值
        pay_info: vd.get("pay_info").cloned().unwrap_or(Value::Null),
        // 官方下发为 unix 秒；对外契约统一 UTC 毫秒
        online_time: num_field(vd, "online_time") * 1000,
        update_tag: str_field_any(vd, &["episode_right_text", "update_tag"]),
        // 追更按钮显隐判定：0=连载中 1=已完结（2026-10-15 官方实包：完结剧
        // series_status=1 且无 update_info；缺席按连载中处理）
        series_status: num_field(vd, "series_status"),
        update_frequency: vd
            .pointer("/update_info/show_update_frequency")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        episode_total_cnt: num_field(vd, "episode_total_cnt"),
        latest_consumed_vid: str_field_any(vd, &["latest_consumed_item_id"]),
        creators: parse_creators(vd),
    })
}

/// 解析出品方 / 演职员（main_creator_users，2026-10-15 抓 player v1.2.1
/// 详情页实包）：base_info 出名字头像，user_tag.sub_title_list 首项出角色
/// 文案（出品方/主演/配角）。名字为空的条目跳过。
fn parse_creators(vd: &Value) -> Vec<SeriesCreator> {
    let mut out = Vec::new();
    for raw in vd
        .get("main_creator_users")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
    {
        let name = raw
            .pointer("/base_info/user_name")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if name.is_empty() {
            continue;
        }
        out.push(SeriesCreator {
            user_id: raw
                .pointer("/base_info/user_id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            name: name.to_string(),
            avatar: raw
                .pointer("/base_info/user_avatar")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            role: raw
                .pointer("/user_tag/sub_title_list/0/text")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
        });
    }
    out
}

/// 计数字段：JSON 数字直取，字符串形态回退解析（与 followed_cnt 同款容错）。
fn num_field(v: &Value, key: &str) -> i64 {
    v.get(key)
        .and_then(Value::as_i64)
        .or_else(|| {
            v.get(key)
                .and_then(Value::as_str)
                .and_then(|s| s.parse().ok())
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
                    "series_intro": "穿越者落地成盒，重启人生第二回合。",
                    "followed_cnt": "1069774",
                    "series_play_cnt": "5160926",
                    "hot_score": 44270843,
                    "record_info": { "record_number": "（番茄）网微剧备字（2026）第847205号", "show": true },
                    "secondary_infos": [
                        { "content": "第1季", "data_type": 0, "highlight": true },
                        { "content": "玄幻", "data_type": 3 },
                        { "content": "逆袭", "data_type": 3 },
                        { "content": "忽略项", "data_type": 9 }
                    ],
                    "pay_info": { "need_pay": true, "price": "18.0" },
                    "online_time": 1790000000,
                    // 2026-10-15 官方实包形态：连载剧 series_status=0 +
                    // update_info + episode_right_text「更新至N集」
                    "series_status": 0,
                    "episode_right_text": "更新至45集",
                    "episode_total_cnt": 80,
                    "update_info": {
                        "show_update_frequency": "每周六更新5集",
                        "update_tag_text": "连载"
                    },
                    "latest_consumed_item_id": "7690802329313872921",
                    "main_creator_users": [
                        {
                            "base_info": {
                                "user_id": "2007062623837002",
                                "user_name": "林间剧场",
                                "user_avatar": "http://example/av.jpg"
                            },
                            "user_tag": {
                                "creator_type": 2,
                                "sub_title_list": [{ "text": "出品方", "text_ext": 0 }]
                            }
                        },
                        { "base_info": { "user_name": "" } }
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
        assert_eq!(m.intro, "穿越者落地成盒，重启人生第二回合。");
        assert_eq!(
            m.update_tag, "更新至45集",
            "统计行原文取 episode_right_text"
        );
        assert_eq!(m.series_status, 0, "连载剧");
        assert_eq!(m.update_frequency, "每周六更新5集");
        assert_eq!(m.episode_total_cnt, 80);
        assert_eq!(m.online_time, 1_790_000_000_000);
        assert_eq!(m.latest_consumed_vid, "7690802329313872921");
        assert!(
            m.pay_info
                .get("need_pay")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            "pay_info 原样透传"
        );
        // 出品方（2026-10-15 抓 player 实包形态）：取 base_info 三件 +
        // sub_title_list 首项做角色；空名条目跳过
        assert_eq!(m.creators.len(), 1);
        assert_eq!(m.creators[0].name, "林间剧场");
        assert_eq!(m.creators[0].role, "出品方");
        assert_eq!(m.creators[0].user_id, "2007062623837002");
        assert_eq!(m.creators[0].avatar, "http://example/av.jpg");
    }

    /// 官方模型对照字段缺席时走默认值（防御性读取语义）。
    #[test]
    fn series_meta_alignment_fields_default_when_absent() {
        let v: Value = serde_json::json!({
            "code": 0,
            "data": { "1": { "series_id_str": "1", "series_title": "t" } }
        });
        let m = parse_series_meta(&v, "1").unwrap();
        assert!(m.update_tag.is_empty());
        assert_eq!(m.series_status, 0, "缺席按连载中处理");
        assert!(m.update_frequency.is_empty(), "无 update_info 对象即空");
        assert_eq!(m.episode_total_cnt, 0);
        assert_eq!(m.online_time, 0);
        assert!(m.latest_consumed_vid.is_empty());
        assert!(m.pay_info.is_null());
    }

    /// 完结剧形态（2026-10-15 官方实包「神豪：大风刮来的全能系统」）：
    /// series_status=1、无 update_info、统计行是「全139集」。
    #[test]
    fn finished_series_has_no_update_info() {
        let v: Value = serde_json::json!({
            "code": 0,
            "data": {
                "7679720313600347198": {
                    "series_id_str": "7679720313600347198",
                    "series_title": "神豪：大风刮来的全能系统",
                    "series_status": 1,
                    "episode_right_text": "全139集",
                    "episode_cnt": 139
                }
            }
        });
        let m = parse_series_meta(&v, "7679720313600347198").unwrap();
        assert_eq!(m.series_status, 1, "完结剧");
        assert_eq!(m.update_tag, "全139集");
        assert!(m.update_frequency.is_empty(), "完结剧无更新频率");
    }

    #[test]
    fn guess_cell_items_live_in_cell_data() {
        // 2026-10-07 活体探测实锤：猜你喜欢 cell 的条目走 **cell_data[]**
        // （二级 cell 各带 video_data），不是自己的 video_data——
        // 只读 video_data 时猜你喜欢恒为 0 条（tab 计数 4 vs 实际 33 事故）
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
