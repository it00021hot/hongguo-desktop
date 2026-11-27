use crate::AppState;
use hongguo_api::{calendar::CalendarPage, reservation};
use hongguo_common::{AppError, AppResult};
use std::collections::HashSet;

fn require_account(state: &AppState) -> AppResult<()> {
    state
        .settings()
        .account
        .as_ref()
        .ok_or_else(|| AppError::Auth("未登录".into()))?;
    Ok(())
}

pub async fn list(online: bool, state: &AppState) -> AppResult<CalendarPage> {
    require_account(state)?;
    reservation::fetch_reservations(online, &state.api_env()).await
}

pub async fn memberships(state: &AppState) -> AppResult<HashSet<String>> {
    require_account(state)?;
    let env = state.api_env();
    let (offline, online) = tokio::try_join!(
        reservation::fetch_reservations(false, &env),
        reservation::fetch_reservations(true, &env),
    )?;
    Ok(offline
        .items
        .into_iter()
        .chain(online.items)
        .map(|item| item.series_id)
        .collect())
}

pub async fn contains(id: &str, state: &AppState) -> AppResult<bool> {
    require_account(state)?;
    let env = state.api_env();
    let offline = reservation::fetch_reservations(false, &env).await?;
    let online = reservation::fetch_reservations(true, &env).await?;
    Ok(offline
        .items
        .iter()
        .chain(&online.items)
        .any(|item| item.series_id == id))
}

pub async fn toggle(id: &str, selected: bool, state: &AppState) -> AppResult<()> {
    require_account(state)?;
    reservation::reserve_series(id, selected, &state.api_env()).await
}

pub use reservation::{
    SelectorId, SelectorOption, SelectorRow, SelectorSelection, UpcomingAudience, UpcomingFilters,
    UpcomingItem, UpcomingPage,
};

pub async fn upcoming(
    filters: &UpcomingFilters,
    offset: i64,
    session_id: Option<&str>,
    state: &AppState,
) -> AppResult<UpcomingPage> {
    reservation::fetch_upcoming(filters, offset, session_id, &state.api_env()).await
}

pub async fn upcoming_panel(state: &AppState) -> AppResult<Vec<SelectorRow>> {
    reservation::fetch_upcoming_panel(&state.api_env()).await
}
