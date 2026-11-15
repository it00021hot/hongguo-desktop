use super::model::HistoryItem;
use hongguo_common::{AppError, AppResult};
use hongguo_service::{
    AppState,
    history_service::{self, HistoryAccount},
};

pub(super) async fn load(state: AppState) -> AppResult<Vec<HistoryItem>> {
    let expected_account = history_service::account(&state);
    history_service::list(&state, expected_account)
        .await
        .map(|page| page.items.into_iter().map(HistoryItem::from).collect())
}

pub(super) fn identity(state: &AppState) -> Option<HistoryAccount> {
    history_service::account(state)
}

pub(super) async fn remove(
    state: AppState,
    entries: Vec<HistoryItem>,
    expected_user: Option<HistoryAccount>,
) -> AppResult<()> {
    if identity(&state) != expected_user {
        return Err(AppError::Auth("history identity changed".into()));
    }
    let refs = entries
        .into_iter()
        .map(|item| (item.id, item.vid, i64::from(item.episode)))
        .collect::<Vec<_>>();
    history_service::delete(&state, &refs, expected_user).await
}
