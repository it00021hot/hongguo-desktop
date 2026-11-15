use crate::AppState;
use hongguo_api::{calendar, danmaku, detail, discover, new_drama, rank, recommend, search};
use hongguo_common::AppResult;

pub async fn feed_tabs(state: &AppState) -> AppResult<Vec<recommend::FeedTabMeta>> {
    recommend::fetch_feed_tabs(&state.api_env()).await
}

pub async fn recommendations(
    tab: &str,
    session_id: &str,
    offset: i64,
    filter_ids: &[String],
    state: &AppState,
) -> AppResult<discover::FeedPage> {
    recommend::fetch_recommend_feed(tab, session_id, offset, filter_ids, &state.api_env()).await
}

pub async fn calendar(
    date: Option<&str>,
    cursor: Option<(i64, &str)>,
    state: &AppState,
) -> AppResult<calendar::CalendarPage> {
    calendar::fetch_new_calendar(date, cursor, &state.api_env()).await
}

pub async fn new_drama(
    gender: i64,
    offset: i64,
    session_id: Option<&str>,
    state: &AppState,
) -> AppResult<rank::RankPage> {
    new_drama::fetch_new_drama(gender, offset, session_id, &state.api_env()).await
}

pub async fn rank(req: rank::RankReq<'_>, state: &AppState) -> AppResult<rank::RankPage> {
    rank::fetch_rank_ex(req, &state.api_env()).await
}

pub async fn browse_panel(state: &AppState) -> AppResult<Vec<discover::SelectorRow>> {
    discover::fetch_browse_panel(&state.api_env()).await
}

pub async fn browse(
    filters: &discover::BrowseFilters,
    offset: i64,
    session_id: &str,
    state: &AppState,
) -> AppResult<discover::FeedPage> {
    discover::fetch_browse(filters, offset, session_id, &state.api_env()).await
}

pub async fn search(
    query: &str,
    offset: i64,
    search_id: &str,
    state: &AppState,
) -> AppResult<search::SearchPage> {
    search::search_series(query, offset, search_id, &state.api_env()).await
}

pub async fn suggestions(query: &str, state: &AppState) -> AppResult<Vec<search::SuggestItem>> {
    search::search_suggest(query, &state.api_env()).await
}

pub async fn metadata(id: &str, state: &AppState) -> AppResult<detail::SeriesMeta> {
    detail::fetch_series_meta(id, &state.api_env()).await
}

pub async fn related(id: &str, state: &AppState) -> AppResult<detail::RelatedSeries> {
    detail::fetch_related_series(id, &state.api_env()).await
}

pub async fn creator(id: &str, state: &AppState) -> AppResult<danmaku::CreatorProfile> {
    danmaku::fetch_creator_profile(id, &state.api_env()).await
}

pub async fn creator_works(
    id: &str,
    cursor: &str,
    state: &AppState,
) -> AppResult<danmaku::CreatorWorksPage> {
    danmaku::fetch_creator_works(id, cursor, &state.api_env()).await
}

pub async fn danmaku(
    vid: &str,
    series_id: &str,
    start_ms: u64,
    duration_ms: u64,
    cursor: &str,
    state: &AppState,
) -> AppResult<danmaku::DanmakuWindowPage> {
    danmaku::fetch_danmaku_window_page(
        vid,
        series_id,
        start_ms,
        duration_ms,
        cursor,
        &state.api_env(),
    )
    .await
}
