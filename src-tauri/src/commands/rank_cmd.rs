//! 排行榜 / 新剧 / 搜索 / 预约 command（2026-10-03 抓包端点）。

use serde::Deserialize;
use tauri::State;

use crate::app_state::AppState;
use crate::domain::api::rank::{
    fetch_new_calendar, fetch_new_drama, fetch_rank, fetch_reservations, CalendarPage, RankList,
    RankPage,
};
use crate::domain::api::search::{search_series, SearchPage};
use crate::error::AppResult;

/// 榜单标识（前端传 `sub_selected_items` 字符串，服务端校验白名单）。
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RankKind {
    Recommend,
    HotPlay,
    Prestige,
    Subscribe,
    NewDrama,
    HotSearch,
    MustWatch,
    Followed,
}

impl From<RankKind> for RankList {
    fn from(k: RankKind) -> Self {
        match k {
            RankKind::Recommend => RankList::Recommend,
            RankKind::HotPlay => RankList::HotPlay,
            RankKind::Prestige => RankList::Prestige,
            RankKind::Subscribe => RankList::Subscribe,
            RankKind::NewDrama => RankList::NewDrama,
            RankKind::HotSearch => RankList::HotSearch,
            RankKind::MustWatch => RankList::MustWatch,
            RankKind::Followed => RankList::Followed,
        }
    }
}

/// 拉一个榜单。
#[tauri::command]
pub async fn rank_list(
    state: State<'_, AppState>,
    kind: RankKind,
) -> AppResult<RankPage> {
    let env = state.api_env();
    fetch_rank(kind.into(), &env).await
}

/// 新剧推荐（gender: 2=全部；offset 翻页步长 18）。
#[tauri::command]
pub async fn new_drama_list(
    state: State<'_, AppState>,
    gender: Option<i64>,
    offset: Option<i64>,
) -> AppResult<RankPage> {
    let env = state.api_env();
    fetch_new_drama(gender.unwrap_or(2), offset.unwrap_or(0), &env).await
}

/// 找剧搜索（首页 offset=0 且不传 search_id；翻页传上一页返回值）。
#[tauri::command]
pub async fn search_series_cmd(
    state: State<'_, AppState>,
    query: String,
    offset: Option<i64>,
    search_id: Option<String>,
) -> AppResult<SearchPage> {
    let query = query.trim();
    if query.is_empty() {
        return Ok(SearchPage::default());
    }
    let env = state.api_env();
    match search_series(
        query,
        offset.unwrap_or(0),
        search_id.as_deref().unwrap_or(""),
        &env,
    )
    .await
    {
        Ok(page) => {
            log::info!(
                "[Search] query={query} offset={} 命中 {} 条 has_more={}",
                offset.unwrap_or(0),
                page.items.len(),
                page.has_more
            );
            Ok(page)
        }
        Err(e) => {
            log::warn!("[Search] query={query} 失败: {e}");
            Err(e)
        }
    }
}

/// 我的预约（is_online=true 已上线 / false 待上线；匿名通常空表）。
#[tauri::command]
pub async fn reservation_list(
    state: State<'_, AppState>,
    is_online: Option<bool>,
) -> AppResult<RankPage> {
    let env = state.api_env();
    fetch_reservations(is_online.unwrap_or(true), &env).await
}

/// 上新日历（date 传返回值 dates 里的日期可切换，不传取默认日）。
#[tauri::command]
pub async fn new_drama_calendar(
    state: State<'_, AppState>,
    date: Option<String>,
) -> AppResult<CalendarPage> {
    let env = state.api_env();
    fetch_new_calendar(date.as_deref(), &env).await
}

#[cfg(test)]
mod tests {
    use super::{RankKind, RankList};

    #[test]
    fn rank_commands_are_async() {
        // 直连接口的命令必须 async（与 discover_feed 同一条纪律）
        let src = include_str!("rank_cmd.rs");
        for name in ["fn rank_list(", "fn new_drama_list(", "fn search_series_cmd(", "fn reservation_list("] {
            let sig = src
                .lines()
                .find(|l| l.contains(name))
                .unwrap_or_else(|| panic!("找不到 {name}"));
            assert!(sig.contains("pub async fn"), "{name} 必须 async: {sig}");
        }
    }

    #[test]
    fn rank_kind_deserializes_snake_case() {
        let k: RankKind = serde_json::from_str("\"must_watch\"").unwrap();
        assert_eq!(RankList::from(k).as_sub_selected(), "ranklist_must_watch");
        let k: RankKind = serde_json::from_str("\"recommend\"").unwrap();
        assert_eq!(RankList::from(k).as_sub_selected(), "ranklist_hot_sc");
    }
}
