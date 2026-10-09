//! 排行榜接口（2026-10-03 mitmproxy 抓包锁定，详见 docs/hongguo-api-endpoints.md）。
//!
//! `GET /reading/bookapi/bookmall/cell/change/v`（**/v 无斜杠**），固定
//! `cell_id=7470092475068071998, tab_type=26, selected_items=all`，榜单用
//! `sub_selected_items` 切换；响应是 `data.cell_view.cell_data[].video_data[]`
//! （每块一条，`recommend_info` 是二次序列化 JSON 字符串，内含排名 `rank`）。
//!
//! bookmall 系对设备敏感（静态旧设备报 ILLEGAL_ACCESS 110），设备注册实现前，
//! 探测用例走 hgplayer 抓包里的已注册设备档案。

mod model;

use serde_json::Value;

use super::client::{ApiEnv, api_call_reading};
use super::danmaku::LQ_API_ORIGIN;
use super::discover::{check_code, int_field, num_field, str_field};
use crate::error::{AppError, AppResult};

pub use model::{RankItem, RankPage, RankPanelItem, RankPanelRow, RankSubList, RankTab};

/// 排行榜（书城 cell 换一换，/v 无斜杠）。
pub const RANK_CELL_PATH: &str = "/reading/bookapi/bookmall/cell/change/v";

/// 排行榜页的 8 个「全部」tab 子榜（probe 遍历用；生产路径的选项表由
/// 响应 `cell_selector` schema 下发，前端直接用字符串 id）。
#[cfg(test)]
pub(crate) const ALL_TAB_SUBS: &[&str] = &[
    "ranklist_hot_sc",
    "ranklist_hot_play_sc",
    "ranklist_prestige",
    "ranklist_subscribe",
    "ranklist_new_rank_sc",
    "ranklist_hot_search_sc",
    "ranklist_must_watch",
    "ranklist_followed",
];

