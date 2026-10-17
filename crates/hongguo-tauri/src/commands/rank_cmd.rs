//! 排行榜 / 新剧 / 搜索 / 预约 command（2026-10-03 抓包端点）。

use tauri::State;

use hongguo_api::calendar::{CalendarPage, fetch_new_calendar};
use hongguo_api::new_drama::fetch_new_drama;
use hongguo_api::rank::{RankPage, RankReq, fetch_rank_ex};
use hongguo_api::reservation::{delete_reservations, fetch_reservations_page, reserve_series};
use hongguo_api::search::{SearchPage, SuggestItem, search_series, search_suggest};
use hongguo_common::{AppError, AppResult};
use hongguo_service::app_state::AppState;

/// 拉一个榜单（任意 tab × 子榜 × 筛选 × 月榜组合；offset 游标翻页）。
///
/// - `selected`：内容 tab id（all/human/comic_series_rank/ai_playlet/
///   ranklist_celebrity/series_album；空串按 all 归一）
/// - `sub`：子榜 id（响应 tabs schema 下发，如 ranklist_hot_sc）
/// - `panel`：筛选面板选中项（gender_female/cate_308…；None 或空 = 总榜）
/// - `background`：榜单历史选中项（date_202609 = 2026 年 9 月榜；None/空
///   = 当前月）
/// - `offset`/`session_id`：翻页游标；`rank_version`：响应下发的版本戳回传
#[expect(
    clippy::too_many_arguments,
    reason = "平铺参数表与 registry.ts 的 IPC 契约一一对应，包结构体反而要二次拆装"
)]
#[tauri::command]
pub async fn rank_list(
    state: State<'_, AppState>,
    selected: String,
    sub: String,
    panel: Option<String>,
    background: Option<String>,
    offset: Option<i64>,
    session_id: Option<String>,
    rank_version: Option<String>,
) -> AppResult<RankPage> {
    let env = state.api_env();
    fetch_rank_ex(
        RankReq {
            selected: &selected,
            sub: &sub,
            panel: panel.as_deref(),
            background: background.as_deref(),
            offset: offset.unwrap_or(0),
            session_id: session_id.as_deref().unwrap_or(""),
            rank_version: rank_version.as_deref().unwrap_or(""),
        },
        &env,
    )
    .await
}

/// 新剧推荐（gender: 2=全部；offset 翻页步长 18；session_id 翻页回传）。
#[tauri::command]
pub async fn new_drama_list(
    state: State<'_, AppState>,
    gender: Option<i64>,
    offset: Option<i64>,
    session_id: Option<String>,
) -> AppResult<RankPage> {
    let env = state.api_env();
    fetch_new_drama(
        gender.unwrap_or(2),
        offset.unwrap_or(0),
        session_id.as_deref(),
        &env,
    )
    .await
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

/// 搜索联想（输入 2~3 字返回相关剧集；空词空表不打接口）。
#[tauri::command]
pub async fn search_suggest_cmd(
    state: State<'_, AppState>,
    q: String,
) -> AppResult<Vec<SuggestItem>> {
    let q = q.trim();
    if q.is_empty() {
        return Ok(Vec::new());
    }
    let env = state.api_env();
    match search_suggest(q, &env).await {
        Ok(items) => {
            log::debug!("[Suggest] q={q} 命中 {} 条", items.len());
            Ok(items)
        }
        Err(e) => {
            // 联想是纯增强：失败只记日志不上抛，输入框照常手动搜索
            log::warn!("[Suggest] q={q} 失败: {e}");
            Ok(Vec::new())
        }
    }
}

/// 我的预约一页（is_online=true 已上线 / false 待上线；匿名空表）。
///
/// 单页即回，`offset`/`sessionId` 翻页续传（响应下发回传）——预约页
/// 懒翻页用；此前后端一次性串行拉全，预约上百部时首屏要等几十个请求
/// （issue #11 的「已上线 5–8 秒」）。
#[tauri::command]
pub async fn reservation_list(
    state: State<'_, AppState>,
    is_online: Option<bool>,
    offset: Option<i64>,
    session_id: Option<String>,
) -> AppResult<CalendarPage> {
    let env = state.api_env();
    fetch_reservations_page(
        is_online.unwrap_or(true),
        offset.unwrap_or(0),
        session_id.as_deref().filter(|s| !s.is_empty()),
        &env,
    )
    .await
}

/// 预约 / 取消预约（需要登录；series_id 为剧集 id）。
#[tauri::command]
pub async fn reservation_reserve(
    state: State<'_, AppState>,
    series_id: String,
    reserve: Option<bool>,
) -> AppResult<()> {
    let series_id = series_id.trim().to_string();
    if series_id.is_empty() {
        return Err(AppError::Auth("剧集 id 不能为空".into()));
    }
    if state.settings().account.is_none() {
        return Err(AppError::Auth("预约需要先登录".into()));
    }
    let env = state.api_env();
    let reserve = reserve.unwrap_or(true);
    reserve_series(&series_id, reserve, &env).await?;
    log::info!(
        "[Reserve] {} {series_id}",
        if reserve { "预约" } else { "取消预约" }
    );
    Ok(())
}

/// 批量删除预约（subscribe_delete，hgplayer 1.1.8 同款；需要登录）。
#[tauri::command]
pub async fn reservations_delete(
    state: State<'_, AppState>,
    item_ids: Vec<String>,
    all_select: Option<bool>,
    not_del_item_ids: Option<Vec<String>>,
    is_online: Option<bool>,
) -> AppResult<()> {
    if state.settings().account.is_none() {
        return Err(AppError::Auth("预约删除需要先登录".into()));
    }
    let env = state.api_env();
    let all_select = all_select.unwrap_or(false);
    let not_del = not_del_item_ids.unwrap_or_default();
    delete_reservations(
        &item_ids,
        all_select,
        &not_del,
        is_online.unwrap_or(true),
        &env,
    )
    .await?;
    log::info!("[Reserve] 批量删除 {} 条预约", item_ids.len());
    Ok(())
}

/// 上新日历（date 传返回值 dates 里的日期可切换，不传取默认日；
/// offset/session_id 是上一页响应的游标——单页返回，前端滚动到底懒翻页）。
#[tauri::command]
pub async fn new_drama_calendar(
    state: State<'_, AppState>,
    date: Option<String>,
    offset: Option<i64>,
    session_id: Option<String>,
) -> AppResult<CalendarPage> {
    let env = state.api_env();
    let cursor = offset.map(|o| (o, session_id.as_deref().unwrap_or_default()));
    fetch_new_calendar(date.as_deref(), cursor, &env).await
}

#[cfg(test)]
mod tests {
    #[test]
    fn rank_commands_are_async() {
        // 直连接口的命令必须 async（与 discover_feed 同一条纪律）
        let src = include_str!("rank_cmd.rs");
        for name in [
            "fn rank_list(",
            "fn new_drama_list(",
            "fn search_series_cmd(",
            "fn reservation_list(",
            "fn reservation_reserve(",
            "fn new_drama_calendar(",
        ] {
            let sig = src
                .lines()
                .find(|l| l.contains(name))
                .unwrap_or_else(|| panic!("找不到 {name}"));
            assert!(sig.contains("pub async fn"), "{name} 必须 async: {sig}");
        }
    }
}
