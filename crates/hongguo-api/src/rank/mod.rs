//! 排行榜接口（端点口径见 docs/hongguo-api-endpoints.md）。
//!
//! bookmall 系对设备敏感（静态旧设备报 ILLEGAL_ACCESS 110），探测用例
//! 需要注册态身份（见本模块 `probe::hg_env`）。

mod model;

use serde_json::Value;

use super::client::{ApiEnv, api_call_reading};
use super::danmaku::LQ_API_ORIGIN;
use super::discover::{check_code, int_field, num_field, str_field};
use hongguo_common::{AppError, AppResult};

pub use model::{RankItem, RankPage, RankPanelItem, RankPanelRow, RankSubList, RankTab};

/// 排行榜（书城 cell 换一换）。
pub const RANK_CELL_PATH: &str = "/reading/bookapi/bookmall/cell/change/v";

/// 排行榜页的 8 个「全部」tab 子榜（probe 遍历用）。
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

/// 一次榜单请求的定位参数（内容 tab × 子榜 × 筛选 × 月榜 + 翻页游标），
/// 字段与官方 query 一一对应。
#[derive(Debug, Clone, Default)]
pub struct RankReq<'a> {
    pub selected: &'a str,
    pub sub: &'a str,
    pub panel: Option<&'a str>,
    pub background: Option<&'a str>,
    pub offset: i64,
    pub session_id: &'a str,
    pub rank_version: &'a str,
}