/// 拉任意 tab × 子榜 × 筛选组合的榜单。
///
/// - `selected`：内容 tab 的 `selected_items`（all/human/comic_series_rank/
///   ai_playlet/series_album…）
/// - `sub`：子榜的 `sub_selected_items`（ranklist_hot_sc/human_hot_sc…）
/// - `panel`：筛选面板选中项 `panel_selected_items`（gender_female /
///   cate_308 / style_1685…；单值——hgplayer 抓包实测每次点击整组替换）
/// - `offset` / `session_id`：翻页游标（**2026-10-09 抓 hgplayer 滚动榜单实锤**：首页 offset=0 不带 session_id，响应下发 next_offset（步进 10）+ session_id；翻页 offset=next_offset 并**回传同一 session_id**，其余参数原样。每页下发 20 条、相邻页重叠 10 条——客户端按 seriesId 去重；limit 恒 "0" 不参与分页，页长服务端固定）
pub async fn fetch_rank_ex(
    selected: &str,
    sub: &str,
    panel: Option<&str>,
    offset: i64,
    session_id: &str,
    env: &ApiEnv,
) -> AppResult<RankPage> {
    let mut q: Vec<(String, String)> = [
        ("cell_id", "7470092475068071998"),
        ("tab_type", "26"),
        ("selected_items", selected),
        ("category_id", "0"),
        ("cell_sub_id", "0"),
        ("client_req_type", "2"),
        ("client_template", "2"),
        ("gender", "2"),
        // 抓包原样：limit 恒 "0"（页长服务端固定 20，limit 参数不参与）
        ("limit", "0"),
        ("offset", &offset.to_string()),
        ("sub_selected_items", sub),
        // 面板 schema（unlimited_selector）随请求下发，抓包恒带 2
        ("unlimited_selector_change_type", "2"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    if let Some(p) = panel.filter(|p| !p.is_empty()) {
        q.push(("panel_selected_items".to_string(), p.to_string()));
    }
    if !session_id.is_empty() {
        q.push(("session_id".to_string(), session_id.to_string()));
    }
    let bytes = api_call_reading(LQ_API_ORIGIN, RANK_CELL_PATH, None, &q, env).await?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析榜单失败: {e}")))?;
    check_code(&value)?;
    let data = value.get("data");
    Ok(RankPage {
        items: parse_rank_items(data)?,
        tabs: parse_cell_selector(data),
        has_more: data
            .and_then(|d| d.get("has_more"))
            .and_then(Value::as_bool)
            .unwrap_or(false),
        next_offset: data
            .and_then(|d| d.get("next_offset"))
            .and_then(Value::as_i64)
            .unwrap_or(0),
        session_id: data
            .and_then(|d| d.get("session_id"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
    })
}

/// 展开响应 `data.cell_view.cell_selector` 的 tab → 子榜 → 面板选项表。
///
/// 抓包结构（2026-10-05 captures/rank-selector-schema.json）：
/// `outer_row.items[]`（内容 tab，selector_item_id）→ `sub_cell_selector.
/// outer_row.items[]`（子榜，sub_title 为描述行）→ `panel_selector.
/// inner_rows[]`（行 row_name + items[] 选项，selection_type=1 单选）。
///
/// 形态由**客户端版本号**决定（probe_rank_login_form 二分实证，与登录态
/// 无关）：73932（7.3.9，hgplayer 1.1.3 内置红果版本）下发两级全量结构；
/// 71332 老版本身份退化成扁平一级结构（无「全部」tab、面板分组丢失）。
/// 设备档案已在启动时对齐版本（[`crate::signer::device::align_app_version`]），
/// 一级形态的兼容归一保留在前端 normalizeTabs 里。
pub(super) fn parse_cell_selector(data: Option<&Value>) -> Vec<RankTab> {
    let Some(tabs) = data
        .and_then(|d| d.get("cell_view"))
        .and_then(|cv| cv.get("cell_selector"))
        .and_then(|cs| cs.get("outer_row"))
        .and_then(|r| r.get("items"))
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };
    tabs.iter()
        .filter_map(|tab| {
            let id = str_field(tab, "selector_item_id");
            if id.is_empty() {
                return None;
            }
            let subs = tab
                .get("sub_cell_selector")
                .and_then(|s| s.get("outer_row"))
                .and_then(|r| r.get("items"))
                .and_then(Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .filter_map(|sub| {
                            let sid = str_field(sub, "selector_item_id");
                            // 一级形态（老版本身份）下 sub 层是筛选选项，
                            // 「总榜」的 id 为空但必须保留（前端靠它渲染回退选项）
                            if sid.is_empty() && str_field(sub, "show_name") != "总榜" {
                                return None;
                            }
                            let panel = sub
                                .get("panel_selector")
                                .and_then(|p| p.get("inner_rows"))
                                .and_then(Value::as_array)
                                .map(|rows| {
                                    rows.iter()
                                        .filter_map(|row| {
                                            let items: Vec<RankPanelItem> = row
                                                .get("items")
                                                .and_then(Value::as_array)
                                                .map(|arr| {
                                                    arr.iter()
                                                        .map(|it| RankPanelItem {
                                                            id: str_field(it, "selector_item_id"),
                                                            name: str_field(it, "show_name"),
                                                        })
                                                        .collect()
                                                })
                                                .unwrap_or_default();
                                            if items.is_empty() {
                                                return None;
                                            }
                                            Some(RankPanelRow {
                                                name: str_field(row, "row_name"),
                                                items,
                                            })
                                        })
                                        .collect()
                                })
                                .unwrap_or_default();
                            Some(RankSubList {
                                id: sid,
                                name: str_field(sub, "show_name"),
                                description: str_field(sub, "sub_title"),
                                panel,
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            Some(RankTab {
                name: str_field(tab, "show_name"),
                id,
                subs,
            })
        })
        .collect()
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
pub(super) fn parse_rank_items(data: Option<&Value>) -> AppResult<Vec<RankItem>> {
    let data = data.ok_or_else(|| AppError::Media("响应缺少 data".into()))?;
    let mut raws = Vec::new();
    collect_video_data(data, &mut raws);

    let mut items = Vec::new();
    for raw in &raws {
        let Some(series_id) = raw
            .get("series_id")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
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
            // 未上线判定：预约榜条目分集数为 0（已上线剧恒 >0；2026-10-09
            // 抓包预约榜条目 episode_cnt=0/play_cnt=0，已上线条目 132 集）
            upcoming: int_field(raw, "episode_cnt") == 0,
            reserved: raw
                .get("online_subscribed")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            season: raw
                .get("sub_title_list")
                .and_then(Value::as_array)
                .and_then(|a| {
                    a.iter()
                        .filter_map(|x| x.get("content").and_then(Value::as_str))
                        .find(|c| c.starts_with('第') && c.ends_with("季"))
                        .map(str::to_string)
                })
                .unwrap_or_default(),
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

    /// 抓包样本（2026-10-05 cell_selector）：tab → 子榜 → 面板行三层展开。
    #[test]
    fn parses_cell_selector_schema() {
        let data: Value = serde_json::json!({
            "cell_view": {
                "cell_selector": {
                    "outer_row": {
                        "items": [
                            {
                                "show_name": "全部",
                                "selector_item_id": "all",
                                "sub_cell_selector": { "outer_row": { "items": [
                                    {
                                        "show_name": "推荐榜",
                                        "selector_item_id": "ranklist_hot_sc",
                                        "sub_title": "10月4日已更新·基于红果观看/互动以及个人兴趣排序",
                                        "panel_selector": { "inner_rows": [
                                            {
                                                "row_name": "综合",
                                                "selection_type": 1,
                                                "items": [
                                                    { "show_name": "总榜", "selector_item_id": "" },
                                                    { "show_name": "女频", "selector_item_id": "gender_female", "is_selected": false },
                                                    { "show_name": "男频", "selector_item_id": "gender_male" }
                                                ]
                                            },
                                            {
                                                "row_name": "时代背景",
                                                "items": [
                                                    { "show_name": "古装", "selector_item_id": "cate_308" },
                                                    { "show_name": "校园", "selector_item_id": "cate_4" }
                                                ]
                                            }
                                        ] }
                                    },
                                    { "show_name": "空面板子榜", "selector_item_id": "ranklist_subscribe" }
                                ] }}
                            },
                            {
                                "show_name": "漫剧",
                                "selector_item_id": "comic_series_rank",
                                "sub_cell_selector": { "outer_row": { "items": [
                                    {
                                        "show_name": "推荐榜",
                                        "selector_item_id": "comic_series_hot_rank",
                                        "panel_selector": { "inner_rows": [
                                            { "row_name": "画风", "items": [
                                                { "show_name": "3d", "selector_item_id": "style_1685" }
                                            ] }
                                        ] }
                                    }
                                ] }}
                            },
                            { "show_name": "无 id 项", "selector_item_id": "" }
                        ]
                    }
                }
            }
        });
        let tabs = parse_cell_selector(Some(&data));
        assert_eq!(tabs.len(), 2, "空 id 的 tab 跳过");
        assert_eq!(tabs[0].id, "all");
        assert_eq!(tabs[0].name, "全部");
        assert_eq!(tabs[0].subs.len(), 2);
        let sub = &tabs[0].subs[0];
        assert_eq!(sub.id, "ranklist_hot_sc");
        assert_eq!(
            sub.description, "10月4日已更新·基于红果观看/互动以及个人兴趣排序",
            "子榜描述行（sub_title）供官方同款排版用"
        );
        assert_eq!(sub.panel.len(), 2);
        assert_eq!(sub.panel[0].name, "综合");
        // 「总榜」id 为空 = 清除筛选，也要在场（前端靠它渲染回退选项）
        assert_eq!(sub.panel[0].items[0].name, "总榜");
        assert_eq!(sub.panel[0].items[0].id, "");
        assert_eq!(sub.panel[0].items[1].id, "gender_female");
        assert_eq!(sub.panel[1].items[0].id, "cate_308");
        assert!(tabs[0].subs[1].panel.is_empty(), "无面板的子榜收空表");
        assert_eq!(tabs[1].subs[0].panel[0].items[0].id, "style_1685");
        // 无 cell_selector 的响应（旧缓存/异常）不报错
        assert!(parse_cell_selector(Some(&serde_json::json!({}))).is_empty());
    }

    /// 一级形态（老版本身份的退化结构，2026-10-05 probe 二分实证：由
    /// version_code 决定、与登录态无关）：outer_row 直接是榜单，
    /// sub_cell_selector 是**筛选选项**（女频/男频/题材…，无 panel 层），
    /// 「总榜」选项 id 为空但要保留。
    #[test]
    fn parses_flat_login_selector_schema() {
        let data: Value = serde_json::json!({
            "cell_view": {
                "cell_selector": {
                    "outer_row": {
                        "items": [
                            {
                                "show_name": "推荐榜",
                                "selector_item_id": "ranklist_hot_sc",
                                "sub_cell_selector": { "outer_row": { "items": [
                                    { "show_name": "总榜", "selector_item_id": "" },
                                    { "show_name": "女频", "selector_item_id": "gender_female" },
                                    { "show_name": "男频", "selector_item_id": "gender_male" },
                                    { "show_name": "古装", "selector_item_id": "cate_308" }
                                ] }}
                            },
                            {
                                "show_name": "热搜榜",
                                "selector_item_id": "ranklist_hot_search_sc",
                                "sub_cell_selector": null
                            }
                        ]
                    }
                }
            }
        });
        let tabs = parse_cell_selector(Some(&data));
        assert_eq!(tabs.len(), 2);
        assert_eq!(tabs[0].id, "ranklist_hot_sc");
        assert_eq!(tabs[0].name, "推荐榜");
        assert_eq!(tabs[0].subs.len(), 4, "空 id 的「总榜」要保留");
        assert_eq!(tabs[0].subs[0].id, "");
        assert_eq!(tabs[0].subs[0].name, "总榜");
        assert_eq!(tabs[0].subs[1].id, "gender_female");
        assert!(
            tabs[0].subs.iter().all(|s| s.panel.is_empty()),
            "一级形态无 panel 层"
        );
        assert!(tabs[1].subs.is_empty(), "无 sub_cell_selector 收空表");
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
    fn collect_walks_nested_cells() {
        let v: Value =
            serde_json::json!({ "a": { "video_data": [1] }, "b": [ { "video_data": [2] } ] });
        let mut out = Vec::new();
        collect_video_data(&v, &mut out);
        assert_eq!(out.len(), 2);
    }
}

/// 直连真实接口的探测用例（全部 `#[ignore]`，用后即删）。本模块的
/// `anon_env` / `hg_env` 同时供 recommend / new_drama / reservation /
/// calendar 各域 probe 复用。
#[cfg(test)]
pub(crate) mod probe {
    use super::*;

    pub(crate) fn anon_env() -> ApiEnv {
        ApiEnv::anonymous(crate::domain::model::ProxyConfig::default())
    }

    /// hgplayer 抓包里的已注册设备（bookmall 系在静态旧设备上报 110，
    /// 设备注册实现前用它验证端点形状）。
    ///
    /// 2026-10-04 更新：旧档案（device_id=1694…）当日搜索开始回 0 字节——
    /// 服务端按 install_id 风控，旧 id 已失效；换当日 hgplayer 新注册的
    /// 设备（captures/flows-20261004.jsonl）后恢复。
    pub fn hg_env(base: &ApiEnv) -> ApiEnv {
        let pairs: Vec<(&str, &str)> = vec![
            ("ac", "wifi"),
            ("aid", "8662"),
            ("app_name", "novelread"),
            ("cdid", "babf8a0e-8586-40ca-bb75-20a1a698b43f"),
            ("channel", "xiaomi_8662_64"),
            ("device_brand", "xiaomi"),
            ("device_id", "2715158266032906"),
            ("device_platform", "android"),
            ("device_type", "23127PN0CC"),
            ("dpi", "460"),
            ("host_abi", "arm64-v8a"),
            ("iid", "2715158266282762"),
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
                "store-region=cn-gd; store-region-src=did; install_id=2715158266282762; ttreq=1$5c6c7c7cd605c0a9f176a0533d9c64755b5df874".into(),
            ),
            x_tt_token: None,
        }
    }

    /// 临时探测：榜单 cell 的 limit/offset 分页行为（用后即删）。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_rank_paging() {
        let device_json = std::env::var("PROBE_DEVICE").expect("PROBE_DEVICE");
        let mut device: crate::signer::device::DeviceProfile =
            serde_json::from_str(&device_json).expect("device json");
        crate::signer::device::align_app_version(&mut device);
        let env = ApiEnv {
            proxy: crate::domain::model::ProxyConfig::default(),
            device,
            cookie: None,
            x_tt_token: None,
        };
        for (limit, offset) in [(0i64, 0i64), (50, 0), (20, 20), (20, 40)] {
            let q: Vec<(String, String)> = [
                ("cell_id", "7470092475068071998"),
                ("tab_type", "26"),
                ("selected_items", "all"),
                ("category_id", "0"),
                ("cell_sub_id", "0"),
                ("client_req_type", "2"),
                ("client_template", "2"),
                ("gender", "2"),
                ("limit", &limit.to_string()),
                ("offset", &offset.to_string()),
                ("sub_selected_items", "ranklist_subscribe"),
                ("unlimited_selector_change_type", "2"),
            ]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
            match api_call_reading(LQ_API_ORIGIN, RANK_CELL_PATH, None, &q, &env).await {
                Ok(bytes) => {
                    let v: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
                    let data = v.get("data");
                    let count = data
                        .map(|d| {
                            d.to_string().matches("series_id").count()
                        })
                        .unwrap_or(0);
                    println!(
                        "[limit={limit} offset={offset}] keys={:?} next_offset={:?} has_more={:?} series_id_hits={count}",
                        data.map(|d| d.as_object().map(|o| o.keys().cloned().collect::<Vec<_>>()).unwrap_or_default()).unwrap_or_default(),
                        data.and_then(|d| d.get("next_offset")).cloned(),
                        data.and_then(|d| d.get("has_more")).cloned(),
                    );
                }
                Err(e) => println!("[limit={limit} offset={offset}] ERR {e}"),
            }
        }
    }

    /// 排行榜筛选面板 schema 直连：dump 完整响应到 captures/，供提取
    /// `unlimited_selector`（panel_selected_items 的选项 id 表，抓包响应被
    /// addon 截断拿不全）。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_rank_selector_schema() {
        let env = hg_env(&anon_env());
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
            ("sub_selected_items", "ranklist_hot_sc"),
            ("unlimited_selector_change_type", "2"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        let bytes = api_call_reading(LQ_API_ORIGIN, RANK_CELL_PATH, None, &q, &env)
            .await
            .expect("排行榜请求");
        let out = std::env::current_dir()
            .ok()
            .map(|d| d.join("../captures/rank-selector-schema.json"))
            .unwrap_or_else(|| std::path::PathBuf::from("captures/rank-selector-schema.json"));
        std::fs::write(&out, &bytes).expect("写 captures/rank-selector-schema.json");
        println!("[rank-selector] {} bytes -> {}", bytes.len(), out.display());
    }

    /// 排行榜 cell_selector 形态二分：服务端是否按客户端版本号分发
    /// 两级（内容 tab × 子榜 × 分组面板）/ 一级（扁平榜单）结构。
    /// 用法：PROBE_DEVICE / PROBE_COOKIE 由真实库导出，
    /// `PROBE_VARIANT=asis|v73932 cargo test probe_rank_login_form -- --ignored --nocapture`
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_rank_login_form() {
        let device_json = std::env::var("PROBE_DEVICE").expect("PROBE_DEVICE");
        let account_cookie = std::env::var("PROBE_COOKIE").expect("PROBE_COOKIE");
        let variant = std::env::var("PROBE_VARIANT").unwrap_or_else(|_| "asis".into());

        let mut v: Value = serde_json::from_str(&device_json).expect("device json");
        if variant == "v73932" {
            let bump = [
                ["version_code", "73932"],
                ["version_name", "7.3.9.32"],
                ["manifest_version_code", "73932"],
                ["update_version_code", "73932"],
            ];
            let fields = v["fields"].as_array_mut().expect("fields");
            for [k, val] in bump {
                for f in fields.iter_mut() {
                    if f[0].as_str() == Some(k) {
                        f[1] = serde_json::json!(val);
                    }
                }
            }
        }
        let device: crate::signer::device::DeviceProfile = serde_json::from_value(v).unwrap();
        let cookie = format!(
            "{}; {}",
            crate::signer::device::anonymous_cookie(&device),
            account_cookie
        );
        let env = ApiEnv {
            proxy: crate::domain::model::ProxyConfig::default(),
            device,
            cookie: Some(cookie),
            x_tt_token: None,
        };

        let page = fetch_rank_ex("all", "ranklist_hot_sc", None, 0, "", &env)
            .await
            .expect("榜单请求");
        println!("[{variant}] tabs={} 第一层 id:", page.tabs.len());
        for tab in &page.tabs {
            let subs: Vec<&str> = tab.subs.iter().map(|s| s.name.as_str()).collect();
            println!("  {:?} {:?} subs={:?}", tab.id, tab.name, subs);
        }
        println!(
            "[{variant}] items={} #1={:?}",
            page.items.len(),
            page.items.first().map(|i| i.title.as_str())
        );
    }

    /// 排行榜 cell_selector 形态二分（补充）：老版本号 + 匿名（无会话），
    /// 区分「版本号」与「登录会话」哪个才是 schema 分发开关。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_rank_login_form_anon() {
        let variant = std::env::var("PROBE_VARIANT").unwrap_or_else(|_| "asis".into());
        let device_json = std::env::var("PROBE_DEVICE").expect("PROBE_DEVICE");
        let mut v: Value = serde_json::from_str(&device_json).expect("device json");
        if variant == "v73932" {
            let fields = v["fields"].as_array_mut().expect("fields");
            for f in fields.iter_mut() {
                match f[0].as_str() {
                    Some("version_code" | "manifest_version_code" | "update_version_code") => {
                        f[1] = serde_json::json!("73932");
                    }
                    Some("version_name") => f[1] = serde_json::json!("7.3.9.32"),
                    _ => {}
                }
            }
        }
        let device: crate::signer::device::DeviceProfile = serde_json::from_value(v).unwrap();
        let env = ApiEnv {
            proxy: crate::domain::model::ProxyConfig::default(),
            cookie: Some(crate::signer::device::anonymous_cookie(&device)),
            device,
            x_tt_token: None,
        };
        let page = fetch_rank_ex("all", "ranklist_hot_sc", None, 0, "", &env)
            .await
            .expect("榜单请求");
        println!("[anon/{variant}] tabs={} 第一层 id:", page.tabs.len());
        for tab in &page.tabs {
            let subs: Vec<&str> = tab.subs.iter().map(|s| s.name.as_str()).collect();
            println!("  {:?} {:?} subs={:?}", tab.id, tab.name, subs);
        }
    }

    /// 排行榜内容 tab「演员」形态验证（ranklist_celebrity，抓包没点到，
    /// 响应 cell_data 若不是剧集形态则前端跳过该 tab）。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_rank_celebrity() {
        let env = hg_env(&anon_env());
        let q: Vec<(String, String)> = [
            ("cell_id", "7470092475068071998"),
            ("tab_type", "26"),
            ("selected_items", "ranklist_celebrity"),
            ("category_id", "0"),
            ("cell_sub_id", "0"),
            ("client_req_type", "2"),
            ("client_template", "2"),
            ("gender", "2"),
            ("limit", "0"),
            ("offset", "0"),
            ("unlimited_selector_change_type", "2"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        let bytes = api_call_reading(LQ_API_ORIGIN, RANK_CELL_PATH, None, &q, &env)
            .await
            .expect("演员榜请求");
        let v: Value = serde_json::from_slice(&bytes).expect("JSON");
        println!("[celebrity] code = {:?}", v.get("code"));
        let data = &v["data"];
        println!(
            "[celebrity] data keys = {:?}",
            data.as_object().map(|m| m.keys().collect::<Vec<_>>())
        );
        let mut raws = Vec::new();
        collect_video_data(data, &mut raws);
        println!("[celebrity] video_data 条数 = {}", raws.len());
        if let Some(first) = raws.first() {
            println!(
                "[celebrity] 首条 keys = {:?}",
                first
                    .as_object()
                    .map(|m| m.keys().take(20).collect::<Vec<_>>())
            );
            println!(
                "[celebrity] title = {:?}, series_id = {:?}, sub_title = {:?}",
                first.get("title").and_then(Value::as_str),
                first.get("series_id").and_then(Value::as_str),
                first.get("sub_title").and_then(Value::as_str),
            );
        }
    }

    /// 筛选面板参数真连：同一子榜，总榜 vs 女频 vs 题材，条目应变化且带 tabs schema。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_rank_panel_filter() {
        let env = hg_env(&anon_env());
        let base = fetch_rank_ex("all", "ranklist_hot_sc", None, 0, "", &env)
            .await
            .expect("总榜");
        println!(
            "[panel:总榜] {} 条 #1={:?} tabs={}",
            base.items.len(),
            base.items.first().map(|i| i.title.as_str()),
            base.tabs.len()
        );
        assert!(!base.items.is_empty());
        assert_eq!(base.tabs[0].id, "all", "tabs schema 应随行下发");

        for p in ["gender_female", "gender_male", "cate_308"] {
            let page = fetch_rank_ex("all", "ranklist_hot_sc", Some(p), 0, "", &env)
                .await
                .unwrap_or_else(|e| panic!("panel={p}: {e}"));
            println!(
                "[panel:{p}] {} 条 #1={:?}",
                page.items.len(),
                page.items.first().map(|i| i.title.as_str())
            );
            assert!(!page.items.is_empty(), "panel={p} 不应为空");
        }

        // 内容 tab 切换（真人剧 / 漫剧 / 系列剧，抓包样本 id）
        for (sel, sub) in [
            ("human", "human_hot_sc"),
            ("comic_series_rank", "comic_series_hot_rank"),
            ("series_album", "series_album_hot_sc"),
        ] {
            let page = fetch_rank_ex(sel, sub, None, 0, "", &env)
                .await
                .unwrap_or_else(|e| panic!("{sel}/{sub}: {e}"));
            println!(
                "[tab:{sel}] {} 条 #1={:?}",
                page.items.len(),
                page.items.first().map(|i| i.title.as_str())
            );
            assert!(!page.items.is_empty(), "{sel}/{sub} 不应为空");
        }
    }

    /// app 生产同款设备（video_device + 匿名 cookie）下发的 selector schema：
    /// 对照 hgplayer 档案——排查两端 tabs 是否一致（灰度可能按设备分发）。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_rank_selector_app_device() {
        let device = crate::signer::video_device();
        let env = ApiEnv {
            proxy: crate::domain::model::ProxyConfig::default(),
            cookie: Some(crate::signer::device::anonymous_cookie(&device)),
            device,
            x_tt_token: None,
        };
        let page = fetch_rank_ex("all", "ranklist_hot_sc", None, 0, "", &env)
            .await
            .expect("app 同款设备榜单");
        println!(
            "[app-device:tabs] {:?}",
            page.tabs
                .iter()
                .map(|t| (t.id.as_str(), t.name.as_str(), t.subs.len()))
                .collect::<Vec<_>>()
        );
        println!(
            "[app-device:subs] {:?}",
            page.tabs[0]
                .subs
                .iter()
                .map(|s| (s.id.as_str(), s.name.as_str(), s.panel.len()))
                .collect::<Vec<_>>()
        );
    }

    /// 排行榜直连：8 个「全部」tab 子榜逐个打，code==0 且条目数 >0 即转正。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_rank_all_lists() {
        let env = hg_env(&anon_env());
        for sub in ALL_TAB_SUBS {
            match fetch_rank_ex("all", sub, None, 0, "", &env).await {
                Ok(page) => {
                    let top = page.items.first();
                    println!(
                        "[rank/{sub}] {} 条, #1={:?} rank={:?} rec={:?}",
                        page.items.len(),
                        top.map(|i| i.title.as_str()).unwrap_or(""),
                        top.map(|i| i.rank).unwrap_or(0),
                        top.map(|i| i.rec_text.as_str()).unwrap_or(""),
                    );
                    assert!(!page.items.is_empty(), "{sub} 榜单不应为空");
                }
                Err(e) => panic!("[rank/{sub}] ERR {e}"),
            }
        }
    }
}
