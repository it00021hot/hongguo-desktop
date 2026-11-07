//! 排行榜会话账本：上游页拉取、账本合并与窗口切片。

use serde_json::Value;

use super::parse::{parse_cell_selector, parse_rank_items, rank_version_field};
use super::{RANK_CELL_PATH, RankItem, RankPage, RankReq, RankTab};
use crate::client::{ApiEnv, api_call_reading};
use crate::danmaku::LQ_API_ORIGIN;
use crate::discover::check_code;
use hongguo_common::{AppError, AppResult};

pub(super) fn selector_change_type(req: &RankReq<'_>) -> &'static str {
    if req.offset == 0 && req.session_id.is_empty() {
        "2"
    } else {
        "1"
    }
}

/// 对外窗口大小（每次返回会话榜单的 10 条切片）。
pub(super) const RANK_WINDOW: usize = 10;

/// 单次请求最多向上游补的页数（护栏：上游 has_more 异常常真时防死循环）。
pub(super) const RANK_FILL_PAGES_MAX: usize = 5;

/// 会话账本里会话数的上限（超出整体重置）。
pub(super) const RANK_BOOK_SESSIONS: usize = 32;

/// 一个浏览会话的累积榜单视图。
#[derive(Default)]
pub(super) struct RankSessionBook {
    /// 累积条目（官方榜单序）
    pub(super) items: Vec<RankItem>,
    /// 上游会话 id（空 = 尚未向上游开过会话）
    pub(super) upstream_session: String,
    /// 最近一次上游响应下发的版本戳
    pub(super) rank_version: String,
    /// 上游是否还有更多
    pub(super) upstream_has_more: bool,
    /// 上游续载游标（= 最近一次响应的 next_offset）
    pub(super) upstream_offset: i64,
    /// 选项表（每次响应都随行下发，存最新一份）
    pub(super) tabs: Vec<RankTab>,
}

pub(super) fn session_books()
-> &'static std::sync::Mutex<std::collections::HashMap<String, RankSessionBook>> {
    static BOOKS: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<String, RankSessionBook>>,
    > = std::sync::OnceLock::new();
    BOOKS.get_or_init(std::sync::Mutex::default)
}

/// 合并一页上游条目进账本：按 series_id 去重，保持交付序。
pub(super) fn merge_into_book(book: &mut RankSessionBook, items: Vec<RankItem>) {
    let seen: std::collections::HashSet<String> =
        book.items.iter().map(|i| i.series_id.clone()).collect();
    book.items
        .extend(items.into_iter().filter(|i| !seen.contains(&i.series_id)));
}

/// 会话榜单的 `[offset, offset+10)` 窗口切片。
pub(super) fn book_window(book: &RankSessionBook, offset: usize) -> (Vec<RankItem>, bool) {
    let end = (offset + RANK_WINDOW).min(book.items.len());
    let items = book.items[offset.min(end)..end].to_vec();
    let has_more = book.items.len() > end || book.upstream_has_more;
    (items, has_more)
}

pub(super) struct UpstreamPage {
    items: Vec<RankItem>,
    tabs: Vec<RankTab>,
    session: String,
    rank_version: String,
    has_more: bool,
    next_offset: i64,
}

/// 打一页上游（首屏无会话，续载带 session + filter_ids）。
pub(super) async fn fetch_upstream(
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

pub(super) fn fallback_session() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    format!("local-{}", now.as_millis())
}

pub(super) fn store_book(session_id: &str, book: RankSessionBook) {
    let mut books = session_books().lock().unwrap_or_else(|p| p.into_inner());
    if !books.contains_key(session_id) && books.len() >= RANK_BOOK_SESSIONS {
        books.clear();
    }
    books.insert(session_id.to_string(), book);
}

/// 拉任意组合的榜单窗口。**对外契约是窗口而非上游页**：同一
/// (session, offset) 重复请求恒得同一批条目——后端持有累积账本，
/// 窗口从账本切片（幂等），账本不够深才按官方协议顺序向上游补页。
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