/// `unlimited_selector_change_type`：首屏=2，翻页续载=1（续载带 2 会被
/// 服务端当「换一批」处理）。
fn selector_change_type(req: &RankReq<'_>) -> &'static str {
    if req.offset == 0 && req.session_id.is_empty() {
        "2"
    } else {
        "1"
    }
}

/// 对外窗口大小（每次返回会话榜单的 10 条切片）。
const RANK_WINDOW: usize = 10;

/// 单次请求最多向上游补的页数（护栏：上游 has_more 异常常真时防死循环）。
const RANK_FILL_PAGES_MAX: usize = 5;

/// 会话账本里会话数的上限（超出整体重置）。
const RANK_BOOK_SESSIONS: usize = 32;

/// 一个浏览会话的累积榜单视图。
#[derive(Default)]
struct RankSessionBook {
    /// 累积条目（官方榜单序）
    items: Vec<RankItem>,
    /// 上游会话 id（空 = 尚未向上游开过会话）
    upstream_session: String,
    /// 最近一次上游响应下发的版本戳
    rank_version: String,
    /// 上游是否还有更多
    upstream_has_more: bool,
    /// 上游续载游标（= 最近一次响应的 next_offset）
    upstream_offset: i64,
    /// 选项表（每次响应都随行下发，存最新一份）
    tabs: Vec<RankTab>,
}

fn session_books() -> &'static std::sync::Mutex<std::collections::HashMap<String, RankSessionBook>>
{
    static BOOKS: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<String, RankSessionBook>>,
    > = std::sync::OnceLock::new();
    BOOKS.get_or_init(std::sync::Mutex::default)
}

/// 合并一页上游条目进账本：按 series_id 去重，保持交付序。
fn merge_into_book(book: &mut RankSessionBook, items: Vec<RankItem>) {
    let seen: std::collections::HashSet<String> =
        book.items.iter().map(|i| i.series_id.clone()).collect();
    book.items
        .extend(items.into_iter().filter(|i| !seen.contains(&i.series_id)));
}

/// 会话榜单的 `[offset, offset+10)` 窗口切片。
fn book_window(book: &RankSessionBook, offset: usize) -> (Vec<RankItem>, bool) {
    let end = (offset + RANK_WINDOW).min(book.items.len());
    let items = book.items[offset.min(end)..end].to_vec();
    let has_more = book.items.len() > end || book.upstream_has_more;
    (items, has_more)
}

struct UpstreamPage {
    items: Vec<RankItem>,
    tabs: Vec<RankTab>,
    session: String,
    rank_version: String,
    has_more: bool,
    next_offset: i64,
}

/// 打一页上游（首屏无会话，续载带 session + filter_ids）。
async fn fetch_upstream(
    selected: &str,
    req: &RankReq<'_>,
    filter_ids: &str,
    env: &ApiEnv,
) -> AppResult<UpstreamPage> {
    let mut q: Vec<(String, String)> = [
        ("cell_id", "7470092475068071998"),
        ("tab_type", "26"),
        ("selected_items", selected),
        ("category_id", "0"),
        ("cell_sub_id", "0"),
        ("client_req_type", "2"),
        ("client_template", "2"),
        ("gender", "2"),
        ("limit", "0"),
        ("need_personal_recommend", "1"),
        ("offset", &req.offset.to_string()),
        ("sub_selected_items", req.sub),
        ("unlimited_selector_change_type", selector_change_type(req)),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    if let Some(p) = req.panel.filter(|p| !p.is_empty()) {
        q.push(("panel_selected_items".to_string(), p.to_string()));
    }
    if let Some(b) = req.background.filter(|b| !b.is_empty()) {
        q.push(("background_selected_items".to_string(), b.to_string()));
    }
    if !req.rank_version.is_empty() {
        q.push(("rank_version".to_string(), req.rank_version.to_string()));
    }
    if !req.session_id.is_empty() {
        q.push(("session_id".to_string(), req.session_id.to_string()));
        if !filter_ids.is_empty() {
            q.push(("filter_ids".to_string(), filter_ids.to_string()));
        }
    }
    let bytes = api_call_reading(LQ_API_ORIGIN, RANK_CELL_PATH, None, &q, env).await?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析榜单失败: {e}")))?;
    check_code(&value)?;
    let data = value.get("data");
    Ok(UpstreamPage {
        items: parse_rank_items(data)?,
        tabs: parse_cell_selector(data),
        session: data
            .and_then(|d| d.get("session_id"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        rank_version: rank_version_field(data).unwrap_or_default(),
        has_more: data
            .and_then(|d| d.get("has_more"))
            .and_then(Value::as_bool)
            .unwrap_or(false),
        next_offset: data
            .and_then(|d| d.get("next_offset"))
            .and_then(Value::as_i64)
            .unwrap_or(0),
    })
}

fn fallback_session() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    format!("local-{}", now.as_millis())
}

fn store_book(session_id: &str, book: RankSessionBook) {
    let mut books = session_books().lock().unwrap_or_else(|p| p.into_inner());
    if !books.contains_key(session_id) && books.len() >= RANK_BOOK_SESSIONS {
        books.clear();
    }
    books.insert(session_id.to_string(), book);
}

/// 拉任意组合的榜单窗口。**对外契约是窗口而非上游页**：同一
/// (session, offset) 重复请求恒得同一批条目——后端持有累积账本，
/// 窗口从账本切片（幂等），账本不够深才按官方协议顺序向上游补页。
pub async fn fetch_rank_ex(req: RankReq<'_>, env: &ApiEnv) -> AppResult<RankPage> {
    let selected = if req.selected.trim().is_empty() {
        "all"
    } else {
        req.selected
    };
    let offset = req.offset.max(0) as usize;
    let mut session = req.session_id.to_string();

    // 无会话或账本已被清：开新会话落账本，键沿用请求带来的 session
    let known = !req.session_id.is_empty() && {
        session_books()
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .contains_key(req.session_id)
    };
    if !known {
        let up = fetch_upstream(
            selected,
            &RankReq {
                session_id: "",
                ..req.clone()
            },
            "",
            env,
        )
        .await?;
        session = if req.session_id.is_empty() {
            if up.session.is_empty() {
                fallback_session()
            } else {
                up.session.clone()
            }
        } else {
            req.session_id.to_string()
        };
        let book = RankSessionBook {
            upstream_session: up.session,
            rank_version: up.rank_version,
            upstream_has_more: up.has_more,
            upstream_offset: up.next_offset,
            tabs: up.tabs,
            items: up.items,
        };
        store_book(&session, book);
    }

    // 补深到窗口深度：短锁读态 → 无锁拉上游 → 短锁合并（锁绝不跨 await）
    for _ in 0..RANK_FILL_PAGES_MAX {
        let (need_fill, up_session, up_rv, up_offset, filter_ids) = {
            let books = session_books().lock().unwrap_or_else(|p| p.into_inner());
            let book = books
                .get(&session)
                .ok_or_else(|| AppError::Media("会话账本丢失".into()))?;
            let mut fid = book
                .items
                .iter()
                .map(|i| i.series_id.as_str())
                .collect::<Vec<_>>()
                .join(",");
            fid.push(',');
            (
                book.items.len() < offset + RANK_WINDOW && book.upstream_has_more,
                book.upstream_session.clone(),
                book.rank_version.clone(),
                book.upstream_offset,
                fid,
            )
        };
        if !need_fill {
            break;
        }
        let up = fetch_upstream(
            selected,
            &RankReq {
                offset: up_offset,
                session_id: &up_session,
                rank_version: &up_rv,
                ..req.clone()
            },
            &filter_ids,
            env,
        )
        .await?;
        let mut books = session_books().lock().unwrap_or_else(|p| p.into_inner());
        if let Some(book) = books.get_mut(&session) {
            if !up.session.is_empty() {
                book.upstream_session = up.session;
            }
            if !up.rank_version.is_empty() {
                book.rank_version = up.rank_version;
            }
            book.upstream_has_more = up.has_more;
            book.upstream_offset = up.next_offset;
            if !up.tabs.is_empty() {
                book.tabs = up.tabs;
            }
            merge_into_book(book, up.items);
        }
    }

    // 窗口应答（账本切片，幂等）
    let books = session_books().lock().unwrap_or_else(|p| p.into_inner());
    let book = books
        .get(&session)
        .ok_or_else(|| AppError::Media("会话账本丢失".into()))?;
    let (items, has_more) = book_window(book, offset);
    Ok(RankPage {
        next_offset: if has_more {
            (offset + items.len()) as i64
        } else {
            0
        },
        items,
        has_more,
        tabs: book.tabs.clone(),
        session_id: session,
        rank_version: book.rank_version.clone(),
    })
}

/// `rank_version` 的容错提取（响应是 JSON 数字，请求回传字符串形态）。
fn rank_version_field(data: Option<&Value>) -> Option<String> {
    let v = data?.get("rank_version")?;
    v.as_str()
        .map(str::to_string)
        .or_else(|| v.as_i64().map(|n| n.to_string()))
}

/// selector 面板行展开（panel_selector / background_selector 同构共用）。
fn parse_selector_rows(rows: &[Value]) -> Vec<RankPanelRow> {
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
}

/// 展开 `data.cell_view.cell_selector` 的 tab → 子榜 → 面板选项表。
///
/// 结构形态由客户端自报版本号决定：较新版本是两级全量结构，过旧版本
/// 退化成扁平一级结构（一级形态的兼容归一在前端 normalizeTabs）。
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
                            // 空 id 但带 panel_selector 的是异常二级子榜；
                            // 空 id 叶子是「全部」哨兵，必须保留
                            if sid.is_empty() && sub.get("panel_selector").is_some() {
                                return None;
                            }
                            let panel = sub
                                .get("panel_selector")
                                .and_then(|p| p.get("inner_rows"))
                                .and_then(Value::as_array)
                                .map(|rows| parse_selector_rows(rows))
                                .unwrap_or_default();
                            let background = sub
                                .get("background_selector")
                                .and_then(|p| p.get("inner_rows"))
                                .and_then(Value::as_array)
                                .map(|rows| parse_selector_rows(rows))
                                .unwrap_or_default();
                            Some(RankSubList {
                                id: sid,
                                name: str_field(sub, "show_name"),
                                description: str_field(sub, "sub_title"),
                                panel,
                                background,
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

/// 遍历 `data` 下所有 `video_data` 数组（cell_data 嵌套结构逐块平铺）。
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
            // 预约榜条目分集数为 0（已上线剧恒 >0）
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
    // 交付序就是官方榜单序，绝不能按 recommend_info.rank 排序——
    // 它是页内位次（每页都是 1~10），rank 只当「有无名次」的标记
    Ok(items)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str) -> RankItem {
        RankItem {
            series_id: id.into(),
            ..Default::default()
        }
    }

    /// 官方样本形状。
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
        assert_eq!(sub.panel[0].items[0].name, "总榜");
        assert_eq!(sub.panel[0].items[0].id, "");
        assert_eq!(sub.panel[0].items[1].id, "gender_female");
        assert_eq!(sub.panel[1].items[0].id, "cate_308");
        assert!(tabs[0].subs[1].panel.is_empty(), "无面板的子榜收空表");
        assert_eq!(tabs[1].subs[0].panel[0].items[0].id, "style_1685");
        assert!(parse_cell_selector(Some(&serde_json::json!({}))).is_empty());
    }

    /// 一级形态（老版本身份的退化结构）样本。
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
    fn empty_id_kept_by_structure_not_by_label() {
        let data: Value = serde_json::json!({
            "cell_view": { "cell_selector": { "outer_row": { "items": [
                {
                    "show_name": "推荐榜",
                    "selector_item_id": "ranklist_hot_sc",
                    "sub_cell_selector": { "outer_row": { "items": [
                        { "show_name": "全部", "selector_item_id": "" },
                        {
                            "show_name": "异常空 id 子榜",
                            "selector_item_id": "",
                            "panel_selector": { "inner_rows": [
                                { "row_name": "画风", "items": [
                                    { "show_name": "3d", "selector_item_id": "style_1685" }
                                ] }
                            ] }
                        }
                    ] }}
                }
            ] } } }
        });
        let tabs = parse_cell_selector(Some(&data));
        assert_eq!(tabs.len(), 1);
        assert_eq!(
            tabs[0].subs.len(),
            1,
            "空 id 叶子保留（文案是「全部」不是「总榜」）"
        );
        assert_eq!(tabs[0].subs[0].name, "全部");
    }

    #[test]
    fn merge_into_book_dedups_keeping_order() {
        let mut book = RankSessionBook::default();
        merge_into_book(&mut book, vec![item("a"), item("b")]);
        merge_into_book(&mut book, vec![item("b"), item("c"), item("b")]);
        let ids: Vec<&str> = book.items.iter().map(|i| i.series_id.as_str()).collect();
        assert_eq!(ids, ["a", "b", "c"], "重叠的 b 与页内重复的 b 都不重付");
    }

    #[test]
    fn book_window_slices_and_terminates() {
        let mut book = RankSessionBook {
            items: vec![item("a"), item("b"), item("c")],
            upstream_has_more: true,
            ..Default::default()
        };
        let (items, has_more) = book_window(&book, 0);
        assert_eq!(
            items
                .iter()
                .map(|i| i.series_id.as_str())
                .collect::<Vec<_>>(),
            ["a", "b", "c"]
        );
        assert!(has_more, "上游未尽即有更多");
        let (items, has_more) = book_window(&book, 30);
        assert!(items.is_empty());
        assert!(has_more);
        book.upstream_has_more = false;
        let (items, has_more) = book_window(&book, 30);
        assert!(items.is_empty());
        assert!(!has_more, "上游已尽且账本浅于窗口=到底");
    }

    #[test]
    fn selector_change_type_fresh_vs_continuation() {
        let fresh = RankReq {
            offset: 0,
            ..Default::default()
        };
        assert_eq!(selector_change_type(&fresh), "2");
        let page2 = RankReq {
            offset: 10,
            session_id: "s1",
            ..Default::default()
        };
        assert_eq!(selector_change_type(&page2), "1");
        let resume = RankReq {
            offset: 0,
            session_id: "s1",
            ..Default::default()
        };
        assert_eq!(selector_change_type(&resume), "1");
    }

    #[test]
    fn rank_version_field_accepts_number_and_string() {
        let data: Value = serde_json::json!({ "rank_version": 1792166880_i64 });
        assert_eq!(
            rank_version_field(Some(&data)).as_deref(),
            Some("1792166880")
        );
        let data: Value = serde_json::json!({ "rank_version": "1792166880" });
        assert_eq!(
            rank_version_field(Some(&data)).as_deref(),
            Some("1792166880")
        );
        assert_eq!(rank_version_field(Some(&serde_json::json!({}))), None);
    }

    #[test]
    fn book_keeps_delivery_order_for_filter_ids() {
        let mut book = RankSessionBook::default();
        merge_into_book(&mut book, vec![item("a"), item("b"), item("c")]);
        let fid = book
            .items
            .iter()
            .map(|i| i.series_id.as_str())
            .collect::<Vec<_>>()
            .join(",");
        assert_eq!(fid, "a,b,c");
    }

    #[test]
    fn parses_background_selector_schema() {
        let data: Value = serde_json::json!({
            "cell_view": { "cell_selector": { "outer_row": { "items": [
                {
                    "show_name": "全部",
                    "selector_item_id": "all",
                    "sub_cell_selector": { "outer_row": { "items": [
                        {
                            "show_name": "必看榜",
                            "selector_item_id": "ranklist_must_watch",
                            "background_selector": { "inner_rows": [
                                { "row_name": "综合", "items": [
                                    { "show_name": "总榜", "selector_item_id": "date_all" }
                                ] },
                                { "row_name": "2026年", "items": [
                                    { "show_name": "10月", "selector_item_id": "date_202610", "is_selected": true },
                                    { "show_name": "9月", "selector_item_id": "date_202609" }
                                ] }
                            ] }
                        }
                    ] }}
                }
            ] } } }
        });
        let tabs = parse_cell_selector(Some(&data));
        let bg = &tabs[0].subs[0].background;
        assert_eq!(bg.len(), 2);
        assert_eq!(bg[0].name, "综合");
        assert_eq!(bg[0].items[0].id, "date_all");
        assert_eq!(bg[1].name, "2026年");
        assert_eq!(bg[1].items[1].id, "date_202609");
    }

    #[test]
    fn rank_items_keep_delivery_order() {
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
        assert_eq!(
            ids,
            vec!["b", "a", "c"],
            "交付序不动，rank 只当有无名次的标记"
        );
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

/// 直连真实接口的探测用例（全部 `#[ignore]`）；`anon_env` / `hg_env`
/// 供 recommend / new_drama / reservation / calendar 各域 probe 复用。
#[cfg(test)]
pub(crate) mod probe {
    use super::*;

    pub(crate) fn anon_env() -> ApiEnv {
        ApiEnv::anonymous(hongguo_common::model::ProxyConfig::default())
    }

    /// 已注册态的探测身份：从 `PROBE_DEVICE` 环境变量读档案 JSON
    /// （sqlite3 -readonly 从壳数据库 device_profile 表 id=1 行导出）。
    pub fn hg_env(base: &ApiEnv) -> ApiEnv {
        let json = std::env::var("PROBE_DEVICE").expect(
            "探测需要注册态身份：设 PROBE_DEVICE=<DeviceProfile JSON>，\
             用 sqlite3 -readonly 从壳数据库 device_profile 表 id=1 行导出",
        );
        let device: hongguo_signer::device::DeviceProfile =
            serde_json::from_str(&json).expect("PROBE_DEVICE 不是合法的 DeviceProfile JSON");
        ApiEnv {
            proxy: base.proxy.clone(),
            cookie: Some(hongguo_signer::device::anonymous_cookie(&device)),
            x_tt_token: None,
            device,
        }
    }

    /// 榜单 cell 的 limit/offset 分页行为。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_rank_paging() {
        let device_json = std::env::var("PROBE_DEVICE").expect("PROBE_DEVICE");
        let mut device: hongguo_signer::device::DeviceProfile =
            serde_json::from_str(&device_json).expect("device json");
        hongguo_signer::device::align_app_version(&mut device);
        let env = ApiEnv {
            proxy: hongguo_common::model::ProxyConfig::default(),
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
                        .map(|d| d.to_string().matches("series_id").count())
                        .unwrap_or(0);
                    println!(
                        "[limit={limit} offset={offset}] keys={:?} next_offset={:?} has_more={:?} series_id_hits={count}",
                        data.map(|d| d
                            .as_object()
                            .map(|o| o.keys().cloned().collect::<Vec<_>>())
                            .unwrap_or_default())
                            .unwrap_or_default(),
                        data.and_then(|d| d.get("next_offset")).cloned(),
                        data.and_then(|d| d.get("has_more")).cloned(),
                    );
                }
                Err(e) => println!("[limit={limit} offset={offset}] ERR {e}"),
            }
        }
    }

    /// 筛选面板 schema 直连，dump 完整响应到 captures/。
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

    /// cell_selector 形态二分：服务端按客户端版本号分发两级 / 一级结构。
    /// 用法：`PROBE_VARIANT=asis|vreg cargo test probe_rank_login_form -- --ignored --nocapture`
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_rank_login_form() {
        let device_json = std::env::var("PROBE_DEVICE").expect("PROBE_DEVICE");
        let account_cookie = std::env::var("PROBE_COOKIE").expect("PROBE_COOKIE");
        let variant = std::env::var("PROBE_VARIANT").unwrap_or_else(|_| "asis".into());

        let mut v: Value = serde_json::from_str(&device_json).expect("device json");
        if variant == "vreg" {
            let bump = [
                ["version_code", hongguo_signer::REGISTER_VERSION_CODE_STR],
                ["version_name", hongguo_signer::REGISTER_VERSION_NAME],
                [
                    "manifest_version_code",
                    hongguo_signer::REGISTER_VERSION_CODE_STR,
                ],
                [
                    "update_version_code",
                    hongguo_signer::REGISTER_VERSION_CODE_STR,
                ],
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
        let device: hongguo_signer::device::DeviceProfile = serde_json::from_value(v).unwrap();
        let cookie = format!(
            "{}; {}",
            hongguo_signer::device::anonymous_cookie(&device),
            account_cookie
        );
        let env = ApiEnv {
            proxy: hongguo_common::model::ProxyConfig::default(),
            device,
            cookie: Some(cookie),
            x_tt_token: None,
        };

        let page = fetch_rank_ex(
            RankReq {
                selected: "all",
                sub: "ranklist_hot_sc",
                ..Default::default()
            },
            &env,
        )
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

    /// 形态二分补充：注册口径版本 + 匿名（区分版本号与会话哪个是开关）。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_rank_login_form_anon() {
        let variant = std::env::var("PROBE_VARIANT").unwrap_or_else(|_| "asis".into());
        let device_json = std::env::var("PROBE_DEVICE").expect("PROBE_DEVICE");
        let mut v: Value = serde_json::from_str(&device_json).expect("device json");
        if variant == "vreg" {
            let fields = v["fields"].as_array_mut().expect("fields");
            for f in fields.iter_mut() {
                match f[0].as_str() {
                    Some("version_code" | "manifest_version_code" | "update_version_code") => {
                        f[1] = serde_json::json!(hongguo_signer::REGISTER_VERSION_CODE_STR);
                    }
                    Some("version_name") => {
                        f[1] = serde_json::json!(hongguo_signer::REGISTER_VERSION_NAME)
                    }
                    _ => {}
                }
            }
        }
        let device: hongguo_signer::device::DeviceProfile = serde_json::from_value(v).unwrap();
        let env = ApiEnv {
            proxy: hongguo_common::model::ProxyConfig::default(),
            cookie: Some(hongguo_signer::device::anonymous_cookie(&device)),
            device,
            x_tt_token: None,
        };
        let page = fetch_rank_ex(
            RankReq {
                selected: "all",
                sub: "ranklist_hot_sc",
                ..Default::default()
            },
            &env,
        )
        .await
        .expect("榜单请求");
        println!("[anon/{variant}] tabs={} 第一层 id:", page.tabs.len());
        for tab in &page.tabs {
            let subs: Vec<&str> = tab.subs.iter().map(|s| s.name.as_str()).collect();
            println!("  {:?} {:?} subs={:?}", tab.id, tab.name, subs);
        }
    }

    /// 排行榜内容 tab「演员」形态验证（ranklist_celebrity；
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
        let base = fetch_rank_ex(
            RankReq {
                selected: "all",
                sub: "ranklist_hot_sc",
                ..Default::default()
            },
            &env,
        )
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
            let page = fetch_rank_ex(
                RankReq {
                    selected: "all",
                    sub: "ranklist_hot_sc",
                    panel: Some(p),
                    ..Default::default()
                },
                &env,
            )
            .await
            .unwrap_or_else(|e| panic!("panel={p}: {e}"));
            println!(
                "[panel:{p}] {} 条 #1={:?}",
                page.items.len(),
                page.items.first().map(|i| i.title.as_str())
            );
            assert!(!page.items.is_empty(), "panel={p} 不应为空");
        }

        // 内容 tab 切换（真人剧 / 漫剧 / 系列剧，官方样本 id）
        for (sel, sub) in [
            ("human", "human_hot_sc"),
            ("comic_series_rank", "comic_series_hot_rank"),
            ("series_album", "series_album_hot_sc"),
        ] {
            let page = fetch_rank_ex(
                RankReq {
                    selected: sel,
                    sub,
                    ..Default::default()
                },
                &env,
            )
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

    /// app 生产同款设备（匿名档案 + 匿名 cookie）下发的 selector schema：
    /// 排查两端 tabs 是否一致（灰度可能按设备分发）。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_rank_selector_app_device() {
        let device = hongguo_signer::anonymous_profile(&hongguo_signer::fresh_cdid());
        let env = ApiEnv {
            proxy: hongguo_common::model::ProxyConfig::default(),
            cookie: Some(hongguo_signer::device::anonymous_cookie(&device)),
            device,
            x_tt_token: None,
        };
        let page = fetch_rank_ex(
            RankReq {
                selected: "all",
                sub: "ranklist_hot_sc",
                ..Default::default()
            },
            &env,
        )
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

    /// 翻页协议真连验证：首页（usct=2 无
    /// 会话）→ 翻页（usct=1 + session_id + rank_version 回传，filter_ids
    /// 由会话账本自动带上），两页条目应为 10 条上下且第二页出现新条目。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_rank_paging_session() {
        let env = hg_env(&anon_env());
        let page1 = fetch_rank_ex(
            RankReq {
                selected: "all",
                sub: "ranklist_must_watch",
                ..Default::default()
            },
            &env,
        )
        .await
        .expect("必看榜首页");
        println!(
            "[page1] {} 条 #1={:?} rv={} sess={}",
            page1.items.len(),
            page1.items.first().map(|i| i.title.as_str()),
            page1.rank_version,
            page1.session_id,
        );
        assert!(!page1.session_id.is_empty(), "首页应下发会话");
        assert!(
            page1.next_offset > 0 && page1.has_more,
            "首页应下发翻页游标"
        );

        let page2 = fetch_rank_ex(
            RankReq {
                selected: "all",
                sub: "ranklist_must_watch",
                offset: page1.next_offset,
                session_id: &page1.session_id,
                rank_version: &page1.rank_version,
                ..Default::default()
            },
            &env,
        )
        .await
        .expect("必看榜翻页");
        println!(
            "[page2] {} 条 #1={:?} 去重后 {} 条",
            page2.items.len(),
            page2.items.first().map(|i| i.title.as_str()),
            page2.items.len(),
        );
        let p1_ids: std::collections::HashSet<_> =
            page1.items.iter().map(|i| i.series_id.as_str()).collect();
        let fresh = page2
            .items
            .iter()
            .filter(|i| !p1_ids.contains(i.series_id.as_str()))
            .count();
        println!("[page2] 与首页不重复条目 {fresh}");
        assert!(
            !page2.items.is_empty(),
            "翻页不应为空（usct=1 + filter_ids 形态）"
        );
    }

    /// 深滚逐页对照：连拉到 has_more=false，逐页打印与官方 diff。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_rank_deep_paging_diff() {
        let env = hg_env(&anon_env());
        let mut offset = 0i64;
        let mut session = String::new();
        let mut rank_version = String::new();
        let mut page_no = 0;
        loop {
            page_no += 1;
            let page = fetch_rank_ex(
                RankReq {
                    selected: "all",
                    sub: "ranklist_hot_play_sc",
                    offset,
                    session_id: &session,
                    rank_version: &rank_version,
                    ..Default::default()
                },
                &env,
            )
            .await
            .expect("榜单页");
            let ids: Vec<String> = page
                .items
                .iter()
                .map(|i| i.series_id[i.series_id.len().saturating_sub(6)..].to_string())
                .collect();
            println!(
                "[p{page_no}] items={} ids={ids:?} next={} has_more={}",
                page.items.len(),
                page.next_offset,
                page.has_more,
            );
            let (next_offset, has_more) = (page.next_offset, page.has_more);
            session = page.session_id;
            rank_version = page.rank_version;
            offset = next_offset;
            if !has_more || next_offset <= 0 || page_no >= 12 {
                break;
            }
        }
    }

    /// 重挂载重拉模拟：翻 3 页后按前端重拉形态再请求第 2 页，
    /// 判定服务端回原窗口还是跳过已见。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_rank_refetch_simulation() {
        let env = hg_env(&anon_env());
        let mut offset = 0i64;
        let mut session = String::new();
        let mut rank_version = String::new();
        let mut pass1 = Vec::new();
        for _ in 0..3 {
            let page = fetch_rank_ex(
                RankReq {
                    selected: "all",
                    sub: "ranklist_hot_play_sc",
                    offset,
                    session_id: &session,
                    rank_version: &rank_version,
                    ..Default::default()
                },
                &env,
            )
            .await
            .expect("首轮翻页");
            pass1.extend(page.items.iter().map(|i| tail6(&i.series_id)));
            session = page.session_id;
            rank_version = page.rank_version;
            offset = page.next_offset;
        }
        println!("[首轮3页] 共 {} 条: {pass1:?}", pass1.len());

        let p1 = fetch_rank_ex(
            RankReq {
                selected: "all",
                sub: "ranklist_hot_play_sc",
                ..Default::default()
            },
            &env,
        )
        .await
        .expect("重拉首页");
        println!("[重拉p1] {} 条", p1.items.len());
        let p2 = fetch_rank_ex(
            RankReq {
                selected: "all",
                sub: "ranklist_hot_play_sc",
                offset: 10,
                session_id: &session,
                rank_version: &rank_version,
                ..Default::default()
            },
            &env,
        )
        .await
        .expect("重拉第2页（旧会话满 fid）");
        let p2_ids: Vec<String> = p2.items.iter().map(|i| tail6(&i.series_id)).collect();
        println!("[重拉p2] {} 条: {p2_ids:?}", p2.items.len());
        let head: Vec<String> = pass1.iter().skip(10).take(10).cloned().collect();
        println!("[判定] 首轮第 11~20 位 = {head:?}");
        println!(
            "[判定] 重拉 p2 {}（原窗口=未被 fid 排异；跳新条目=fid 全局排异）",
            if p2_ids == head {
                "回原窗口"
            } else {
                "跳过已见"
            }
        );
    }

    fn tail6(id: &str) -> String {
        id[id.len().saturating_sub(6)..].to_string()
    }

    /// 排行榜直连：8 个「全部」tab 子榜逐个打，code==0 且条目数 >0 即转正。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_rank_all_lists() {
        let env = hg_env(&anon_env());
        for sub in ALL_TAB_SUBS {
            match fetch_rank_ex(
                RankReq {
                    selected: "all",
                    sub,
                    ..Default::default()
                },
                &env,
            )
            .await
            {
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
