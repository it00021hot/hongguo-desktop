//! 排行榜 / 新剧 / 预约 / 上新日历接口（2026-10-03 mitmproxy 抓包锁定）。
//!
//! 抓包口径（详见 docs/hongguo-api-endpoints.md）：
//! - 排行榜 `GET /reading/bookapi/bookmall/cell/change/v`（**/v 无斜杠**），
//!   固定 `cell_id=7470092475068071998, tab_type=26, selected_items=all`，
//!   榜单用 `sub_selected_items` 切换；
//! - 新剧 `GET .../cell/change/v1/`（**v1/ 带斜杠**，与排行榜不同路径），
//!   `selected_items=firstonlinetime_new, cell_gender`；
//! - 两者响应都是 `data.cell_view.cell_data[].video_data[]`（每块一条，
//!   `recommend_info` 是二次序列化 JSON 字符串，内含排名 `rank`）；
//! - 预约与上新日历共用 `GET /reading/user/subscribe/list/v1/`：
//!   预约 `tab_type=13` + `is_online`，日历 `tab_type=5` + `need_calendar_schema=true`。
//!
//! bookmall 系对设备敏感（静态旧设备报 ILLEGAL_ACCESS 110），设备注册实现前，
//! 探测用例走 hgplayer 抓包里的已注册设备档案。

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::client::{api_call_full, ApiEnv};
use super::discover::{check_code, int_field, num_field, str_field};
use crate::error::{AppError, AppResult};
use super::danmaku::LQ_API_ORIGIN;

/// 排行榜（书城 cell 换一换，/v 无斜杠）。
pub const RANK_CELL_PATH: &str = "/reading/bookapi/bookmall/cell/change/v";
/// 新剧推荐（v1/ 带斜杠——两条路径后缀不一致是服务端原状，抓包实锤）。
pub const NEW_DRAMA_CELL_PATH: &str = "/reading/bookapi/bookmall/cell/change/v1/";
/// 预约列表与上新日历共用。
pub const SUBSCRIBE_LIST_PATH: &str = "/reading/user/subscribe/list/v1/";

/// 排行榜页的 8 个榜单（顺序即 UI 竖排自上而下）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RankList {
    Recommend,
    HotPlay,
    Prestige,
    Subscribe,
    NewDrama,
    HotSearch,
    MustWatch,
    Followed,
}

impl RankList {
    /// 抓包得到的 `sub_selected_items` 标识。
    pub fn as_sub_selected(self) -> &'static str {
        match self {
            RankList::Recommend => "ranklist_hot_sc",
            RankList::HotPlay => "ranklist_hot_play_sc",
            RankList::Prestige => "ranklist_prestige",
            RankList::Subscribe => "ranklist_subscribe",
            RankList::NewDrama => "ranklist_new_rank_sc",
            RankList::HotSearch => "ranklist_hot_search_sc",
            RankList::MustWatch => "ranklist_must_watch",
            RankList::Followed => "ranklist_followed",
        }
    }

    /// 全部榜单（探测/测试遍历用；command 层按前端传的 kind 单独取）。
    #[cfg(test)]
    pub fn all() -> [RankList; 8] {
        [
            RankList::Recommend,
            RankList::HotPlay,
            RankList::Prestige,
            RankList::Subscribe,
            RankList::NewDrama,
            RankList::HotSearch,
            RankList::MustWatch,
            RankList::Followed,
        ]
    }
}

/// 榜单条目：比信息流卡片多排名与榜单文案。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RankItem {
    pub series_id: String,
    pub title: String,
    #[serde(default)]
    pub cover: String,
    #[serde(default)]
    pub vid: String,
    /// 榜单名次（recommend_info.rank，从 1 起）
    pub rank: u32,
    /// "玄幻·全200集" 形态的副标题
    #[serde(default)]
    pub sub_title: String,
    /// 评分（"8.0"，容错字符串）
    #[serde(default)]
    pub score: f64,
    #[serde(default)]
    pub play_cnt: i64,
    #[serde(default)]
    pub episode_cnt: u32,
    /// 榜单热点文案（rec_text_item.RecommendText，如 "13707万最高热度"）
    #[serde(default)]
    pub rec_text: String,
    /// 次级信息（secondary_info_list[].content，如 "258.7万收藏"）
    #[serde(default)]
    pub secondary_infos: Vec<String>,
    /// 简介（video_desc）
    #[serde(default)]
    pub description: String,
    /// 题材标签（category_schema 二次解析）
    #[serde(default)]
    pub tags: Vec<String>,
}

/// 一页榜单。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RankPage {
    pub items: Vec<RankItem>,
}

/// 拉一个榜单（全量一次返回，无翻页；抓包 offset/limit 固定 0）。
pub async fn fetch_rank(list: RankList, env: &ApiEnv) -> AppResult<RankPage> {
    let q: Vec<(String, String)> = [
        ("cell_id", "7470092475068071998"),
        ("tab_type", "26"),
        ("selected_items", "all"),
        ("category_id", "0"),
        ("cell_sub_id", "0"),
        ("client_req_type", "2"),
        ("client_template", "2"),
        ("gender", "2"),
        ("limit", "0"),
        ("offset", "0"),
        ("sub_selected_items", list.as_sub_selected()),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    let bytes = api_call_full(LQ_API_ORIGIN, RANK_CELL_PATH, None, &q, env).await?;
    let value: Value =
        serde_json::from_slice(&bytes).map_err(|e| AppError::Media(format!("解析榜单失败: {e}")))?;
    check_code(&value)?;
    Ok(RankPage {
        items: parse_rank_items(value.get("data"))?,
    })
}

/// 新剧推荐页（`cell_gender`：2=全部，其余见抓包；分页 offset/limit）。
pub async fn fetch_new_drama(gender: i64, offset: i64, env: &ApiEnv) -> AppResult<RankPage> {
    let q: Vec<(String, String)> = [
        ("cell_id", "7431550523368554558"),
        ("selected_items", "firstonlinetime_new"),
        ("cell_gender", gender.to_string().as_str()),
        ("change_type", "1"),
        ("client_req_type", "1"),
        ("limit", "18"),
        ("offset", offset.to_string().as_str()),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    let bytes = api_call_full(LQ_API_ORIGIN, NEW_DRAMA_CELL_PATH, None, &q, env).await?;
    let value: Value =
        serde_json::from_slice(&bytes).map_err(|e| AppError::Media(format!("解析新剧失败: {e}")))?;
    check_code(&value)?;
    Ok(RankPage {
        items: parse_rank_items(value.get("data"))?,
    })
}

/// 预约列表（is_online=true 已上线 / false 待上线）。
pub async fn fetch_reservations(is_online: bool, env: &ApiEnv) -> AppResult<RankPage> {
    let q: Vec<(String, String)> = [
        ("is_online", if is_online { "true" } else { "false" }),
        ("limit", "20"),
        ("offset", "0"),
        ("subscribe_offset", "0"),
        ("subscribe_order_type", "0"),
        ("swipe_type", "0"),
        ("tab_type", "13"),
        ("need_calendar_schema", "false"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    let bytes = api_call_full(LQ_API_ORIGIN, SUBSCRIBE_LIST_PATH, None, &q, env).await?;
    let value: Value =
        serde_json::from_slice(&bytes).map_err(|e| AppError::Media(format!("解析预约失败: {e}")))?;
    check_code(&value)?;
    Ok(RankPage {
        items: parse_rank_items(value.get("data"))?,
    })
}

/// 遍历 `data` 下所有 `video_data` 数组（cell_data 是嵌套结构，逐块平铺）。
fn collect_video_data(node: &Value, out: &mut Vec<Value>) {
    if let Some(arr) = node.as_array() {
        for item in arr {
            collect_video_data(item, out);
        }
        return;
    }
    let Some(obj) = node.as_object() else { return };
    if let Some(vd) = obj.get("video_data").and_then(Value::as_array) {
        out.extend(vd.iter().cloned());
    }
    for (_k, v) in obj {
        collect_video_data(v, out);
    }
}

/// cell_view 结构 → 平铺榜单条目。
fn parse_rank_items(data: Option<&Value>) -> AppResult<Vec<RankItem>> {
    let data = data.ok_or_else(|| AppError::Media("响应缺少 data".into()))?;
    let mut raws = Vec::new();
    collect_video_data(data, &mut raws);

    let mut items = Vec::new();
    for raw in &raws {
        let Some(series_id) = raw.get("series_id").and_then(Value::as_str).filter(|s| !s.is_empty())
        else {
            continue;
        };
        let rank = raw
            .get("recommend_info")
            .and_then(Value::as_str)
            .and_then(|s| serde_json::from_str::<Value>(s).ok())
            .and_then(|info| int_field(&info, "rank").try_into().ok())
            .unwrap_or(0);
        let rec_text = raw
            .get("rec_text_item")
            .and_then(|t| t.get("RecommendText"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let secondary_infos = raw
            .get("secondary_info_list")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.get("content").and_then(Value::as_str))
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        items.push(RankItem {
            series_id: series_id.to_string(),
            title: str_field(raw, "title"),
            cover: str_field(raw, "cover"),
            vid: str_field(raw, "vid"),
            rank,
            sub_title: str_field(raw, "sub_title"),
            score: num_field(raw, "score"),
            play_cnt: int_field(raw, "play_cnt"),
            episode_cnt: int_field(raw, "episode_cnt").max(0) as u32,
            rec_text,
            secondary_infos,
            description: str_field(raw, "video_desc"),
            tags: super::discover::parse_tags(raw.get("category_schema")),
        });
    }
    // 排名字段缺失时按抓包顺序（接口本身就是榜单序）
    items.sort_by_key(|i| if i.rank == 0 { u32::MAX } else { i.rank });
    Ok(items)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 抓包样本形状：cell_data 每块 1 条 + recommend_info 二次序列化。
    #[test]
    fn parses_rank_items_from_cell_view() {
        let data: Value = serde_json::json!({
            "cell_view": {
                "cell_id": 7470092475068071998_i64,
                "cell_data": [
                    { "video_data": [{
                        "series_id": "7681495327056071704",
                        "title": "万妖图录传第十二季",
                        "cover": "https://x.heic",
                        "vid": "7681497259883645977",
                        "sub_title": "玄幻·全200集",
                        "score": "8.0",
                        "play_cnt": 6197486,
                        "episode_cnt": 200,
                        "rec_text_item": { "RecommendText": "13707万最高热度" },
                        "secondary_info_list": [{ "content": "258.7万收藏" }],
                        "recommend_info": "{\"rank\":\"1\"}",
                        "video_desc": "以妖魔之血为墨",
                    }]},
                    { "video_data": [{
                        "series_id": "",
                        "title": "废条目"
                    }]}
                ]
            }
        });
        let items = parse_rank_items(Some(&data)).unwrap();
        assert_eq!(items.len(), 1, "空 series_id 要跳过");
        let it = &items[0];
        assert_eq!(it.rank, 1);
        assert_eq!(it.rec_text, "13707万最高热度");
        assert_eq!(it.secondary_infos, vec!["258.7万收藏"]);
        assert_eq!(it.score, 8.0);
        assert_eq!(it.sub_title, "玄幻·全200集");
    }

    #[test]
    fn rank_items_sort_by_rank_missing_last() {
        let data: Value = serde_json::json!({
            "cell_data": [
                { "video_data": [
                    { "series_id": "b", "recommend_info": "{\"rank\":\"2\"}" },
                    { "series_id": "a", "recommend_info": "{\"rank\":\"1\"}" },
                    { "series_id": "c" }
                ]}
            ]
        });
        let items = parse_rank_items(Some(&data)).unwrap();
        let ids: Vec<&str> = items.iter().map(|i| i.series_id.as_str()).collect();
        assert_eq!(ids, vec!["a", "b", "c"], "按名次排，缺名次殿后");
    }

    #[test]
    fn rank_list_covers_all_eight() {
        let subs: Vec<&str> = RankList::all().iter().map(|l| l.as_sub_selected()).collect();
        assert_eq!(subs.len(), 8);
        assert!(subs.contains(&"ranklist_hot_sc"));
        assert!(subs.contains(&"ranklist_must_watch"));
    }

    #[test]
    fn collect_walks_nested_cells() {
        let v: Value = serde_json::json!({ "a": { "video_data": [1] }, "b": [ { "video_data": [2] } ] });
        let mut out = Vec::new();
        collect_video_data(&v, &mut out);
        assert_eq!(out.len(), 2);
    }
}

#[cfg(test)]
pub(crate) mod probe {
    use super::*;

    fn anon_env() -> ApiEnv {
        ApiEnv::anonymous(crate::domain::model::ProxyConfig::default())
    }

    /// hgplayer 抓包里的已注册设备（bookmall 系在静态旧设备上报 110，
    /// 设备注册实现前用它验证端点形状）。
    pub fn hg_env(base: &ApiEnv) -> ApiEnv {
        let pairs: Vec<(&str, &str)> = vec![
            ("ac", "wifi"),
            ("aid", "8662"),
            ("app_name", "novelread"),
            ("cdid", "e9ca8ec4-bcbf-46e2-8e4c-281855bccaae"),
            ("channel", "xiaomi_8662_64"),
            ("device_brand", "xiaomi"),
            ("device_id", "2169800441471882"),
            ("device_platform", "android"),
            ("device_type", "23127PN0CC"),
            ("dpi", "460"),
            ("host_abi", "arm64-v8a"),
            ("iid", "2169800441475978"),
            ("language", "zh"),
            ("manifest_version_code", "73932"),
            ("os", "android"),
            ("os_api", "34"),
            ("os_version", "14"),
            ("resolution", "1200*2670"),
            ("ssmix", "a"),
            ("update_version_code", "73932"),
            ("version_code", "73932"),
            ("version_name", "7.3.9.32"),
        ];
        let mut v = serde_json::to_value(crate::signer::video_device()).unwrap();
        v["fields"] = serde_json::Value::Array(
            pairs
                .into_iter()
                .map(|(k, val)| serde_json::json!([k, val]))
                .collect(),
        );
        v["user_agent"] = serde_json::Value::String(
            "com.phoenix.read/73932 (Linux; U; Android 14; zh_CN; Xiaomi 14; Build/UKQ1.230804.001; Cronet/TTNetVersion:8d40f833 QuicVersion:462f352c 2026-08-31)".into(),
        );
        ApiEnv {
            proxy: base.proxy.clone(),
            device: serde_json::from_value(v).unwrap(),
            cookie: Some(
                "store-region=cn-gd; store-region-src=did; install_id=2169800441475978; ttreq=1$f814f969f9b9fe3c43ab008c4e981e84d23a6b4d".into(),
            ),
        }
    }

    /// 排行榜直连：8 个榜单逐个打，code==0 且条目数 >0 即转正。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_rank_all_lists() {
        let env = hg_env(&anon_env());
        for list in RankList::all() {
            match fetch_rank(list, &env).await {
                Ok(page) => {
                    let top = page.items.first();
                    println!(
                        "[rank/{:?}] {} 条, #1={:?} rank={:?} rec={:?}",
                        list,
                        page.items.len(),
                        top.map(|i| i.title.as_str()).unwrap_or(""),
                        top.map(|i| i.rank).unwrap_or(0),
                        top.map(|i| i.rec_text.as_str()).unwrap_or(""),
                    );
                    assert!(!page.items.is_empty(), "{list:?} 榜单不应为空");
                }
                Err(e) => panic!("[rank/{list:?}] ERR {e}"),
            }
        }
    }

    /// 新剧推荐直连。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_new_drama() {
        let env = hg_env(&anon_env());
        let page = fetch_new_drama(2, 0, &env).await.expect("新剧推荐");
        println!(
            "[new-drama] {} 条, #1={:?}",
            page.items.len(),
            page.items.first().map(|i| &i.title)
        );
        assert!(!page.items.is_empty());
    }

    /// 预约列表直连（匿名应返回空表 code==0）。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_reservations() {
        let env = anon_env();
        let page = fetch_reservations(true, &env).await.expect("预约列表");
        println!("[reservations] {} 条（匿名空表属正常）", page.items.len());
    }
}
